//! Single pages and the states their list reports.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// The page list is one request: every page's state, so a screen can show a status without asking
/// per page. Readable pages only, judged as elsewhere.
#[tokio::test]
async fn the_single_page_list_reports_every_page_state() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    for name in ["about", "contact"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/models/single_pages/{name}/schema"),
            Some(&token),
            Some(sample_schema()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(
            &app.router,
            Method::PUT,
            &format!("/models/single_pages/{name}/item"),
            Some(&token),
            Some(json!({ "title": name, "tags": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // One is published, the other is left as a draft.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/about/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/items/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["about"]["status"], "published");
    assert_eq!(body["about"]["has_draft"], false);
    assert_eq!(body["contact"]["status"], "draft");
    assert!(body["contact"]["updated_at"].is_string(), "{body}");

    // Saving after publishing leaves a working copy behind, which the list has to show.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/about/item",
        Some(&token),
        Some(json!({ "title": "About us", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/items/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(body["about"]["status"], "published");
    assert_eq!(
        body["about"]["has_draft"], true,
        "the working copy is not live yet"
    );

    // A read-only account sees the pages it may read, and nothing else.
    let (status, _) = create_account(
        &app,
        json!({
            "username": "page-reader",
            "password": "reader-password",
            "is_admin": false,
            "permission": { "can_view": true, "can_edit": false, "can_publish": false }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, login_body) = login(&app, "page-reader", "reader-password").await;
    let reader = login_body["token"].as_str().unwrap().to_string();
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/items/metadata",
        Some(&reader),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["about"]["status"], "published", "{body}");
}

#[tokio::test]
async fn single_page_crud_round_trip() {
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
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // FieldValueResponse is untagged, so the text value comes back bare.
    assert_eq!(body["title"], "Home");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/single_pages/home",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
