//! Publishing a working copy: who may, what is refused, and what a batch does.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// Publishing promotes the working copy the publisher read, and only that one.
///
/// The race this is about needs no interleaving to stage: the repository is asked directly, with
/// content that is no longer the stored one. That is exactly what a save landing between the
/// caller's read and the promotion looks like from the repository's side, and the answer has to be
/// a refusal rather than a promotion that deletes the newer save.
#[tokio::test]
async fn publishing_a_working_copy_that_changed_under_it_is_refused() {
    use tsubame_core::models::item_status::ItemStatus;
    use tsubame_core::models::values::FieldValue;
    use tsubame_core::repositories::collection_repository::{
        ApplyStatusError, CollectionRepository,
    };

    let app = test_app().await;
    let token = app.admin_token.clone();
    let name = CollectionName::from("moving");

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/moving/schema",
        Some(&token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, created) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/moving/item",
        Some(&token),
        Some(json!({ "title": "first" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = CollectionItemId::from_u64(created.as_u64().expect("the item id"));

    let storage = app.backend().storage();
    let read = storage.get_collection_item_draft(&name, &id).await.unwrap();
    let read = read.expect("the item was saved as a working copy");

    // Somebody saves again, which is what moves the working copy under the publisher.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/moving/items/{}", *id),
        Some(&token),
        Some(json!({ "title": "second" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let metadata = storage
        .get_item_metadata(&name, &id)
        .await
        .unwrap()
        .unwrap_or_default()
        .with_status(ItemStatus::Published, None);

    // The content the publisher read is not the stored one any more.
    let refused = storage
        .apply_item_status(&name, &id, Some(&read), &metadata)
        .await
        .expect_err("promoting the older working copy has to be refused");
    assert_eq!(
        refused.downcast_ref::<ApplyStatusError>(),
        Some(&ApplyStatusError::DraftChanged),
        "{refused}"
    );

    // Nothing was applied: the item is still unpublished and the newer save is still there.
    let stored = storage.get_collection_item_draft(&name, &id).await.unwrap();
    assert_eq!(
        stored
            .expect("the newer working copy")
            .get("title")
            .map(|value| match value {
                FieldValue::Text(text) => text.clone(),
                other => format!("{other:?}"),
            }),
        Some("second".to_string())
    );
    assert!(
        !storage
            .get_item_metadata(&name, &id)
            .await
            .unwrap()
            .unwrap_or_default()
            .is_published()
    );

    // And publishing what is actually stored works.
    let current = storage.get_collection_item_draft(&name, &id).await.unwrap();
    storage
        .apply_item_status(&name, &id, current.as_ref(), &metadata)
        .await
        .expect("promoting the stored working copy");
    assert!(
        storage
            .get_item_metadata(&name, &id)
            .await
            .unwrap()
            .unwrap_or_default()
            .is_published()
    );

    // A publish with nothing pending is not affected: the working copy is gone now.
    assert!(
        storage
            .get_collection_item_draft(&name, &id)
            .await
            .unwrap()
            .is_none()
    );
}

/// The same race, for a single page: a page has exactly one working copy, so it has the same
/// promotion to lose.
#[tokio::test]
async fn publishing_a_page_working_copy_that_changed_under_it_is_refused() {
    use tsubame_core::models::item_status::ItemStatus;
    use tsubame_core::repositories::collection_repository::ApplyStatusError;
    use tsubame_core::repositories::single_page_repository::SinglePageRepository;

    let app = test_app().await;
    let token = app.admin_token.clone();
    let name = SinglePageName::from("about");

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/single_pages/about/schema",
        Some(&token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let save = |body: Value| {
        let router = app.router.clone();
        let token = token.clone();
        async move {
            send(
                &router,
                Method::PUT,
                "/api/models/single_pages/about/item",
                Some(&token),
                Some(body),
            )
            .await
            .0
        }
    };
    assert_eq!(save(json!({ "title": "first" })).await, StatusCode::OK);

    let storage = app.backend().storage();
    let read = storage
        .get_single_page_item_draft(&name)
        .await
        .unwrap()
        .expect("the page was saved as a working copy");

    assert_eq!(save(json!({ "title": "second" })).await, StatusCode::OK);

    let metadata = storage
        .get_page_metadata(&name)
        .await
        .unwrap()
        .unwrap_or_default()
        .with_status(ItemStatus::Published, None);

    let refused = storage
        .apply_page_status(&name, Some(&read), &metadata)
        .await
        .expect_err("promoting the older working copy has to be refused");
    assert_eq!(
        refused.downcast_ref::<ApplyStatusError>(),
        Some(&ApplyStatusError::DraftChanged),
        "{refused}"
    );
    assert!(
        !storage
            .get_page_metadata(&name)
            .await
            .unwrap()
            .unwrap_or_default()
            .is_published()
    );
    assert!(
        storage
            .get_single_page_item_draft(&name)
            .await
            .unwrap()
            .is_some()
    );

    // The stored working copy still publishes.
    let current = storage.get_single_page_item_draft(&name).await.unwrap();
    storage
        .apply_page_status(&name, current.as_ref(), &metadata)
        .await
        .expect("promoting the stored working copy");
    assert!(
        storage
            .get_page_metadata(&name)
            .await
            .unwrap()
            .unwrap_or_default()
            .is_published()
    );
}

/// A batch publish answers per item: one item's refusal does not stop the others, and the caller
/// can tell which is which.
#[tokio::test]
async fn a_batch_publishes_what_it_can_and_says_what_it_could_not() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/schema",
        Some(&token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    for title in ["one", "two"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/api/models/collections/blog/item",
            Some(&token),
            Some(json!({ "title": title })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // One of the ids is not there: the batch still publishes the other, and reports the miss.
    let (status, outcomes) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/status",
        Some(&token),
        Some(json!({ "ids": [1, 999, 2], "status": "published" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcomes[0]["outcome"], "changed");
    assert_eq!(outcomes[0]["id"], 1);
    assert_eq!(outcomes[1]["outcome"], "refused");
    assert_eq!(outcomes[1]["id"], 999);
    assert_eq!(outcomes[1]["code"], "not_found");
    assert_eq!(outcomes[2]["outcome"], "changed");

    // Both are on the site, and their status is what the batch asked for.
    for id in [1, 2] {
        let (status, _) = send_raw(
            &app.router,
            Method::GET,
            &format!("/api/content/collections/blog/items/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "item {id} should be live");
    }

    // The other way round, and the batch says the same shape.
    let (status, outcomes) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/status",
        Some(&token),
        Some(json!({ "ids": [1, 2], "status": "draft" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        outcomes
            .as_array()
            .unwrap()
            .iter()
            .all(|outcome| outcome["outcome"] == "changed")
    );
    let (status, _) = send_raw(
        &app.router,
        Method::GET,
        "/api/content/collections/blog/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A batch has a size, and an empty one is a mistake rather than a no-op.
    let too_many: Vec<u64> = (0..101).collect();
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/status",
        Some(&token),
        Some(json!({ "ids": too_many, "status": "published" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/status",
        Some(&token),
        Some(json!({ "ids": [], "status": "published" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Publishing is a write, so a read-only account cannot do it.
#[tokio::test]
async fn publishing_requires_edit_permission() {
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
    send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": [] })),
    )
    .await;

    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    let viewer_token = body["token"].as_str().unwrap().to_string();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/1/publish",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The public route stays reachable without a token.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Publishing leaves an audit trail: the account is recorded on the item, the admin side
/// can read it back, and the public delivery API never sees it.
#[tokio::test]
async fn publishing_records_who_did_it_and_keeps_that_off_the_public_api() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    // A second account, so "who published" is not just the admin who set everything up.
    let (status, body) = create_account(
        &app,
        json!({
            "username": "publisher@example.com",
            "password": "publisher-password",
            "is_admin": false,
            "permission": Permission { can_view: true, can_edit: true, can_publish: true },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, body) = login(&app, "publisher@example.com", "publisher-password").await;
    let publisher = body["token"].as_str().unwrap().to_string();

    let publish = format!("/api/models/collections/blog/items/{item_id}/publish");
    let (status, body) = send(&app.router, Method::POST, &publish, Some(&publisher), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["published_by"]["username"], "publisher@example.com");
    assert!(
        body["published_by"]["id"].is_string(),
        "the account id travels too: {body}"
    );

    // The admin metadata endpoint reports it as well: that is what the UI reads.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/collections/blog/items/{item_id}/metadata"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["published_by"]["username"], "publisher@example.com");

    // The delivery API says when it was published, never who did it: the operator's
    // address is not something a public site should be able to read.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/api/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["published_at"].is_string());
    assert!(
        body.get("published_by").is_none(),
        "the publisher must not reach the public API: {body}"
    );

    // Unpublishing takes the item off the site, so it forgets who put it there. The
    // publication date stays: it is a fact about the item, not about the current state.
    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/blog/items/{item_id}/unpublish"),
        Some(&publisher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["published_by"].is_null());
    assert!(body["published_at"].is_string());
}

/// The draft/published model, end to end.
///
/// A save lands in the working copy; publishing copies it to the published copy, which is
/// the only thing the delivery API serves. That separation is what makes "may edit but may
/// not publish" a safe role.
#[tokio::test]
async fn edits_wait_in_the_working_copy_until_they_are_published() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let item_url = format!("/api/models/collections/blog/items/{item_id}");
    let content_url = format!("/api/content/collections/blog/items/{item_id}");

    // A new item starts unpublished, even though its values are already stored.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"], json!([]));

    // Publishing puts the working copy in front of the delivery API.
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
    assert_eq!(body["values"]["title"], "Hello");

    // An edit is visible to the editor...
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &item_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Edited");

    // ...but the site keeps serving what was published.
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");

    // The metadata says so, so the admin list can show it.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("{item_url}/metadata"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");
    assert_eq!(body["has_draft"], true, "an unpublished edit is pending");

    // Unpublishing hides the item without losing either copy.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("{item_url}/unpublish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Publishing again releases the edit that was waiting, and clears the working copy.
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
    assert_eq!(body["values"]["title"], "Edited");
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("{item_url}/metadata"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["has_draft"], false,
        "nothing is pending after publishing"
    );

    // A preview shows the working copy before it is released.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Previewed", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("{item_url}/preview"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Previewed");
    assert_eq!(body["schema"][0]["name"], "title");
    // ...while the site still serves the published one.
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Edited");

    // Publishing again *while still published* releases the waiting edit without the item ever
    // leaving the site - which is what the "publish changes" control in the admin screens does.
    // The reply carries the new state, including whether anything is still waiting, because the
    // screens take it as the state they should show next.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Released", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("{item_url}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");
    assert_eq!(
        body["has_draft"], false,
        "the release clears the working copy"
    );
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK, "the item never left the site");
    assert_eq!(body["values"]["title"], "Released");

    // Taking it down keeps whatever was being worked on, and publishing after that restores it.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Waiting", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("{item_url}/unpublish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "draft");
    assert_eq!(body["has_draft"], true, "the working copy is kept");
    assert_eq!(
        send(&app.router, Method::GET, &content_url, None, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
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
    assert_eq!(body["values"]["title"], "Waiting");
}

/// A save that writes back what the site is already serving is not a change.
///
/// The screens report "unpublished changes" from `has_draft`, which is a working copy being there
/// at all - so an untouched save used to leave one that differed from the published copy in no
/// field: a notice no diff could justify, over a publish that would release nothing.
#[tokio::test]
async fn saving_what_the_site_already_serves_is_not_a_change() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let item_url = format!("/api/models/collections/blog/items/{item_id}");
    let metadata_url = format!("{item_url}/metadata");
    let content_url = format!("/api/content/collections/blog/items/{item_id}");

    // Publish, so there is a copy the site serves for the save to be unchanged from.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("{item_url}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, values) = send(&app.router, Method::GET, &item_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, before) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);

    // The editor opens the item and presses save without touching a field.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(values.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, after) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        after["has_draft"], false,
        "nothing is waiting to be published"
    );
    assert_eq!(
        timestamp(&after["updated_at"]),
        timestamp(&before["updated_at"]),
        "and the content clock does not move either"
    );

    // A change is still a change: it waits in the working copy, and the site keeps serving the
    // published copy until it is released.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, pending) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(pending["has_draft"], true, "an edit is waiting");
    let (_, live) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(live["values"]["title"], "Hello");

    // Saving the published values back is the undo: the working copy it was compared against is
    // gone, and nothing had to be published to get there.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(values),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, undone) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(
        undone["has_draft"], false,
        "the working copy it replaced is gone"
    );
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");
}
