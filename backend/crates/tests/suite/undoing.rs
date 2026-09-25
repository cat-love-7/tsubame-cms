//! Undoing unpublished work: seeing what the site serves, and throwing the working copy away.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// An item that was published and then edited: the editor can see what is live, and can put the
/// item back to it without publishing the change.
#[tokio::test]
async fn what_is_live_can_be_read_and_the_working_copy_discarded() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let id = create_sample_item(&app, "blog").await;

    // Publish it, so there is something live, and then change it without publishing.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/blog/items/{id}"),
        Some(&token),
        Some(json!({ "title": "Changed", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The editor's own read shows the working copy...
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Changed", "編集者が見るのは作業コピー");

    // ...and the published copy is what the site serves, which is the other half.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{id}/published"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Hello", "公開中はこちら");

    // Throwing the working copy away is the undo: nothing is published, nothing is unpublished,
    // and the editor is looking at what the site serves again.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/api/models/collections/blog/items/{id}/draft"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Hello", "公開中の内容に戻る");
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{id}/metadata"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["has_draft"], false, "保留中の変更は無くなった");
    // And the site was never touched: the delivery API still serves the same content.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/content/collections/blog/items/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");

    // Discarding nothing is not an error: the caller asked for a state, and that state is what it
    // gets (as for trashing an image that is already in the trash).
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/api/models/collections/blog/items/{id}/draft"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // An item that is not published has nothing live to show, whatever the item store holds.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Never published", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let new_id = body.as_u64().expect("the new item id");
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{new_id}/published"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An item that is not there at all, for both routes.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/items/9999/published",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/collections/blog/items/9999/draft",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Reading what is live is a read; discarding is an edit, not a publish - it does not change
    // what the site serves. A viewer may do the first and not the second.
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    let viewer = body["token"].as_str().unwrap().to_string();
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{id}/published"),
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/api/models/collections/blog/items/{id}/draft"),
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// The same for a single page, which has one item and therefore no id in the path.
#[tokio::test]
async fn a_pages_working_copy_can_be_seen_against_what_is_live_and_discarded() {
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
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Live", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Nothing pending: what is live is what the editor sees.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/published",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Live");

    // Change it, and the two reads part company.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Changed", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, draft) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/item",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(draft["title"], "Changed");
    let (_, live) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/published",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(live["title"], "Live");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/single_pages/home/draft",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/item",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Live", "公開中の内容に戻る");
    let (_, metadata) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/item/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(metadata["has_draft"], false);

    // A page that does not exist, for both routes.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/missing/published",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/single_pages/missing/draft",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
