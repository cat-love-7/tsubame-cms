//! Slugs: how they are normalised, and when a field may become one.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// A slug is normalised when it is written, so one canonical spelling is what is stored, what the
/// unique index holds and what a URL resolves. Two spellings of one slug therefore cannot become
/// two items, which is the whole reason the type exists.
#[tokio::test]
async fn a_slug_is_normalised_and_no_two_items_can_spell_it_differently() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "address", "field_type": { "Slug": { "generate_from": "title" } }, "required": false, "width": 12, "height": 1 }
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

    // Whatever was typed, what is stored is the canonical slug.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "Hello World", "address": "  Hello, World!  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = body.as_u64().unwrap_or(1);
    let (status, stored) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/posts/items/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["address"], "hello-world");

    // A second item that spells the same slug differently is refused, and the refusal names the
    // field so a form can mark it.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "Another", "address": "hello world" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "value_taken");
    assert_eq!(body["field"], "address");
    // What collided travels beside the code, so the screen can say which value and which item
    // rather than "a value is taken".
    assert_eq!(body["details"]["value"], "hello-world");
    assert_eq!(body["details"]["owner"], "1");

    // Empty is not a slug: nothing is reserved, so two items may leave it out.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "No address" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A value with nothing a slug can be made of is refused rather than stored as empty.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "日本語のタイトル", "address": "日本語" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_slug");
    assert_eq!(body["field"], "address");

    // A published item is found by its slug however the caller spells it: the lookup normalises
    // what it was asked for, which is what makes a canonical URL work.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/posts/items/{id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for asked in [
        "hello-world",
        "Hello%20World",
        "HELLO%20WORLD",
        "hello--world",
    ] {
        let (status, found) = send(
            &app.router,
            Method::GET,
            &format!("/api/models/collections/posts/items/by/address/{asked}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "looking up {asked}");
        assert_eq!(found["id"], id, "looking up {asked}");
        assert_eq!(
            found["values"]["address"], "hello-world",
            "looking up {asked}"
        );
    }

    // The delivery API answers the same way, so a site can resolve a slug.
    let (status, page) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/posts/items/by/address/Hello%20World",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["values"]["address"], "hello-world");

    // A slug that is already too long for a URL is refused with the same wording as a text field.
    let long = "a".repeat(201);
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "Too long", "address": long })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "address");
}

/// Making an existing text field a slug is refused while the stored values are not canonical: the
/// index would hold a spelling no lookup asks for, and the item that holds it would be invisible
/// under its own address. The items to fix are named by the refusal.
#[tokio::test]
async fn a_text_field_cannot_become_a_slug_while_its_values_are_not_canonical() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let text_field = json!([
        { "name": "address", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/schema",
        Some(&token),
        Some(text_field),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/item",
        Some(&token),
        Some(json!({ "address": "Hello World" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The same field, now a slug: refused, because what is stored is not one yet.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/schema",
        Some(&token),
        Some(json!([
            { "name": "address", "field_type": { "Slug": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let body: Value = serde_json::from_slice(&body).expect("an error body");
    // A code of its own, because this is not the same refusal as writing a value that cannot be a
    // slug: the value is already stored, and the answer has to say which item to fix.
    assert_eq!(body["code"], "slug_not_canonical");
    assert_eq!(body["field"], "address");
    assert_eq!(body["details"]["value"], "Hello World");
    assert_eq!(body["details"]["item"], "1");

    // While the field is still text, a save stores what it is given: the value has to be made a
    // slug by hand (which is what the refusal asks for), and then the schema is accepted.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/items/1",
        Some(&token),
        Some(json!({ "address": "hello-world" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/posts/schema",
        Some(&token),
        Some(json!([
            { "name": "address", "field_type": { "Slug": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // And it is found by its canonical address.
    let (status, found) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/posts/items/by/address/hello-world",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["values"]["address"], "hello-world");
}
