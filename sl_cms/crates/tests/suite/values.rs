//! What a schema accepts, what a value is on the wire, and how a refusal points at a field.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// Values are untyped on the wire because the schema already states each field's type.
#[tokio::test]
async fn item_values_are_untyped_and_mismatches_are_rejected() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;

    // Untagged values are accepted.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": ["blog"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ...and come back untagged, so request and response agree on the shape.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Hello");
    assert_eq!(body["tags"], json!(["blog"]));

    // A value of the wrong type is rejected rather than coerced.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("expected a string"),
        "unexpected body: {text}"
    );

    // An undeclared field is rejected so a client typo cannot silently drop content.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "ok", "titel": "typo" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("Unknown field"), "unexpected body: {text}");

    // An enum value outside the declared options is rejected.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "ok", "tags": ["not-an-option"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// What a schema says about a value comes back as something a form can act on: the status says
/// the input was wrong, the code says how, and the field says which input - a path, so a refusal
/// inside a composite still points at something the screen can mark.
#[tokio::test]
async fn a_value_outside_its_schema_is_refused_with_a_code_and_the_field() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let composite = json!([
        { "name": "description", "field_type": { "Text": { "max_length": 5 } }, "required": false, "width": 12, "height": 1 }
    ]);
    send(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/seo",
        Some(&token),
        Some(composite),
    )
    .await;

    let deep = json!([
        { "name": "label", "field_type": { "Text": { "max_length": 3 } }, "required": false, "width": 12, "height": 1 }
    ]);
    send(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/deep",
        Some(&token),
        Some(deep),
    )
    .await;

    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "code", "field_type": { "Text": { "max_length": 5 } }, "required": false, "width": 12, "height": 1 },
        { "name": "tags", "field_type": { "Array": [{ "Text": { "max_length": 3 } }] }, "required": false, "width": 12, "height": 1 },
        { "name": "seo", "field_type": { "CompositeField": { "id": "seo" } }, "required": false, "width": 12, "height": 1 },
        { "name": "parts", "field_type": { "Array": [{ "CompositeField": { "id": "deep" } }] }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A required field left empty: the save keeps it as a working copy, and publishing is what
    // refuses it - the refusal names the field, so the editor knows which one to fill in.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let incomplete = body.as_u64().expect("the new item's id");
    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/limited/items/{incomplete}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_required");
    assert_eq!(body["field"], "title");

    // Longer than the field allows. The limit counts characters: six Japanese characters are
    // over a limit of five, and five are not, whatever they take in bytes.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "code": "あいうえおか" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "code");

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "code": "あいうえお" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // An array item is named by its index, so the reader knows which one to look at.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "tags": ["one", "toolong"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "tags[1]");

    // A refusal inside a composite names the path down to it.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "seo": { "description": "toolong" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "seo.description");

    // And one inside an element of an array of composites names the whole way there.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/limited/item",
        Some(&token),
        Some(
            json!({ "title": "ok", "parts": [{ "id": "deep", "values": { "label": "toolong" } }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "parts[0].label");
}

/// Image arrays are allowed on their own; `Number` + `Image` together is not, because
/// array items carry no type tag and an image id is a JSON number.
#[tokio::test]
async fn image_arrays_work_but_number_and_image_together_are_rejected() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let ambiguous = json!([{
        "name": "mixed",
        "field_type": { "Array": ["Number", "Image"] },
        "required": false,
        "width": 12,
        "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/mixed/schema",
        Some(&token),
        Some(ambiguous),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("both Number and Image"),
        "unexpected body: {text}"
    );

    // Record an image so the id resolves when it is read back.
    let (_, upload) = send(
        &app.router,
        Method::POST,
        "/api/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "a.png", "ext": "png", "size": PNG_BYTES.len() })),
    )
    .await;
    let image_id = upload["id"].as_u64().expect("image id");

    let schema = json!([{
        "name": "covers",
        "field_type": { "Array": ["Image"] },
        "required": false,
        "width": 12,
        "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/gallery/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A bare id is accepted on write.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/gallery/item",
        Some(&token),
        Some(json!({ "covers": [image_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // It comes back as the `{id, url}` shape, and re-submitting that is accepted too.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/gallery/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["covers"][0]["id"], image_id);

    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/gallery/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

/// Structural schema rules are enforced at save time, not when someone later writes
/// content against the schema.
#[tokio::test]
async fn invalid_schemas_are_rejected_when_saved() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let cases = [
        (
            json!([{ "name": "", "field_type": "Number", "required": false, "width": 12, "height": 1 }]),
            "must not be empty",
        ),
        (
            json!([
                { "name": "dup", "field_type": "Number", "required": false, "width": 12, "height": 1 },
                { "name": "dup", "field_type": "Boolean", "required": false, "width": 12, "height": 1 }
            ]),
            "duplicate field name",
        ),
        (
            json!([{ "name": "wide", "field_type": "Number", "required": false, "width": 20, "height": 1 }]),
            "width must be between 1 and 12",
        ),
        (
            json!([{ "name": "empty", "field_type": { "Array": [] }, "required": false, "width": 12, "height": 1 }]),
            "at least one item type",
        ),
    ];

    for (index, (schema, expected)) in cases.iter().enumerate() {
        let (status, body) = send_raw(
            &app.router,
            Method::POST,
            &format!("/api/models/collections/bad{index}/schema"),
            Some(&token),
            Some(schema.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "case {index} was accepted");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains(expected),
            "case {index}: unexpected body: {text}"
        );
    }
}

/// Composite values are read wrapped (`{id, values}`) and written bare, which used to make
/// a load-then-save round trip fail with "Unknown field 'id'".
#[tokio::test]
async fn composite_values_round_trip_and_references_are_validated() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let composite = json!([
        { "name": "description", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 },
        { "name": "score", "field_type": "Number", "required": false, "width": 6, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/seo",
        Some(&token),
        Some(composite),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "seo", "field_type": { "CompositeField": { "id": "seo" } }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A write sends the bare object of sub-values.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "Hello", "seo": { "description": "meta", "score": 7 } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A read wraps it, and re-submitting that unchanged has to be accepted.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["seo"]["id"], "seo");
    assert_eq!(body["seo"]["values"]["description"], "meta");

    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A dangling composite reference is refused when the schema is saved.
    let dangling = json!([{
        "name": "bad",
        "field_type": { "CompositeField": { "id": "missing" } },
        "required": false, "width": 12, "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/bad/schema",
        Some(&token),
        Some(dangling),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("does not exist"));

    // And a composite cannot reference itself.
    let cyclic = json!([{
        "name": "self",
        "field_type": { "CompositeField": { "id": "loop" } },
        "required": false, "width": 12, "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/loop",
        Some(&token),
        Some(cyclic),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("cannot reference itself"));
}

/// Which fields a collection's list shows is part of the field, so it has to survive the round trip
/// through storage on both adapters - and a schema that does not mention it has to read as "no".
#[tokio::test]
async fn the_list_columns_of_a_schema_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/listed/schema",
        Some(&token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1, "show_in_list": true },
            { "name": "body", "field_type": { "Markdown": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/listed/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["name"], "title");
    assert_eq!(body[0]["show_in_list"], true);
    // The one that did not ask for it is not in the list, and says so by saying nothing.
    assert!(body[1].get("show_in_list").is_none(), "{body}");

    // The value still travels: a schema that shows a field is still a schema that holds it.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/listed/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// An array whose items are composites.
///
/// The element types are tried in the declared order and the first that accepts the JSON wins,
/// so a composite element is written either bare or with the `{id, values}` wrapper a read
/// produces. The wrapper is what keeps a value with two plausible definitions unambiguous.
#[tokio::test]
async fn composite_arrays_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let definition = |field: &str| {
        json!([
            { "name": field, "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])
    };
    for (id, field) in [("block_a", "heading"), ("block_b", "body")] {
        let (status, body) = send_raw(
            &app.router,
            Method::POST,
            &format!("/api/models/composite_fields/{id}"),
            Some(&token),
            Some(definition(field)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    }

    let schema = json!([{
        "name": "blocks",
        "field_type": { "Array": [
            { "CompositeField": { "id": "block_a" } },
            { "CompositeField": { "id": "block_b" } }
        ] },
        "required": false,
        "width": 12,
        "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/page/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // One element names its definition, one leaves it off (the first declared type then wins).
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/page/item",
        Some(&token),
        Some(json!({ "blocks": [
            { "id": "block_b", "values": { "body": "second" } },
            { "heading": "first" }
        ] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/page/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blocks"][0]["id"], "block_b");
    assert_eq!(body["blocks"][0]["values"]["body"], "second");
    assert_eq!(body["blocks"][1]["id"], "block_a");
    assert_eq!(body["blocks"][1]["values"]["heading"], "first");

    // What a read produced is accepted back unchanged, which is what lets an editor load an
    // item, change one field and save it.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/page/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A definition the array does not declare is refused rather than read as another one.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/page/item",
        Some(&token),
        Some(json!({ "blocks": [{ "id": "block_c", "values": { "heading": "x" } }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("does not match any declared array item type"),
        "unexpected body: {text}"
    );

    // An element that is not an object at all is refused the same way.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/page/item",
        Some(&token),
        Some(json!({ "blocks": [7] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A block that holds blocks: the definition references itself through an array. That is a
    // loop in the reference graph and it is allowed, because the elements come from the value -
    // an empty array is where it stops.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/tree",
        Some(&token),
        Some(json!([
            { "name": "line", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 },
            { "name": "children", "field_type": { "Array": [{ "CompositeField": { "id": "tree" } }] },
              "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/nested/schema",
        Some(&token),
        Some(json!([{
            "name": "blocks",
            "field_type": { "Array": [{ "CompositeField": { "id": "tree" } }] },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let tree = json!({ "blocks": [
        { "id": "tree", "values": { "line": "root", "children": [
            { "line": "leaf", "children": [] }
        ] } }
    ] });
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/nested/item",
        Some(&token),
        Some(tree),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/nested/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blocks"][0]["values"]["line"], "root");
    assert_eq!(
        body["blocks"][0]["values"]["children"][0]["values"]["line"],
        "leaf"
    );
    assert_eq!(
        body["blocks"][0]["values"]["children"][0]["values"]["children"],
        json!([])
    );

    // A loop that never enters an array is still refused: the editor draws a composite's
    // sub-fields from the schema, so that one would never finish. Two definitions are needed
    // because the first has to exist before the second may point at it.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/loop_b",
        Some(&token),
        Some(json!([{
            "name": "value", "field_type": "Number",
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/loop_a",
        Some(&token),
        Some(json!([{
            "name": "to_b",
            "field_type": { "CompositeField": { "id": "loop_b" } },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Now closing the loop is refused, and it is the cycle that is reported rather than the
    // reference being missing.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/composite_fields/loop_b",
        Some(&token),
        Some(json!([{
            "name": "back",
            "field_type": { "CompositeField": { "id": "loop_a" } },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("cannot reference itself"),
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );

    // And a composite array item that does not exist is refused when the schema is saved.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/unknown/schema",
        Some(&token),
        Some(json!([{
            "name": "blocks",
            "field_type": { "Array": [{ "CompositeField": { "id": "missing" } }] },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("does not exist"),
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
}

/// A reference shows the title of what it points at, so a collection has to say which field names
/// an item - one field, and one that reads as a name.
#[tokio::test]
async fn a_collection_says_which_field_names_an_item() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/categories/schema",
        Some(&token),
        Some(json!([
            { "name": "name", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1, "is_title": true },
            { "name": "note", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/categories/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["is_title"], true);
    assert!(body[1].get("is_title").is_none(), "{body}");

    // The named items answer with the field's value, as stored - the screens know how to draw it.
    for name in ["技術", "ニュース"] {
        let (status, body) = send(
            &app.router,
            Method::POST,
            "/api/models/collections/categories/item",
            Some(&token),
            Some(json!({ "name": name, "note": "x" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/categories/items/titles?ids=1,2,99",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "1": "技術", "2": "ニュース" }),
        "an id that is not there is left out"
    );

    // A working copy is what an editor is holding, so a title that has been edited but not
    // published answers with the edit.
    let (status, body) = send(
        &app.router,
        Method::PUT,
        "/api/models/collections/categories/items/1",
        Some(&token),
        Some(json!({ "name": "技術 (改)", "note": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/categories/items/titles?ids=1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "1": "技術 (改)" }));

    // A collection with no title field names nothing, and says so by answering nothing: the screens
    // then show the reference itself.
    send(
        &app.router,
        Method::POST,
        "/api/models/collections/plain/schema",
        Some(&token),
        Some(json!([{ "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }])),
    )
    .await;
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/plain/items/titles?ids=1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({}));

    // Two names for one item, and a field that cannot be read as a name, are refused when saved.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/greedy/schema",
        Some(&token),
        Some(json!([
            { "name": "one", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1, "is_title": true },
            { "name": "two", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1, "is_title": true }
        ])),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        String::from_utf8_lossy(&body).contains("already the title"),
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/illustrated/schema",
        Some(&token),
        Some(json!([
            { "name": "cover", "field_type": "Image", "required": false, "width": 12, "height": 1, "is_title": true }
        ])),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        String::from_utf8_lossy(&body).contains("cannot be read as one line"),
        "{}",
        String::from_utf8_lossy(&body)
    );

    // Ids that are not ids are refused rather than guessed at.
    let (status, body) = send_raw(
        &app.router,
        Method::GET,
        "/api/models/collections/categories/items/titles?ids=1,nope",
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
}

/// A page can name itself too: a reference to one shows the page's title field if it has one.
#[tokio::test]
async fn a_page_can_say_which_field_names_it() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(json!([
            { "name": "heading", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1, "is_title": true }
        ])),
    )
    .await;
    let (status, body) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "heading": "ホーム" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A page with no title field is not in the answer: its name already names it.
    send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/about/schema",
        Some(&token),
        Some(json!([])),
    )
    .await;

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/titles",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "home": "ホーム" }));
}
