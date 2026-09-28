//! Preview links: one working copy, without a token, until they expire.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// Turn preview links on for a collection.
///
/// Off until an administrator asks for them (`SchemaSettings`), so every test that mints a link
/// goes through here first - which is also what the contract says a deployment looks like.
async fn allow_collection_preview(app: &TestApp, collection: &str) {
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/{collection}/settings"),
        Some(&app.admin_token),
        Some(json!({ "preview": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "enabling preview for {collection}");
}

/// The same for a single page.
async fn allow_page_preview(app: &TestApp, page: &str) {
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/single_pages/{page}/settings"),
        Some(&app.admin_token),
        Some(json!({ "preview": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "enabling preview for {page}");
}

/// A preview link shows one working copy to someone with no account, until it expires.
#[tokio::test]
async fn a_preview_link_shows_one_working_copy_without_a_token() {
    let app = test_app().await;
    // The first call also creates the collection's schema, so the second item goes straight
    // in.
    let first = create_sample_item(&app, "blog").await;
    allow_collection_preview(&app, "blog").await;
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&app.admin_token),
        Some(json!({ "title": "Second", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = body.as_u64().expect("the second item id");

    // An unpublished edit is what a preview is for.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/blog/items/{first}"),
        Some(&app.admin_token),
        Some(json!({ "title": "Draft wording", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mint = |id: u64| {
        let app = &app;
        async move {
            let (status, body) = send(
                &app.router,
                Method::POST,
                &format!("/api/models/collections/blog/items/{id}/preview-link"),
                Some(&app.admin_token),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "preview link for {id}");
            body
        }
    };

    let link = mint(first).await;
    let path = link["path"].as_str().unwrap().to_string();
    assert!(path.starts_with(&format!(
        "/api/preview/collections/blog/items/{first}?token="
    )));
    assert!(link["expires_at"].is_string());

    // Opening it needs no token: the signature is the credential.
    let (status, body) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], first);
    assert_eq!(
        body["values"]["title"], "Draft wording",
        "作業コピーが見える"
    );
    assert_eq!(body["schema"][0]["name"], "title");

    // Looking at a preview publishes nothing.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/api/content/collections/blog/items/{first}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A link for the other item does not open this one - the target is part of what was
    // signed, so a holder cannot go wandering.
    let other_path = mint(second).await["path"].as_str().unwrap().to_string();
    let (status, _) = send(&app.router, Method::GET, &other_path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    let swapped = path.replace(&format!("items/{first}"), &format!("items/{second}"));
    let (status, _) = send(&app.router, Method::GET, &swapped, None, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "差し替えたリンクは通らない"
    );

    // A tampered token, a wrong one, and no token at all.
    let (status, _) = send(&app.router, Method::GET, &format!("{path}x"), None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/api/preview/collections/blog/items/{first}?token=1.deadbeef"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/api/preview/collections/blog/items/{first}"),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "トークンが無ければクエリとして不正"
    );
}

/// The same for a single page, and the kind of target is signed too: a page's link cannot be
/// pointed at a collection item or the other way round.
#[tokio::test]
async fn a_single_page_preview_link_is_public_and_only_opens_that_page() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    allow_page_preview(&app, "home").await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Unpublished home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/preview-link",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = body["path"].as_str().unwrap().to_string();
    assert!(path.starts_with("/api/preview/single_pages/home?token="));

    let (status, body) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Unpublished home");
    assert_eq!(body["schema"][0]["name"], "title");

    // Another page's link, and a link for a collection item, are both refused.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &path.replace("single_pages/home", "single_pages/other"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &path.replace(
            "/api/preview/single_pages/home",
            "/api/preview/collections/home/items/1",
        ),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A page that does not exist has no link to mint.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/missing/preview-link",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// An expired link is refused, and told apart from one that was never ours.
#[tokio::test]
async fn an_expired_preview_link_is_refused() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    // Minted two hours in the past with a one-hour life, which is what waiting would take.
    let expired = app.module.preview_links.issue(
        &PreviewTarget::CollectionItem {
            collection: "blog".to_string(),
            item_id,
        },
        chrono::Utc::now() - chrono::Duration::hours(2),
    );

    let (status, _) = send(&app.router, Method::GET, &expired.path, None, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "期限切れは 403");

    // The same link with the expiry pushed out is not ours, which is the whole point of
    // signing the deadline.
    let (_, signature) = expired.path.rsplit_once('.').unwrap();
    let forged = format!(
        "/api/preview/collections/blog/items/{item_id}?token={}.{signature}",
        (chrono::Utc::now() + chrono::Duration::days(30)).timestamp()
    );
    let (status, _) = send(&app.router, Method::GET, &forged, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Preview is off until an administrator turns it on, per schema, and turning it off again closes
/// the links that are already out there.
#[tokio::test]
async fn preview_links_are_off_until_a_schema_allows_them() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    // A collection that has never been given settings says so, rather than 404 - the screen that
    // offers a preview link has to be able to ask.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/settings",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "preview": false }), "既定は無効");

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item_id}/preview-link"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "preview_disabled");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("the collection 'blog'"),
        "どのスキーマか名指しする: {}",
        body["message"]
    );

    allow_collection_preview(&app, "blog").await;
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/settings",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["preview"], true, "設定は保存される");

    let (status, link) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item_id}/preview-link"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = link["path"].as_str().unwrap().to_string();
    let (status, _) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);

    // Turning it off closes that link too: a link lives for minutes and there is no list of them.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/collections/blog/settings",
        Some(&app.admin_token),
        Some(json!({ "preview": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "無効にしたら既存リンクも閉じる"
    );
    assert_eq!(body["code"], "preview_disabled");

    // A page has the same setting, and it is the page's own.
    let (status, _body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/settings",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "無いページは 404");
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/settings",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["preview"], false);
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/preview-link",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "preview_disabled");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("the single page 'home'"),
        "ページも名指しする: {}",
        body["message"]
    );
    allow_page_preview(&app, "home").await;
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/preview-link",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Settings belong to the schema they were given to: one recreated under the same name starts with
/// none, which is what makes deleting a collection a clean slate rather than an inheritance.
#[tokio::test]
async fn settings_do_not_outlive_the_schema_they_were_given_to() {
    let app = test_app().await;
    create_sample_item(&app, "blog").await;
    allow_collection_preview(&app, "blog").await;

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/collections/blog",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/schema",
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/settings",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["preview"], false, "作り直したコレクションは既定に戻る");

    // And the same for a page, which has its own record.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    allow_page_preview(&app, "home").await;
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/single_pages/home",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/settings",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["preview"], false, "作り直したページも既定に戻る");
}

/// Turning preview on lets unpublished content leave the CMS, so it is an administrator's act -
/// like creating the schema it belongs to.
#[tokio::test]
async fn only_an_administrator_may_change_which_schemas_allow_previews() {
    let app = test_app().await;
    create_sample_item(&app, "blog").await;
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    let viewer = body["token"].as_str().unwrap().to_string();

    // Reading is part of the editor's screen, so it is allowed.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/settings",
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/collections/blog/settings",
        Some(&viewer),
        Some(json!({ "preview": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "編集者は設定を変えられない");

    // Nothing changed, so there is still no link to be had - refused for the reader's role before
    // the setting is ever consulted, which is the order the middleware settles.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/1/preview-link",
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "forbidden");

    // An administrator, with the setting still off, is refused for the setting instead.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/1/preview-link",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "preview_disabled");
}

/// An editor - `can_edit`, and not an administrator - can issue a link, and it is a real one.
///
/// The two rules are different and easy to run together. *Issuing* is the middleware's write rule
/// (`can_edit`), because an editor can already read the draft the link shows and reviewing it with
/// a client is what the link is for (`create_collection_item_preview_link` in the CMS says so).
/// *Enabling* is administration: which Schemas may be previewed is a `PUT .../settings`, and the
/// test above this one pins that a non-administrator cannot make that change. A deployment where an
/// editor sees no button at all is almost always the second rule rather than the first - the
/// setting is off, and off is the default, so a Schema that was migrated into the CMS has no
/// setting until an administrator turns it on once.
#[tokio::test]
async fn an_editor_may_issue_a_preview_link() {
    let app = test_app().await;
    let item = create_sample_item(&app, "blog").await;
    allow_collection_preview(&app, "blog").await;

    let (status, created) = create_account(
        &app,
        json!({
            "username": "preview-editor@example.com",
            "password": "editor-password",
            "is_admin": false,
            "permission": Permission::editor(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, body) = login(&app, "preview-editor@example.com", "editor-password").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let editor = body["token"].as_str().unwrap().to_string();

    let (status, link) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item}/preview-link"),
        Some(&editor),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an editor may issue a link: {link}");
    let path = link["path"].as_str().expect("a path").to_string();
    assert!(path.starts_with(&format!(
        "/api/preview/collections/blog/items/{item}?token="
    )));

    // And it is the same link an administrator's is: the signature is the credential.
    let (status, body) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], item);
}
