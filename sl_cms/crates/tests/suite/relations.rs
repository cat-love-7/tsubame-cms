//! Relations: what a schema may point at, what a reference looks like on the wire, and how a
//! value that does not match its field is refused.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// Send a schema or a value, reporting the status and the body as text so a test can say what it
/// expected to read.
async fn send_json(
    app: &TestApp,
    token: &str,
    method: Method,
    path: &str,
    values: Value,
) -> (StatusCode, String) {
    let (status, body) = send_raw(&app.router, method, path, Some(token), Some(values)).await;
    (status, String::from_utf8_lossy(&body).to_string())
}

/// Save a schema, failing loudly with the body when it is refused.
async fn save_schema(app: &TestApp, token: &str, path: &str, fields: Value) {
    let (status, body) = send_json(app, token, Method::POST, path, fields).await;
    assert_eq!(status, StatusCode::OK, "saving {path}: {body}");
}

/// A text field as the Angular client sends it.
fn text_field(name: &str, required: bool) -> Value {
    json!({ "name": name, "field_type": { "Text": {} }, "required": required, "width": 12, "height": 1 })
}

/// A relation field as the Angular client sends it.
fn relation_field(name: &str, kind: &str, target: &str, has_many: bool) -> Value {
    json!({
        "name": name,
        "field_type": { "Relation": {
            "target": { "kind": kind, "name": target },
            "has_many": has_many,
            "inverse_name": "articles",
        }},
        "required": false, "width": 12, "height": 1,
    })
}

/// A relation to a collection's items round-trips as a list of `{target, item}`, and the list is a
/// set: the order it was sent in does not survive, and the same reference twice is one reference.
#[tokio::test]
async fn a_relation_to_a_collection_round_trips_as_a_set_of_references() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/authors/item",
        Some(&token),
        Some(json!({ "title": "Ada" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    save_schema(
        &app,
        &token,
        "/api/models/collections/posts/schema",
        Value::Array(vec![
            text_field("title", true),
            relation_field("author", "collection", "authors", false),
            relation_field("also", "collection", "authors", true),
        ]),
    )
    .await;

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({
            "title": "Hello",
            "author": [{ "target": "authors", "item": 1 }],
            "also": [
                { "target": "authors", "item": 3 },
                { "target": "authors", "item": 1 },
                { "target": "authors", "item": 3 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Read back sorted and deduplicated, which is what makes two items holding the same
    // references compare equal.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["author"], json!([{ "target": "authors", "item": 1 }]));
    assert_eq!(
        body["also"],
        json!([
            { "target": "authors", "item": 1 },
            { "target": "authors", "item": 3 },
        ])
    );

    // The response is a write shape, so loading an item and saving it back unchanged works.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // The schema kept what it was told, including the name the other side is known by.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body[1]["field_type"]["Relation"]["target"]["name"],
        "authors"
    );
    assert_eq!(body[1]["field_type"]["Relation"]["has_many"], false);
    assert_eq!(
        body[1]["field_type"]["Relation"]["inverse_name"],
        "articles"
    );
    // A reference to several is the same field with the flag set.
    assert_eq!(body[2]["field_type"]["Relation"]["has_many"], true);
}

/// A single page is one item whose identity is its name, so a reference to it is the name and
/// nothing else - there is no id to store.
#[tokio::test]
async fn a_relation_to_a_single_page_is_the_page_name() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/single_pages/home/schema",
        json!([]),
    )
    .await;
    save_schema(
        &app,
        &token,
        "/api/models/collections/posts/schema",
        Value::Array(vec![
            text_field("title", true),
            relation_field("landing", "single_page", "home", false),
        ]),
    )
    .await;

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({
            "title": "Hello",
            "landing": [{ "target": "home" }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["landing"], json!([{ "target": "home" }]));

    // An id for a page is a value the schema has nowhere to keep.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        Some(&token),
        Some(json!({
            "title": "Hello",
            "landing": [{ "target": "home", "item": 1 }],
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        String::from_utf8_lossy(&body).contains("has no item id"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}

/// A relation is a promise that the other end is there, so a schema naming something the site
/// does not have is refused - not stored as a field nobody could ever fill in.
#[tokio::test]
async fn a_relation_may_only_point_at_what_the_site_has() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;

    // Nothing of that name.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/schema",
        Value::Array(vec![relation_field(
            "author",
            "collection",
            "writers",
            false,
        )]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("'writers' is not a collection"), "{body}");

    // A name that exists as the other kind is a different target, so it is missing too.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/schema",
        Value::Array(vec![relation_field(
            "home",
            "single_page",
            "authors",
            false,
        )]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("'authors' is not a single page"), "{body}");

    // A page is one item, so a relation to one cannot ask for several.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/schema",
        Value::Array(vec![relation_field(
            "homes",
            "single_page",
            "authors",
            true,
        )]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("can only be a single reference"), "{body}");

    // ...and a relation cannot be an array item: how many it holds is its own `has_many`.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/schema",
        json!([{
            "name": "related",
            "field_type": { "Array": [{ "Relation": {
                "target": { "kind": "collection", "name": "authors" },
                "has_many": true,
            }}]},
            "required": false, "width": 12, "height": 1,
        }]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("cannot be an array item"), "{body}");
}

/// A reference has to match the field that holds it, and how many it holds has to match too.
#[tokio::test]
async fn a_relation_value_has_to_match_the_field_holding_it() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    save_schema(
        &app,
        &token,
        "/api/models/collections/posts/schema",
        Value::Array(vec![
            text_field("title", true),
            relation_field("author", "collection", "authors", false),
            relation_field("also", "collection", "authors", true),
        ]),
    )
    .await;

    // The field points at `authors`, so a reference naming another collection is refused.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({
            "title": "Hello",
            "author": [{ "target": "writers", "item": 1 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("points at 'authors'"), "{body}");

    // A collection reference needs the id of the item it points at.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({
            "title": "Hello",
            "author": [{ "target": "authors" }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("item id"), "{body}");

    // A single reference holds one, not two.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({
            "title": "Hello",
            "author": [
                { "target": "authors", "item": 1 },
                { "target": "authors", "item": 2 },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("holds one reference"), "{body}");

    // A relation is a list of references, not a scalar.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "author": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("expected an array"), "{body}");

    // A required relation is one that says something. A working copy may still be empty - it is
    // a draft - but it cannot go live: the same rule that asks for a required text field asks
    // for a required reference, and names it.
    save_schema(
        &app,
        &token,
        "/api/models/collections/strict/schema",
        json!([{
            "name": "author",
            "field_type": { "Relation": {
                "target": { "kind": "collection", "name": "authors" },
                "has_many": true,
            }},
            "required": true, "width": 12, "height": 1,
        }]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/strict/item",
        json!({ "author": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let empty = body.parse::<u64>().expect("the new item's id");

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/strict/items/{empty}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("field_required"), "{body}");
    assert!(body.contains("author"), "{body}");
}
