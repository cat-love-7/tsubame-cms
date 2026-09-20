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

/// Who references one author, as the management API answers it.
async fn references(app: &TestApp, token: &str, id: u64) -> Value {
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/authors/items/{id}/references"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body
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

    // A composite definition is stored on its own and reused, so its save is where its own
    // relation's target is checked - and it is checked again when a schema embeds it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/composite_fields/cta",
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

    // The same definition, naming what the site has, is stored.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/composite_fields/cta",
        Value::Array(vec![relation_field(
            "author",
            "collection",
            "authors",
            false,
        )]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
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

/// The reference index, which is what answers "who points at this?" without reading every item.
///
/// It is written with the content itself, so what these check is not only that the answer is right
/// but that it keeps up: a save adds and drops entries, publishing counts the working copy too,
/// and a delete takes them with it.
#[tokio::test]
async fn the_reference_index_follows_what_content_holds() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    for title in ["Ada", "Grace"] {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            "/api/models/collections/authors/item",
            json!({ "title": title }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

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

    // Nothing references the authors yet.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/items/1/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]), "an unused item has no referrers");

    // A save writes the entries for what the item holds.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({
            "title": "Hello",
            "author": [{ "target": "authors", "item": 1 }],
            "also": [{ "target": "authors", "item": 2 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        references(&app, &token, 1).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }])
    );
    assert_eq!(
        references(&app, &token, 2).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }])
    );

    // A save writes the *working copy*, and the published record the create left behind still
    // holds what it held - so the index keeps both: the editor is holding a reference to author 1
    // even though the draft just dropped it, and the published copy still names it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        json!({
            "title": "Hello",
            "author": [],
            "also": [{ "target": "authors", "item": 2 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        references(&app, &token, 1).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
        "the published copy still references it"
    );

    // Publishing promotes the working copy, which is what drops it: the promotion replaces the
    // published record and deletes the working one, so what is left is what was saved.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/items/1/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(references(&app, &token, 1).await, json!([]));
    assert_eq!(
        references(&app, &token, 2).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
        "the reference it kept is still there"
    );

    // And the working copy counts on its own: a save that puts a reference back is enough, even
    // though nothing has been published since.
    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        json!({
            "title": "Hello",
            "author": [{ "target": "authors", "item": 1 }],
            "also": [],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        references(&app, &token, 1).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
        "the working copy counts"
    );
    assert_eq!(
        references(&app, &token, 2).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
        "the published copy still holds the reference the draft dropped"
    );

    // Unpublishing takes the item off the site but keeps both copies, so the index is unchanged:
    // it is still a reason to keep both authors.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/items/1/unpublish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for id in [1, 2] {
        assert_eq!(
            references(&app, &token, id).await,
            json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
            "an unpublish leaves the content where it was"
        );
    }

    // Deleting the content takes its entries with it, both directions and both copies.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/posts/items/1",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for id in [1, 2] {
        assert_eq!(references(&app, &token, id).await, json!([]));
    }

    // An item that is not there is a 404, not an empty list: the caller asked about something that
    // does not exist.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/items/999/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A relation may sit inside a composite definition: the definition is stored once and reused by
