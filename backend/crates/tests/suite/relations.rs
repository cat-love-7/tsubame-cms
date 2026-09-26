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

/// A relation field as the Angular client sends it: `Relation` holds exactly one reference.
fn relation_field(name: &str, kind: &str, target: &str) -> Value {
    json!({
        "name": name,
        "field_type": { "Relation": { "target": { "kind": kind, "name": target } } },
        "required": false, "width": 12, "height": 1,
    })
}

/// The same, naming the other side (`inverse_name`), which is how `?populate=` addresses it.
fn relation_field_named(name: &str, kind: &str, target: &str, inverse_name: &str) -> Value {
    json!({
        "name": name,
        "field_type": { "Relation": {
            "target": { "kind": kind, "name": target },
            "inverse_name": inverse_name,
        }},
        "required": false, "width": 12, "height": 1,
    })
}

/// The same relation, required: `null` saves as a draft and publishing asks for one.
fn required_relation_field(name: &str, kind: &str, target: &str) -> Value {
    let mut field = relation_field(name, kind, target);
    field["required"] = json!(true);
    field
}

/// Several references: an array whose item types are relations, each target named once.
fn relation_array_field(name: &str, targets: &[(&str, &str)]) -> Value {
    let item_types: Vec<Value> = targets
        .iter()
        .map(|(kind, target)| json!({ "Relation": { "target": { "kind": kind, "name": target } } }))
        .collect();
    json!({
        "name": name,
        "field_type": { "Array": item_types },
        "required": false, "width": 12, "height": 1,
    })
}

/// The same array, required: `[]` saves as a draft and publishing asks for one.
fn required_relation_array_field(name: &str, targets: &[(&str, &str)]) -> Value {
    let mut field = relation_array_field(name, targets);
    field["required"] = json!(true);
    field
}

/// Several references to one target, where the other side calls the relation by name: the inverse
/// direction has to reach into an array's item type, not only a bare `Relation`.
fn relation_array_field_named(name: &str, kind: &str, target: &str, inverse_name: &str) -> Value {
    json!({
        "name": name,
        "field_type": { "Array": [{ "Relation": {
            "target": { "kind": kind, "name": target },
            "inverse_name": inverse_name,
        }}]},
        "required": false, "width": 12, "height": 1,
    })
}

