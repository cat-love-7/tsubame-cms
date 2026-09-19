//! When content changed, and the publication date the delivery API reports.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// The content clock and the publication date answer different questions.
///
/// `updated_at` is when the content last changed - a save, or a publish that released a working
/// copy - and `published_at` is when the item was first published, which later releases do not
/// move.
#[tokio::test]
async fn saving_records_when_content_changed_and_releasing_does_too() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let metadata_url = format!("/api/models/collections/blog/items/{item_id}/metadata");

    let (status, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let created_at = timestamp(&body["created_at"]);
    let updated_at = timestamp(&body["updated_at"]);
    assert_eq!(
        created_at, updated_at,
        "a new item is created and updated at the same moment"
    );

    // The first publish releases the working copy a new item is written to, so the content the
    // site serves changes with it.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item_id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert!(
        timestamp(&body["updated_at"]) > updated_at,
        "releasing the first version changes the content"
    );
    let first_published = timestamp(&body["published_at"]);
    let first_release = timestamp(&body["last_published_at"]);
    assert_eq!(
        first_release, first_published,
        "the first release is also the first publication"
    );

    // An edit moves `updated_at`, keeps `created_at`, and does not unpublish.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/blog/items/{item_id}"),
        Some(&token),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        timestamp(&body["updated_at"]) > updated_at,
        "an edit has to move updated_at"
    );
    assert_eq!(
        timestamp(&body["created_at"]),
        created_at,
        "created_at must not move"
    );
    assert_eq!(body["status"], "published");
    assert_eq!(
        timestamp(&body["published_at"]),
        first_published,
        "an edit does not move the publication date"
    );
    assert_eq!(
        timestamp(&body["last_published_at"]),
        first_release,
        "and it does not move the release time either"
    );

    // The edit is waiting in the working copy: the live site keeps serving what was
    // published until the item is published again.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");
    assert!(body["values"].get("updated_at").is_none());

    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item_id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Edited");
    assert_eq!(
        timestamp(&body["published_at"]),
        first_published,
        "the publication date is the first one"
    );

    // Releasing the edit moved the content clock and the release time, not the publication date.
    let (_, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    let released_at = timestamp(&body["updated_at"]);
    assert!(
        released_at > updated_at,
        "releasing an edit changes the content"
    );
    assert!(
        timestamp(&body["last_published_at"]) > first_release,
        "and it is a release"
    );
    assert_eq!(timestamp(&body["published_at"]), first_published);

    // ...while publishing again with nothing waiting moves neither.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item_id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(
        timestamp(&body["updated_at"]),
        released_at,
        "publishing with nothing to release is not a change"
    );
    assert_eq!(timestamp(&body["published_at"]), first_published);
}

/// The delivery API carries both dates: the publication date a site shows, and the release time
/// an incremental build compares.
#[tokio::test]
async fn the_delivery_api_reports_the_publication_date_and_the_release_time() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let item_url = format!("/api/models/collections/blog/items/{item_id}");
    let content_url = format!("/api/content/collections/blog/items/{item_id}");

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("{item_url}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first_date = timestamp(&body["published_at"]);

    let (_, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(timestamp(&body["published_at"]), first_date);
    let first_release = timestamp(&body["last_published_at"]);
    assert_eq!(first_release, first_date);

    // A release moves the last-published time, and the publication date is still the first one.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Second", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("{item_url}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Second");
    assert_eq!(
        timestamp(&body["published_at"]),
        first_date,
        "公開日は初回のまま"
    );
    assert!(
        timestamp(&body["last_published_at"]) > first_release,
        "公開物が変わった時刻は進む"
    );
}

#[tokio::test]
async fn saving_a_single_page_records_when_it_changed() {
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
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let metadata_url = "/api/models/single_pages/home/item/metadata";
    let (status, body) = send(&app.router, Method::GET, metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let updated_at = timestamp(&body["updated_at"]);
    assert_eq!(timestamp(&body["created_at"]), updated_at);

    // A published page that is saved again stays published.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home again", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(timestamp(&body["updated_at"]) > updated_at);
    assert_eq!(body["status"], "published", "saving must not unpublish");

    // Saving a published page leaves the live copy alone until it is published again.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Home");

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Home again");
}
