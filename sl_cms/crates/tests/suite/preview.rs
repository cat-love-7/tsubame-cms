//! Preview links: one working copy, without a token, until they expire.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// A preview link shows one working copy to someone with no account, until it expires.
#[tokio::test]
async fn a_preview_link_shows_one_working_copy_without_a_token() {
    let app = test_app().await;
    // The first call also creates the collection's schema, so the second item goes straight
    // in.
    let first = create_sample_item(&app, "blog").await;
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
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
        &format!("/models/collections/blog/items/{first}"),
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
                &format!("/models/collections/blog/items/{id}/preview-link"),
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
    assert!(path.starts_with(&format!("/preview/collections/blog/items/{first}?token=")));
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
        &format!("/content/collections/blog/items/{first}"),
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
        &format!("/preview/collections/blog/items/{first}?token=1.deadbeef"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/preview/collections/blog/items/{first}"),
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
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Unpublished home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/preview-link",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = body["path"].as_str().unwrap().to_string();
    assert!(path.starts_with("/preview/single_pages/home?token="));

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
            "/preview/single_pages/home",
            "/preview/collections/home/items/1",
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
        "/models/single_pages/missing/preview-link",
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
        "/preview/collections/blog/items/{item_id}?token={}.{signature}",
        (chrono::Utc::now() + chrono::Duration::days(30)).timestamp()
    );
    let (status, _) = send(&app.router, Method::GET, &forged, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