/// A relation to a collection round-trips as one `{target, item}` reference, and several are an
/// `Array` of them: the array keeps the order it was written in, and the same reference twice is
/// one reference at the place it first appeared.
#[tokio::test]
async fn a_relation_to_a_collection_round_trips_as_an_ordered_list_of_references() {
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
        json!([
            text_field("title", true),
            relation_field_named("author", "collection", "authors", "articles"),
            relation_array_field("also", &[("collection", "authors")]),
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
            "author": { "target": "authors", "item": 1 },
            "also": [
                { "target": "authors", "item": 3 },
                { "target": "authors", "item": 1 },
                { "target": "authors", "item": 3 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Read back in the order they were written, with the repeat dropped: the order is what a site
    // shows, and it is the editor's, not the id order.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["author"], json!({ "target": "authors", "item": 1 }));
    assert_eq!(
        body["also"],
        json!([
            { "target": "authors", "item": 3 },
            { "target": "authors", "item": 1 },
        ])
    );

    // Reordering is a save like any other, and what the site shows is the new order.
    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        json!({
            "title": "Hello",
            "author": { "target": "authors", "item": 1 },
            "also": [
                { "target": "authors", "item": 1 },
                { "target": "authors", "item": 3 },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        body["also"],
        json!([
            { "target": "authors", "item": 1 },
            { "target": "authors", "item": 3 },
        ]),
        "the order the editor saved"
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

    // The schema kept what it was told, including the name the other side is known by. One target
    // is a bare `Relation`; several are an `Array` whose item type is a relation.
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
    assert_eq!(
        body[1]["field_type"]["Relation"]["inverse_name"],
        "articles"
    );
    assert_eq!(
        body[2]["field_type"]["Array"][0]["Relation"]["target"]["name"],
        "authors"
    );
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
        json!([
            text_field("title", true),
            relation_field("landing", "single_page", "home"),
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
            "landing": { "target": "home" },
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
    assert_eq!(body["landing"], json!({ "target": "home" }));

    // An id for a page is a value the schema has nowhere to keep.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        Some(&token),
        Some(json!({
            "title": "Hello",
            "landing": { "target": "home", "item": 1 },
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
        json!([relation_field("author", "collection", "writers")]),
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
        json!([relation_field("home", "single_page", "authors")]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("'authors' is not a single page"), "{body}");

    // A composite definition is stored on its own and reused, so its save is where its own
    // relation's target is checked - and it is checked again when a schema embeds it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/composite_fields/cta",
        json!([relation_field("author", "collection", "writers")]),
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
        json!([relation_field("author", "collection", "authors")]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Several references are an `Array` of relations, and one array may name several *different*
/// targets. What it may not do is name the same target twice: an element carries its target and
/// not which declaration wrote it, so the reverse lookup could not tell the two apart.
#[tokio::test]
async fn an_array_of_relations_may_name_each_target_once() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    for name in ["authors", "categories"] {
        save_schema(
            &app,
            &token,
            &format!("/api/models/collections/{name}/schema"),
            json!([text_field("title", true)]),
        )
        .await;
    }
    for name in ["home", "about"] {
        save_schema(
            &app,
            &token,
            &format!("/api/models/single_pages/{name}/schema"),
            json!([]),
        )
        .await;
    }

    // Several different collections, and several different pages: both are what an array may hold.
    save_schema(
        &app,
        &token,
        "/api/models/collections/good/schema",
        json!([
            text_field("title", true),
            relation_array_field(
                "related",
                &[("collection", "authors"), ("collection", "categories")],
            ),
            relation_array_field(
                "landing",
                &[("single_page", "home"), ("single_page", "about")]
            ),
        ]),
    )
    .await;

    // A collection declared twice in one array is what it may not do.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/twice/schema",
        json!([
            text_field("title", true),
            relation_array_field(
                "related",
                &[("collection", "authors"), ("collection", "authors")],
            ),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("'authors' is declared twice"), "{body}");

    // A second item type for one page is the same target twice.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/twice/schema",
        json!([
            text_field("title", true),
            relation_array_field(
                "landing",
                &[("single_page", "home"), ("single_page", "home")],
            ),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("'home' is declared twice"), "{body}");
}

/// A reference has to match the field that holds it, and the shape has to match too: a `Relation`
/// holds one (or `null`), and several are an `Array` (or `[]`).
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
        json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
            relation_array_field("also", &[("collection", "authors")]),
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
            "author": { "target": "writers", "item": 1 },
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
            "author": { "target": "authors" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("item id"), "{body}");

    // A single reference holds one, and the shape that several references use is not one.
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
    assert!(
        body.contains("expected one { target, item } reference"),
        "{body}"
    );

    // A single reference is an object, not a scalar either.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "author": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.contains("expected one { target, item } reference"),
        "{body}"
    );

    // Several references are a list, not a scalar.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "also": 3 }),
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
        json!([required_relation_array_field(
            "author",
            &[("collection", "authors")],
        )]),
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

/// The same emptiness rule for a bare `Relation`: `null` saves as a draft, publishing asks for one.
#[tokio::test]
async fn a_single_required_relation_is_empty_until_it_has_a_target() {
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
        "/api/models/collections/strict/schema",
        json!([required_relation_field("author", "collection", "authors")]),
    )
    .await;

    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/strict/item",
        json!({ "author": null }),
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
        json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
            relation_array_field("also", &[("collection", "authors")]),
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
            "author": { "target": "authors", "item": 1 },
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
            "author": null,
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
            "author": { "target": "authors", "item": 1 },
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
        json!([relation_field("author", "collection", "authors")]),
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
            "cta": { "author": { "target": "authors", "item": 1 } },
            "blocks": [{ "author": null }],
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
            "cta": { "author": null },
            "blocks": [{ "author": { "target": "authors", "item": 1 } }],
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
    assert_eq!(body["cta"]["values"]["author"], json!(null));
    assert_eq!(body["blocks"][0]["values"]["author"], json!(null));
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
        json!([
            text_field("title", true),
            relation_field("landing", "single_page", "home"),
        ]),
    )
    .await;

    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "landing": { "target": "home" } }),
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
        json!([relation_field("author", "collection", "authors")]),
    )
    .await;

    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        "/api/models/single_pages/about/item",
        json!({ "author": { "target": "authors", "item": 1 } }),
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
        json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
            relation_array_field("also", &[("collection", "authors")]),
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
            "author": { "target": "authors", "item": 1 },
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
    // working copy an editor would open. One relation is emptied to `null`, an array to `[]`.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["author"], json!(null));
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
        json!([
            text_field("title", true),
            relation_field("landing", "single_page", "home"),
        ]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "landing": { "target": "home" } }),
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
    assert_eq!(body["landing"], json!(null));

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

    // Nothing in this schema is a relation, so no author references any other. Deleting is about
    // *being* referenced, not about referencing something; a collection of plain text fields is
    // therefore deletable without the detach flag, which is the floor this file's rules sit on.
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

/// A required relation is a promise that the site can serve it: publishing content whose only
/// target is still a draft would break that promise, so the publish is refused - and the same rule
/// refuses taking the last published target off the site while somebody requires it.
#[tokio::test]
async fn a_required_relation_needs_a_published_target_to_go_live() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let categories = "/api/models/collections/categories";

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

    // The category requires the author, and the author is only a draft.
    save_schema(
        &app,
        &token,
        &format!("{categories}/schema"),
        json!([
            text_field("title", true),
            required_relation_field("author", "collection", "authors"),
        ]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/item"),
        json!({ "title": "Tech", "author": { "target": "authors", "item": 1 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("relation_unpublished"), "{body}");
    assert!(body.contains("authors item 1"), "{body}");

    // Publishing the target first is the way through, which is the order a migration follows too.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/1/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Now the author is what the published category requires: taking it down is refused for the
    // same reason, from the other end, and the refusal names the content that requires it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/1/unpublish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("relation_required_by"), "{body}");
    assert!(body.contains("categories item 1"), "{body}");

    // Unpublishing the category first frees it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/items/1/unpublish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/1/unpublish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// The rule is about being left with *nothing* published: another published target keeps the
/// field served, a referrer that is not on the site asks nothing of its targets, and an optional
/// relation is free to point at a draft (the delivery API drops it).
#[tokio::test]
async fn only_a_required_relation_that_would_be_left_empty_holds_a_target() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let categories = "/api/models/collections/categories";
    let posts = "/api/models/collections/posts";

    save_schema(
        &app,
        &token,
        "/api/models/collections/authors/schema",
        json!([text_field("title", true)]),
    )
    .await;
    for (index, title) in ["Ada", "Grace"].iter().enumerate() {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            "/api/models/collections/authors/item",
            json!({ "title": title }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        // Both authors are on the site before the category that requires them.
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            &format!(
                "/api/models/collections/authors/items/{}/publish",
                index + 1
            ),
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // A required set: two published authors, so one of them can go.
    save_schema(
        &app,
        &token,
        &format!("{categories}/schema"),
        json!([
            text_field("title", true),
            required_relation_array_field("authors", &[("collection", "authors")]),
        ]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/item"),
        json!({
            "title": "Tech",
            "authors": [
                { "target": "authors", "item": 1 },
                { "target": "authors", "item": 2 },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // One of two going is one the site still serves.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/1/unpublish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The last one is not.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/2/unpublish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("relation_required_by"), "{body}");

    // An optional relation does not hold anything: taking its only target down is allowed.
    save_schema(
        &app,
        &token,
        &format!("{posts}/schema"),
        json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
        ]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/item"),
        json!({ "title": "Hello", "author": { "target": "authors", "item": 2 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The category is unpublished, so its required set asks nothing; the post's relation is
    // optional, so the site drops it rather than refusing the unpublish.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{categories}/items/1/unpublish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/2/unpublish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// What a site is served: a reference to something unpublished is gone (an editor still sees it,
/// because the reference is what they are working on), and `?populate=` carries the published
/// values of what a field points at, one level deep.
#[tokio::test]
async fn the_delivery_api_drops_unpublished_references_and_expands_what_it_is_asked_for() {
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
    // One author on the site, one still a draft.
    for (index, title) in ["Ada", "Grace"].iter().enumerate() {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            "/api/models/collections/authors/item",
            json!({ "title": title }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if index == 0 {
            let (status, body) = send_json(
                &app,
                &token,
                Method::POST,
                "/api/models/collections/authors/items/1/publish",
                json!(null),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }

    save_schema(
        &app,
        &token,
        &format!("{posts}/schema"),
        json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
        ]),
    )
    .await;

    // A post pointing at the draft: publishing it is fine (the relation is optional), and the
    // site is served no reference at all.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/item"),
        json!({ "title": "Hello", "author": { "target": "authors", "item": 2 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, delivered) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(
        delivered["values"]["author"],
        json!(null),
        "a site cannot follow it"
    );

    // The editor still sees what they picked: it is the reference they are working on.
    let (_, managed) = send(
        &app.router,
        Method::GET,
        &format!("{posts}/items/1"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(managed["author"], json!({ "target": "authors", "item": 2 }));

    // Pointed at the author that is on the site, the reference is served - bare by default.
    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        &format!("{posts}/items/1"),
        json!({ "title": "Hello", "author": { "target": "authors", "item": 1 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, delivered) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(
        delivered["values"]["author"],
        json!({ "target": "authors", "item": 1 }),
        "the reference, without the values behind it"
    );

    // `?populate=` is what asks for the values, and one level is all it gives.
    let (_, populated) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=author",
        None,
        None,
    )
    .await;
    assert_eq!(
        populated["values"]["author"],
        json!({
            "target": "authors",
            "item": 1,
            "values": { "title": "Ada" },
        })
    );

    // The list route expands the same way, so a site does not need a request per row.
    let (_, listed) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts?populate=author",
        None,
        None,
    )
    .await;
    assert_eq!(
        listed["items"][0]["values"]["author"]["values"]["title"],
        "Ada"
    );

    // A name that is not a relation field is refused rather than answered with nothing: the
    // caller is a build, and a typo it never hears about is the expensive kind.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=nope",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// A page is served the same way, through the same rules.
#[tokio::test]
async fn a_page_is_served_the_references_it_can_follow() {
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
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/authors/items/1/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home", "author": { "target": "authors", "item": 1 } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, page) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home?populate=author",
        None,
        None,
    )
    .await;
    assert_eq!(
        page["values"]["author"],
        json!({ "target": "authors", "item": 1, "values": { "title": "Ada" } })
    );
}

/// The other side of a relation, which a site needs to build a page: the articles of a category,
/// and the articles that name a category, under the name that side gives the relation.
#[tokio::test]
async fn a_site_can_read_a_relation_from_the_other_side() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let articles = "/api/models/collections/articles";

    save_schema(
        &app,
        &token,
        "/api/models/collections/categories/schema",
        json!([text_field("title", true)]),
    )
    .await;
    for title in ["Tech", "Life"] {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            "/api/models/collections/categories/item",
            json!({ "title": title }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    for id in [1, 2] {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            &format!("/api/models/collections/categories/items/{id}/publish"),
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // The articles name the category relation `articles` on the other side.
    save_schema(
        &app,
        &token,
        &format!("{articles}/schema"),
        json!([
            text_field("title", true),
            relation_field_named("category", "collection", "categories", "articles"),
        ]),
    )
    .await;

    // Three articles in the first category, two of them on the site.
    for (index, category) in [1u64, 2, 1].iter().enumerate() {
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            &format!("{articles}/item"),
            json!({
                "title": format!("Article {}", index + 1),
                "category": { "target": "categories", "item": category },
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if index < 2 {
            let (status, body) = send_json(
                &app,
                &token,
                Method::POST,
                &format!("{articles}/items/{}/publish", index + 1),
                json!(null),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }

    // The filter reads the index and the published copy decides: the draft is not in the list.
    let (status, page) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/articles?where=category:1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1, "{page}");
    assert_eq!(page["items"][0]["id"], 1);
    assert_eq!(page["items"][0]["values"]["title"], "Article 1");

    // Paging is over the filtered set, not over the collection.
    let (_, second) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/articles?where=category:2",
        None,
        None,
    )
    .await;
    assert_eq!(second["total"], 1);
    assert_eq!(second["items"][0]["id"], 2);
    let (_, paged) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/articles?where=category:1&limit=1&offset=1",
        None,
        None,
    )
    .await;
    assert_eq!(paged["total"], 1);
    assert_eq!(paged["items"], json!([]));
    assert_eq!(paged["next_offset"], json!(null));

    // A field that is not there, or is not a relation, is a refusal rather than an empty page.
    for query in [
        "where=nope:1",
        "where=title:1",
        "where=category",
        "where=category:abc",
    ] {
        let (status, _) = send(
            &app.router,
            Method::GET,
            &format!("/api/content/collections/articles?{query}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }

    // The inverse expansion: the category carries the articles that name it, by `inverse_name`,
    // with the values a site would render - and only the ones it serves.
    let (status, category) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/categories/items/1?populate=articles",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        category["values"]["articles"],
        json!([{
            "target": "articles",
            "item": 1,
            "values": { "title": "Article 1", "category": { "target": "categories", "item": 1 } },
        }])
    );

    // A name the site does declare, with nothing referencing this item yet: an empty list, not a
    // refusal - a build asking about a category with no articles is a normal question.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/categories/item",
        json!({ "title": "Empty" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/categories/items/3/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, empty) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/categories/items/3?populate=articles",
        None,
        None,
    )
    .await;
    assert_eq!(empty["values"]["articles"], json!([]));

    // A name nothing answers is refused, and the cap is the route's own `limit`.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/categories/items/1?populate=nope",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, capped) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/categories/items/1?populate=articles&limit=0",
        None,
        None,
    )
    .await;
    assert_eq!(
        capped["values"]["articles"],
        json!([]),
        "the cap is what `limit` says"
    );
}

/// The inverse direction reaches into an array item type: the referrer holds several references,
/// and still answers `?populate=<inverse_name>` as one list of the content that holds the target.
#[tokio::test]
async fn an_array_of_relations_answers_the_inverse_direction_too() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let articles = "/api/models/collections/articles";

    save_schema(
        &app,
        &token,
        "/api/models/collections/categories/schema",
        json!([text_field("title", true)]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/categories/item",
        json!({ "title": "Tech" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/categories/items/1/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The array's item type is the one that names the other side.
    save_schema(
        &app,
        &token,
        &format!("{articles}/schema"),
        json!([
            text_field("title", true),
            relation_array_field_named("topics", "collection", "categories", "articles"),
        ]),
    )
    .await;

    // One published article holding the reference, one published holding none, and one draft
    // that holds it but is not on the site yet.
    for (index, title) in ["Holds it", "Holds none", "Still a draft"]
        .iter()
        .enumerate()
    {
        let topics = if index == 1 {
            json!([])
        } else {
            json!([{ "target": "categories", "item": 1 }])
        };
        let (status, body) = send_json(
            &app,
            &token,
            Method::POST,
            &format!("{articles}/item"),
            json!({ "title": title, "topics": topics }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if index < 2 {
            let (status, body) = send_json(
                &app,
                &token,
                Method::POST,
                &format!("{articles}/items/{}/publish", index + 1),
                json!(null),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }

    // The published referrer comes back under the inverse name, as a list; the one holding nothing
    // and the draft do not.
    let (status, category) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/categories/items/1?populate=articles",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        category["values"]["articles"],
        json!([{
            "target": "articles",
            "item": 1,
            "values": {
                "title": "Holds it",
                "topics": [{ "target": "categories", "item": 1 }],
            },
        }])
    );

    // A name nothing declares is still 400, array or not.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/categories/items/1?populate=nope",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Several references of several targets: the value keeps the written order, a duplicate is one
/// entry, and delivery expands only the target the client names.
#[tokio::test]
async fn an_array_of_relations_round_trips_and_delivery_expands_the_named_target() {
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
    save_schema(
        &app,
        &token,
        "/api/models/collections/categories/schema",
        json!([text_field("title", true)]),
    )
    .await;
    for (path, title) in [
        ("/api/models/collections/authors/item", "Ada"),
        ("/api/models/collections/categories/item", "Tech"),
    ] {
        let (status, body) =
            send_json(&app, &token, Method::POST, path, json!({ "title": title })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    for path in [
        "/api/models/collections/authors/items/1/publish",
        "/api/models/collections/categories/items/1/publish",
    ] {
        let (status, body) = send_json(&app, &token, Method::POST, path, json!(null)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    save_schema(
        &app,
        &token,
        &format!("{posts}/schema"),
        json!([
            text_field("title", true),
            relation_array_field(
                "related",
                &[("collection", "authors"), ("collection", "categories")],
            ),
            relation_array_field("coauthors", &[("collection", "authors")]),
        ]),
    )
    .await;

    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/item"),
        json!({
            "title": "Hello",
            "related": [
                { "target": "authors", "item": 1 },
                { "target": "categories", "item": 1 },
                { "target": "authors", "item": 1 },
            ],
            "coauthors": [{ "target": "authors", "item": 1 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The duplicate is one entry, at the place it first appeared; both targets stay.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("{posts}/items/1"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["related"],
        json!([
            { "target": "authors", "item": 1 },
            { "target": "categories", "item": 1 },
        ])
    );

    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        &format!("{posts}/items/1/publish"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Bare: both references, no values behind them.
    let (_, delivered) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(
        delivered["values"]["related"],
        json!([
            { "target": "authors", "item": 1 },
            { "target": "categories", "item": 1 },
        ])
    );

    // The name alone cannot say which target: refused, because guessing would serve the other one.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=related",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Naming one target expands only those elements; the others come back as they are.
    let (_, populated) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=related.authors",
        None,
        None,
    )
    .await;
    assert_eq!(
        populated["values"]["related"],
        json!([
            { "target": "authors", "item": 1, "values": { "title": "Ada" } },
            { "target": "categories", "item": 1 },
        ])
    );
    let (_, populated) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=related.categories",
        None,
        None,
    )
    .await;
    assert_eq!(
        populated["values"]["related"],
        json!([
            { "target": "authors", "item": 1 },
            { "target": "categories", "item": 1, "values": { "title": "Tech" } },
        ])
    );

    // One target declared: the field name is enough.
    let (_, populated) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=coauthors",
        None,
        None,
    )
    .await;
    assert_eq!(
        populated["values"]["coauthors"],
        json!([{ "target": "authors", "item": 1, "values": { "title": "Ada" } }])
    );

    // A target the field does not name is refused, and the filter's bare form is refused too:
    // with several targets the name alone does not say which one the caller meant.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/1?populate=related.nope",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts?where=related:1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The filter names one target of several, and the index answers who holds it.
    let (status, filtered) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts?where=related.authors:1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(filtered["total"], 1, "{filtered}");
    assert_eq!(filtered["items"][0]["id"], 1);

    // With one target declared the field name already says which one, so the bare form works.
    let (status, filtered) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts?where=coauthors:1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(filtered["total"], 1, "{filtered}");
    assert_eq!(filtered["items"][0]["id"], 1);
}

/// Deleting a whole collection is the same rule as deleting one item: nothing that points at what
/// goes may be left behind.
#[tokio::test]
async fn a_collection_that_is_pointed_at_is_not_deleted() {
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
        json!([
            text_field("title", true),
            relation_field("author", "collection", "authors"),
        ]),
    )
    .await;
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/item",
        json!({ "title": "Hello", "author": { "target": "authors", "item": 1 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The author is referenced, so the collection that holds it does not go.
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("still_referenced"), "{body}");
    assert!(body.contains("posts item 1"), "{body}");

    // Removing the reference *and publishing the removal* is what frees it: the published copy is
    // what the site serves, so it counts until it changes.
    let (status, body) = send_json(
        &app,
        &token,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        json!({ "title": "Hello", "author": null }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/posts/items/1/publish",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_json(
        &app,
        &token,
        Method::DELETE,
        "/api/models/collections/authors",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/authors/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the collection is gone");
}

/// An inverse name is how the other side addresses a relation, so one target cannot have two
/// relations answering to it: `?populate=articles` on a category would depend on who asked.
#[tokio::test]
async fn an_inverse_name_belongs_to_one_relation() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    for name in ["categories", "sections"] {
        save_schema(
            &app,
            &token,
            &format!("/api/models/collections/{name}/schema"),
            json!([text_field("title", true)]),
        )
        .await;
    }

    // The relation the name belongs to.
    save_schema(
        &app,
        &token,
        "/api/models/collections/articles/schema",
        json!([
            text_field("title", true),
            relation_field_named("category", "collection", "categories", "articles"),
        ]),
    )
    .await;
    // Saving the same schema again is the same declaration, not a second one.
    let (status, body) = send(
        &app.router,
        Method::PUT,
        "/api/models/collections/articles/schema",
        Some(&token),
        Some(json!([
            text_field("title", true),
            relation_field_named("category", "collection", "categories", "articles"),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // One schema cannot give the same target the same name twice either.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/dupes/schema",
        json!([
            text_field("title", true),
            relation_field_named("category", "collection", "categories", "articles"),
            relation_field_named("topic", "collection", "categories", "articles"),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("'articles'"), "{body}");

    // Another collection calling the *same target* the same name is refused, and the refusal says
    // who has it.
    let (status, body) = send_json(
        &app,
        &token,
        Method::POST,
        "/api/models/collections/cards/schema",
        json!([
            text_field("title", true),
            relation_field_named("category", "collection", "categories", "articles"),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("articles"), "{body}");
    assert!(body.contains("'articles'"), "{body}");

    // The same name pointing at a *different* target is a different relation, and is fine: the
    // name is addressed from the target.
    save_schema(
        &app,
        &token,
        "/api/models/collections/cards/schema",
        json!([
            text_field("title", true),
            relation_field_named("section", "collection", "sections", "articles"),
        ]),
    )
    .await;

    // A page can declare one too, and the rule does not care which kind declares it.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(json!([
            text_field("title", true),
            relation_field_named("category", "collection", "categories", "articles"),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