/// whatever embeds it, and the reference it holds belongs to the item that holds the value - so the
/// index, the delete refusal and `?detach` all have to reach inside it.
#[tokio::test]
async fn a_relation_inside_a_composite_is_indexed_and_detached_like_any_other() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let posts = "/api/models/collections/posts";

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/item",
        json!({ "title": "Ada" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The definition holds the relation, and the collection embeds it twice: once as a field and
    // once as an array whose elements are the same definition.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/composite_fields/cta",
        json!([relation_field("author", "collection", "authors", false)]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    save_schema(
        &app,
        &token,
        &format!("{posts}/schema"),
        json!([
            text_field("title", true),
            { "name": "cta", "field_type": { "CompositeField": { "id": "cta" } },
              "required": false, "width": 12, "height": 1 },
            { "name": "blocks", "field_type": { "Array": [{ "CompositeField": { "id": "cta" } }] },
              "required": false, "width": 12, "height": 1 },
        ]),
    )
    .await;

    // A value is written the way the schema reads it: the bare object, with the reference inside.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/item"),
        json!({
            "title": "Hello",
            "cta": { "author": [{ "target": "authors", "item": 1 }] },
            "blocks": [{ "author": [] }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        references(&app, &token, 1).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
        "a reference inside a composite is indexed"
    );

    // Moving it into an element of the array keeps the entry, and takes it away when it goes.
    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        &format!("{posts}/items/1"),
        json!({
            "title": "Hello",
            "cta": { "author": [] },
            "blocks": [{ "author": [{ "target": "authors", "item": 1 }] }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        references(&app, &token, 1).await,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }]),
        "a reference inside an array element is indexed"
    );

    // The author cannot be deleted while anything points at it, however deep the field is.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors/items/1",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("still_referenced"), "{body}");
    assert!(body.contains("posts item 1"), "{body}");

    // Detaching reaches into the composite, and leaves everything else as it was.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors/items/1?detach=true",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("{posts}/items/1"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Hello");
    assert_eq!(body["cta"]["values"]["author"], json!([]));
    assert_eq!(body["blocks"][0]["values"]["author"], json!([]));
}

/// The same index, with a single page as the thing being pointed at.
#[tokio::test]
async fn a_page_can_be_referenced_too() {
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

    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "landing": [{ "target": "home" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!([{ "kind": "collection_item", "name": "posts", "item": 1 }])
    );

    // A page that is not there has no referrers either, and says so.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/nope/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A page's own values are indexed as well, and its working copy counts like an item's.
#[tokio::test]
async fn a_page_that_references_an_item_is_indexed() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/item",
        json!({ "title": "Ada" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    save_schema(
        &app,
        &token,
        "/api/models/single_pages/about/schema",
        Value::Array(vec![relation_field(
            "author",
            "collection",
            "authors",
            false,
        )]),
    )
    .await;

    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        "/api/models/single_pages/about/item",
        json!({ "author": [{ "target": "authors", "item": 1 }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/items/1/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([{ "kind": "single_page", "name": "about" }]));

    // Deleting the page takes its reference with it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/single_pages/about",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/items/1/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

/// Deleting something another piece of content points at is refused, unless the caller says the
/// references may go with it.
///
/// Refused rather than allowed with a dangling reference, because the site would serve content
/// naming an item that is not there; and `?detach=true` rather than a second endpoint, because
/// removing the references is the same act as deleting the thing that made them meaningful.
#[tokio::test]
async fn content_something_points_at_is_not_deleted_until_the_references_go() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/item",
        json!({ "title": "Ada" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

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
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({
            "title": "About Ada",
            "author": [{ "target": "authors", "item": 1 }],
            "also": [{ "target": "authors", "item": 1 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The author has a post pointing at it, so the delete is refused - with the code a client
    // words, and a message that names what points at it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors/items/1",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("still_referenced"), "{body}");
    assert!(body.contains("posts item 1"), "{body}");

    // Refused means refused: the author is still there, and so is the post.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The referrer's own record says it changed, which is what a build watches.
    let (status, before) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let was = timestamp(&before["updated_at"]);

    // `?detach=true` removes every reference to it first - both fields, in both copies - and then
    // deletes it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors/items/1?detach=true",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the author was deleted");

    // What pointed at it no longer does, in the values the delivery API would serve and in the
    // working copy an editor would open.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["author"], json!([]));
    assert_eq!(body["also"], json!([]));
    assert_eq!(
        body["title"], "About Ada",
        "the rest of the content is untouched"
    );

    // Detaching changed that content, so its `updated_at` moved - the same thing a save records,
    // without touching whether it is published.
    let (status, after) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        timestamp(&after["updated_at"]) > was,
        "detaching is a change: {was} -> {}",
        after["updated_at"]
    );

    // The post can now be published, which is the point: nothing it holds names content that is
    // gone.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/items/1/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// The same refusal for a page others point at, and the same way past it.
#[tokio::test]
async fn a_referenced_page_is_not_deleted_until_the_references_go() {
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
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "landing": [{ "target": "home" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/single_pages/home",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("still_referenced"), "{body}");
    assert!(body.contains("posts item 1"), "{body}");

    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/single_pages/home?detach=true",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["landing"], json!([]));

    // Nothing references it any more, so a second delete attempt would not be refused for that
    // reason - it is simply gone.
    let (status, _) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/single_pages/home",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Deleting content that is not referenced is not asked about, and detaching a page's own
/// references never touches the content it pointed at.
#[tokio::test]
async fn a_delete_nothing_points_at_goes_straight_through() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    for title in ["Ada", "Grace"] {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            "/api/models/collections/authors/item",
            json!({ "title": title }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // Author 2 points at author 1. Deleting author 2 is about *being* referenced, not about
    // referencing something, so it goes through and takes its entry with it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/item",
        json!({ "title": "Ada's editor" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Author 1 is not referenced by anything, so it deletes without the flag.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors/items/1",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
