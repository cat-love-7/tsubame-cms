//! Collections and their items: the CRUD round trip, deletion, duplication.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

#[tokio::test]
async fn collection_item_crud_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["name"], "title");
    // width/height must survive the round trip; their absence used to make saves fail.
    assert_eq!(body[0]["width"], 12);
    assert_eq!(body[1]["field_type"]["TextEnum"][1], "blog");

    // Create.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": ["news"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(1));

    // List (the id/response tuple shape is preserved).
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0][0], 1);

    // Read one.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A working copy may be missing a required field: it is a draft, and the site is not served
    // from it (see `a_required_field_is_asked_for_at_publication`). What it may *not* do is go
    // live.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "tags": ["news"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let incomplete = body.as_u64().expect("the new item's id");

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{incomplete}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_required");
    assert_eq!(body["field"], "title");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/models/collections/blog/items/{incomplete}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Update + delete.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/blog/items/1",
        Some(&token),
        Some(json!({ "title": "Updated", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Deleting the collection itself used to fail with 500.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn deleting_a_collection_without_items_succeeds() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/empty/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/empty",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Duplicating an item: everything comes across except what has to be new, and the copy is a draft
/// whatever the original was.
#[tokio::test]
async fn an_item_can_be_duplicated_into_a_draft_with_free_unique_fields() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Both unique fields are *required*: a copy that empties them is exactly the case that used to
    // be impossible, because a create was validated as if it were going live (see
    // `FieldValueMap::validate_draft`). The slug is the other kind of unique field, and the one a
    // "generate from the title" button fills in.
    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1, "unique": true },
        { "name": "slug", "field_type": { "Slug": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "body", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, created) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "original", "slug": "original", "body": "the words" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let original = created.as_u64().expect("the item id");
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{original}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, copy) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{original}/duplicate"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let copy = copy.as_u64().expect("the copy's id");
    assert_ne!(copy, original);

    let (_, values) = send(
        &app.router,
        Method::GET,
        &format!("/models/collections/blog/items/{copy}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(values["body"], "the words", "everything else comes across");
    assert_eq!(
        values["title"], "",
        "the unique fields are emptied, not copied - a required one included"
    );
    assert_eq!(values["slug"], "");
    // Which leaves the value free: the copy can be given a new one.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/models/collections/blog/items/{copy}"),
        Some(&token),
        Some(json!({ "title": "a second one", "slug": "a-second-one", "body": "the words" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A copy of a published item is a draft, and nothing was published about it.
    let (_, metadata) = send(
        &app.router,
        Method::GET,
        &format!("/models/collections/blog/items/{copy}/metadata"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(metadata["status"], "draft", "{metadata}");
    let (_, source) = send(
        &app.router,
        Method::GET,
        &format!("/models/collections/blog/items/{original}/metadata"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(source["status"], "published", "the original is untouched");

    // The copy is not on the site.
    let (status, _) = send_raw(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{copy}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Duplicating something that is not there says so.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/999/duplicate",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
