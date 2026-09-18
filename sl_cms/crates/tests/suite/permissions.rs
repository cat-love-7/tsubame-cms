//! What each role and each grant allows, and when a change to them takes effect.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

#[tokio::test]
async fn viewer_can_read_but_not_write_and_cannot_manage_users() {
    let app = test_app().await;
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );

    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["is_admin"], false);
    let viewer_token = body["token"].as_str().unwrap().to_string();

    // Reads are allowed.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Writes are not.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&viewer_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Account management is administrator-only even for editors.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Per-resource permissions: an account can be trusted with one collection and not the rest,
/// and the account-wide role is what it has everywhere else.
#[tokio::test]
async fn a_collection_grant_applies_to_that_collection_only() {
    let app = test_app().await;
    let admin = app.admin_token.clone();

    for collection in ["blog", "news", "docs"] {
        create_sample_item(&app, collection).await;
    }
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A viewer everywhere to start with...
    let (status, created) = create_account(
        &app,
        json!({
            "username": "scoped@example.com",
            "password": "scoped-password",
            "is_admin": false,
            "permission": Permission::viewer(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let scoped_id = created["id"].as_str().unwrap().to_string();

    // ...then trusted with `blog` (edit), `news` (edit and release) and nothing of `home`.
    let deny = json!({ "can_view": false, "can_edit": false, "can_publish": false });
    let (status, updated) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{scoped_id}"),
        Some(&admin),
        Some(json!({
            "collection_permissions": {
                "blog": Permission::editor(),
                "news": Permission { can_view: true, can_edit: true, can_publish: true },
            },
            "single_page_permissions": { "home": deny },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    // The overrides come back, which is what the account screen reads.
    assert_eq!(updated["collection_permissions"]["blog"]["can_edit"], true);
    assert_eq!(
        updated["collection_permissions"]["blog"]["can_publish"],
        false
    );
    assert_eq!(
        updated["single_page_permissions"]["home"]["can_view"],
        false
    );

    let (_, body) = login(&app, "scoped@example.com", "scoped-password").await;
    let scoped = body["token"].as_str().unwrap().to_string();
    let edit = json!({ "title": "edited by the scoped account", "tags": [] });

    // `blog` is editable but not releasable.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/blog/items/1",
        Some(&scoped),
        Some(edit.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "grant したコレクションは編集できる");
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/1/publish",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "公開の grant は無い");

    // `news` carries the publish grant, so both work there.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/news/items/1",
        Some(&scoped),
        Some(edit.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/news/items/1/publish",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // `docs` has no override: the account-wide viewer role applies, so it reads and cannot
    // write.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections/docs/items/1",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "共通ロールはどこでも効く");
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/docs/items/1",
        Some(&scoped),
        Some(edit),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "grant の無いコレクションは読み取りだけ"
    );

    // `home` was denied, so even reading it is refused...
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "拒否したページは読めない");
    // ...and the list does not offer it, so the sidebar cannot lead there.
    let (status, pages) = send(
        &app.router,
        Method::GET,
        "/models/single_pages",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pages, json!([]), "deny したページは一覧に出ない");

    let (status, collections) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = collections
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect();
    assert!(names.contains(&"blog") && names.contains(&"news") && names.contains(&"docs"));

    // An administrator keeps seeing and doing everything.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// A grant for something that does not exist is a typo, not a permission.
#[tokio::test]
async fn a_grant_for_an_unknown_resource_is_refused() {
    let app = test_app().await;
    let admin = app.admin_token.clone();
    create_sample_item(&app, "blog").await;

    let (status, created) = create_account(
        &app,
        json!({
            "username": "typo@example.com",
            "password": "typo-password",
            "is_admin": false,
            "permission": Permission::viewer(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // A typo in the collection name is refused...
    let (status, body) = send_raw(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "collection_permissions": { "blgo": Permission::editor() } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("blgo"),
        "the answer names the typo: {}",
        String::from_utf8_lossy(&body)
    );

    // ...as is one in a page name, while the real names are accepted.
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "single_page_permissions": { "nope": Permission::viewer() } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "collection_permissions": { "blog": Permission::editor() } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Roles and per-resource grants belong to the CMS, not to whoever authenticates the user: an
/// administrator changes them on the account record, and the change is in force on the very
/// next request rather than the next time a token is issued.
#[tokio::test]
async fn a_role_change_takes_effect_on_the_next_request() {
    let app = test_app().await;
    let admin = app.admin_token.clone();

    let (status, created) = create_account(
        &app,
        json!({ "username": "promoted@example.com", "password": "promoted-password" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("an id").to_string();
    let (_, body) = login(&app, "promoted@example.com", "promoted-password").await;
    let token = body["token"].as_str().unwrap().to_string();

    // The collections have to exist before there is anything to edit in them.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/news/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A viewer may read, but writing content needs `can_edit`.
    let write = |token: String, collection: &'static str| {
        let router = app.router.clone();
        async move {
            send(
                &router,
                Method::POST,
                &format!("/models/collections/{collection}/item"),
                Some(&token),
                Some(json!({ "title": "Hello", "tags": [] })),
            )
            .await
        }
    };
    assert_eq!(write(token.clone(), "blog").await.0, StatusCode::FORBIDDEN);

    // The administrator grants edit rights. The caller's token does not change.
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "permission": Permission::editor() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        write(token.clone(), "blog").await.0,
        StatusCode::OK,
        "the stored permission is what decides, not what the token was issued with"
    );

    // A per-collection override narrows it again, for one collection only.
    let deny = json!({ "can_view": false, "can_edit": false, "can_publish": false });
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "collection_permissions": { "blog": deny } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(write(token.clone(), "blog").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        write(token, "news").await.0,
        StatusCode::OK,
        "another collection is unaffected"
    );
}

/// The permission matrix, checked through the real routes rather than the helpers.
///
/// Reading needs `can_view`; editing content needs `can_edit`; releasing it needs
/// `can_publish`; and changing the shape of the site needs an administrator, because
/// editing content is not the same as deciding what content can exist.
#[tokio::test]
async fn each_role_can_do_exactly_what_it_is_granted() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    let roles = [
        ("viewer@example.com", Permission::viewer()),
        ("editor@example.com", Permission::editor()),
        (
            "publisher@example.com",
            Permission {
                can_view: true,
                can_edit: true,
                can_publish: true,
            },
        ),
        (
            "nobody@example.com",
            Permission {
                can_view: false,
                can_edit: false,
                can_publish: false,
            },
        ),
    ];
    for (email, permission) in roles {
        let (status, body) = create_account(
            &app,
            json!({
                "username": email,
                "password": "role-password",
                "is_admin": false,
                "permission": permission,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{email}: {body}");
    }

    let mut tokens = HashMap::new();
    for (email, _) in roles {
        let (status, body) = login(&app, email, "role-password").await;
        assert_eq!(status, StatusCode::OK, "{email} login: {body}");
        tokens.insert(email, body["token"].as_str().unwrap().to_string());
    }

    // Reading: everyone holding `can_view`.
    for email in [
        "viewer@example.com",
        "editor@example.com",
        "publisher@example.com",
    ] {
        let (status, _) = send(
            &app.router,
            Method::GET,
            "/models/collections",
            Some(&tokens[email]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{email} should be able to read");
    }
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&tokens["nobody@example.com"]),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an account without can_view cannot read"
    );

    // Editing content: editors and publishers, not viewers.
    for (email, expected) in [
        ("viewer@example.com", StatusCode::FORBIDDEN),
        ("editor@example.com", StatusCode::OK),
        ("publisher@example.com", StatusCode::OK),
    ] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/models/collections/blog/item",
            Some(&tokens[email]),
            Some(json!({ "title": "draft", "tags": [] })),
        )
        .await;
        assert_eq!(status, expected, "{email} creating an item");
    }

    // Releasing it: only a publisher.
    for (email, expected) in [
        ("viewer@example.com", StatusCode::FORBIDDEN),
        ("editor@example.com", StatusCode::FORBIDDEN),
        ("publisher@example.com", StatusCode::OK),
    ] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/models/collections/blog/items/{item_id}/publish"),
            Some(&tokens[email]),
            None,
        )
        .await;
        assert_eq!(status, expected, "{email} publishing");
    }

    // Changing the shape of the site: administrators only.
    for email in ["editor@example.com", "publisher@example.com"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/models/collections/another/schema",
            Some(&tokens[email]),
            Some(sample_schema()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{email} changing a schema");
    }

    // Deleting content takes it off the site, so it needs a publisher.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/models/collections/blog/items/{item_id}"),
        Some(&tokens["editor@example.com"]),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an editor cannot delete content"
    );

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/models/collections/blog/items/{item_id}"),
        Some(&tokens["publisher@example.com"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a publisher may delete content");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog",
        Some(&tokens["publisher@example.com"]),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "deleting a collection is structural"
    );

    // The administrator can do all of it.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&tokens["publisher@example.com"]),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "account management is administrator-only"
    );
}
