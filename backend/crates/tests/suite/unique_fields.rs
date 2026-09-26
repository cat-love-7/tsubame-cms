//! The uniqueness contract: the index, what is reserved, and what a value resolves to.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// A field declared unique is checked by an index, not by a scan: the same value cannot be
/// stored twice, and the refusal names the field so a form can mark it.
#[tokio::test]
async fn a_unique_field_refuses_a_value_another_item_holds() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([{
        "name": "slug",
        "field_type": { "Text": {} },
        "required": false,
        "width": 12,
        "height": 1,
        "unique": true
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let create = |slug: serde_json::Value| {
        let token = token.clone();
        let router = app.router.clone();
        async move {
            send_raw(
                &router,
                Method::POST,
                "/api/models/collections/pages/item",
                Some(&token),
                Some(json!({ "slug": slug })),
            )
            .await
        }
    };

    let (status, body) = create(json!("intro")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = create(json!("guide")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // The value is taken, and the refusal says which field and which item.
    let (status, body) = create(json!("intro")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let refusal: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(refusal["code"], "value_taken");
    assert_eq!(refusal["field"], "slug");
    assert!(
        refusal["message"]
            .as_str()
            .unwrap_or_default()
            .contains("item 1"),
        "who holds it belongs in the message: {refusal}"
    );

    // An empty value is "not set": two items may leave an optional unique field blank.
    let (status, body) = create(json!("")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = create(json!("")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Saving an item with the value it already has is not a conflict with itself.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "intro" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Changing it gives the old value up...
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = create(json!("intro")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the value it no longer holds is free: {}",
        String::from_utf8_lossy(&body)
    );

    // ...and so does deleting the item.
    let (status, _) = send_raw(
        &app.router,
        Method::DELETE,
        "/api/models/collections/pages/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = create(json!("welcome")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "deleting frees what the item held: {}",
        String::from_utf8_lossy(&body)
    );
}

/// A published item keeps its published value reserved while a draft edits it: the live copy is
/// still what the delivery API serves, so another item must not be able to take its value.
#[tokio::test]
async fn a_published_value_stays_reserved_while_a_draft_changes_it() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([{
        "name": "slug",
        "field_type": { "Text": {} },
        "required": false,
        "width": 12,
        "height": 1,
        "unique": true
    }]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/item",
        Some(&token),
        Some(json!({ "slug": "intro" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The draft moves the item to another value; both are held.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    for taken in ["intro", "welcome"] {
        let (status, _) = send_raw(
            &app.router,
            Method::POST,
            "/api/models/collections/pages/item",
            Some(&token),
            Some(json!({ "slug": taken })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{taken} is still in use by the published copy or the working one"
        );
    }

    // Releasing the draft gives up the old published value and keeps the new one.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/item",
        Some(&token),
        Some(json!({ "slug": "intro" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the released value is free again: {}",
        String::from_utf8_lossy(&body)
    );
}

/// Taking a published item down frees the value its published copy held, while the working copy
/// keeps its own: nothing serves the published copy any more, but that draft is still an item's
/// content.
#[tokio::test]
async fn unpublishing_frees_the_published_value_but_not_the_working_one() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let unique = json!([{
        "name": "slug", "field_type": { "Text": {} },
        "required": false, "width": 12, "height": 1, "unique": true
    }]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(unique),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let create = |slug: &str| {
        let token = token.clone();
        let router = app.router.clone();
        let slug = slug.to_string();
        async move {
            send_raw(
                &router,
                Method::POST,
                "/api/models/collections/pages/item",
                Some(&token),
                Some(json!({ "slug": slug })),
            )
            .await
        }
    };
    assert_eq!(create("intro").await.0, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/items/1/unpublish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = create("intro").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "nothing serves the published copy any more: {}",
        String::from_utf8_lossy(&body)
    );
    let (status, _) = create("welcome").await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the working copy still holds its value"
    );
}

/// A unique value names one item, so both APIs can resolve one with a point read rather than a
/// scan - which is what a site needs to turn a slug into an item.
#[tokio::test]
async fn a_unique_value_resolves_to_its_item() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([
        {
            "name": "slug", "field_type": { "Text": {} },
            "required": false, "width": 12, "height": 1, "unique": true
        },
        { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/item",
        Some(&token),
        Some(json!({ "slug": "intro", "title": "Hello" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The management API finds it by value, and says which item it is.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/pages/items/by/slug/intro",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], 1);
    assert_eq!(body["values"]["title"], "Hello");

    // A value nobody holds is a 404, and a field that is not unique is a 400: the value would
    // not name one item.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/pages/items/by/slug/missing",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/pages/items/by/title/Hello",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nothing is published yet, so the delivery API answers "not found" rather than revealing
    // that a draft exists.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], 1);
    assert_eq!(body["values"]["title"], "Hello");
    assert!(body["last_published_at"].is_string());

    // A draft moving the value does not move the published one: the live value still resolves,
    // and the value waiting to be released does not (yet).
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome", "title": "Hello" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the served copy still uses it: {body}"
    );
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/pages/items/by/slug/welcome",
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a draft does not publish a value early"
    );
    // The management side still finds the item by either value: the index holds both.
    for value in ["intro", "welcome"] {
        let (status, body) = send(
            &app.router,
            Method::GET,
            &format!("/api/models/collections/pages/items/by/slug/{value}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}: {body}");
        assert_eq!(body["id"], 1);
    }

    // Releasing the change moves the delivery API over.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/pages/items/by/slug/welcome",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Making a field unique indexes what is already stored, and refuses while items would break it.
#[tokio::test]
async fn making_a_field_unique_indexes_the_items_already_stored() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let plain = json!([{
        "name": "slug", "field_type": { "Text": {} },
        "required": false, "width": 12, "height": 1
    }]);
    let unique = json!([{
        "name": "slug", "field_type": { "Text": {} },
        "required": false, "width": 12, "height": 1, "unique": true
    }]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(plain.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let item = |slug: &str| {
        let token = token.clone();
        let router = app.router.clone();
        let slug = slug.to_string();
        async move {
            send_raw(
                &router,
                Method::POST,
                "/api/models/collections/pages/item",
                Some(&token),
                Some(json!({ "slug": slug })),
            )
            .await
        }
    };
    assert_eq!(item("intro").await.0, StatusCode::OK);
    assert_eq!(item("guide").await.0, StatusCode::OK);

    // Nothing is duplicated, so the constraint can be switched on...
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(unique.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // ...and the items stored before it are in the index.
    let (status, body) = item("intro").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["field"],
        "slug"
    );

    // A duplicate that already exists is refused rather than silently indexed half-way.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(plain),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(item("intro").await.0, StatusCode::OK);
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(unique),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let refusal: Value = serde_json::from_slice(&body).unwrap();
    // Two stored items hold the same value, which is a different answer from a save that names a
    // value somebody else has: both items are named, because the editor has to change one of them.
    // (Which of the two the scan reached first is its business, so the pair is checked as a pair.)
    assert_eq!(refusal["code"], "duplicate_values");
    assert_eq!(refusal["field"], "slug");
    assert_eq!(refusal["details"]["value"], "intro");
    let item = refusal["details"]["item"].as_str().expect("an item id");
    let owner = refusal["details"]["owner"].as_str().expect("an item id");
    assert_ne!(item, owner, "both items have to be named: {refusal}");
    // The message is the log's, and it names the same pair, so an operator reading either sees
    // which two items to look at.
    let message = refusal["message"].as_str().expect("a message");
    assert!(
        message.contains(item) && message.contains(owner),
        "{refusal}"
    );
}

/// An item with two unique fields that collides on the second gives back the first: the save is
/// not happening, so the value it would have held has to be free again - otherwise it stays
/// reserved by an item that does not exist, and nobody can ever use it.
/// A field that stops being unique gives its values back.
///
/// The index is what refuses a value, and it only ever gave one back while its field was still
/// unique. Reusing a value while the constraint was off then refused the constraint's return:
/// the old claim was still there, held for an item that no longer had the value.
#[tokio::test]
async fn a_field_that_stops_being_unique_gives_its_values_back() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let schema = |unique: bool| {
        json!([
            { "name": "code", "field_type": { "Text": {} }, "required": true, "unique": unique, "width": 12, "height": 1 }
        ])
    };
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema(true)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let create = |body: Value| {
        let token = token.clone();
        let router = app.router.clone();
        async move {
            send(
                &router,
                Method::POST,
                "/api/models/collections/pages/item",
                Some(&token),
                Some(body),
            )
            .await
        }
    };
    let (status, first) = create(json!({ "code": "intro" })).await;
    assert_eq!(status, StatusCode::OK);
    let first = first.as_u64().expect("the first item's id");
    let (status, second) = create(json!({ "code": "guide" })).await;
    assert_eq!(status, StatusCode::OK);
    let second = second.as_u64().expect("the second item's id");

    // The field stops being unique.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema(false)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // While nothing is unique, the values move: one item takes what the other used to hold.
    let update = |id: u64, code: &str| {
        let token = token.clone();
        let router = app.router.clone();
        let code = code.to_string();
        async move {
            send_raw(
                &router,
                Method::PUT,
                &format!("/api/models/collections/pages/items/{id}"),
                Some(&token),
                Some(json!({ "code": code })),
            )
            .await
        }
    };
    let (status, body) = update(first, "news").await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = update(second, "intro").await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Unique again: one item holds "intro" now, and the index has to say the same.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema(true)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a value released with the constraint is still reserved: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn a_collision_on_one_unique_field_does_not_keep_the_other_reserved() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1, "unique": true },
        { "name": "code", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1, "unique": true }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let create = |body: Value| {
        let token = token.clone();
        let router = app.router.clone();
        async move {
            send(
                &router,
                Method::POST,
                "/api/models/collections/pages/item",
                Some(&token),
                Some(body),
            )
            .await
        }
    };

    // The first item holds both values.
    let (status, _) = create(json!({ "title": "first", "code": "one" })).await;
    assert_eq!(status, StatusCode::OK);

    // The second is refused on `code`, after `title` was free and had to be claimed.
    let (status, refusal) = create(json!({ "title": "second", "code": "one" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["field"], "code");

    // So "second" has to be free: the item that never existed must not be holding it.
    let (status, created) = create(json!({ "title": "second", "code": "two" })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the title claimed before the refusal is still reserved: {created}"
    );
    let second = created.as_u64().expect("the new item's id");

    // The same for a save: it claims the new title, then collides on the code item 1 holds.
    let (status, body) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/pages/items/{second}"),
        Some(&token),
        Some(json!({ "title": "third", "code": "one" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, created) = create(json!({ "title": "third", "code": "four" })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the title claimed by the refused save is still reserved: {created}"
    );
}
