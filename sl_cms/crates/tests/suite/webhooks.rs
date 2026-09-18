//! Notifying the configured webhook, and what publishing does when it is unreachable.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

#[tokio::test]
async fn publishing_notifies_the_configured_webhook() {
    let (url, received) = start_webhook_receiver(StatusCode::OK).await;
    let secret = b"webhook-secret".to_vec();
    let app = test_app_with_notifier(Arc::new(
        WebhookNotifier::new(vec![url], Some(secret.clone())).unwrap(),
    ))
    .await;

    let item_id = create_sample_item(&app, "blog").await;

    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let deliveries = wait_for_webhooks(&received, 1).await;
    let first = &deliveries[0];
    assert_eq!(first.event.as_deref(), Some("collection_item.published"));
    assert!(
        first.delivery.is_some(),
        "a delivery id lets a receiver ignore a duplicate"
    );

    let body: Value = serde_json::from_slice(&first.body).unwrap();
    assert_eq!(body["event"], "collection_item.published");
    assert_eq!(body["collection"], "blog");
    assert_eq!(body["id"], item_id);
    assert_eq!(body["status"], "published");
    assert!(body["published_at"].is_string());
    // A collection event says nothing about single pages.
    assert!(body.get("page").is_none());

    // The signature has to verify over the exact bytes that were sent.
    let expected = format!(
        "sha256={}",
        sl_cms_core::webhook::sign(&secret, &first.body)
    );
    assert_eq!(first.signature.as_deref(), Some(expected.as_str()));

    // Unpublishing is an event too: the site has to be rebuilt without the page.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/unpublish"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let deliveries = wait_for_webhooks(&received, 2).await;
    assert_eq!(
        deliveries[1].event.as_deref(),
        Some("collection_item.unpublished")
    );
    let body: Value = serde_json::from_slice(&deliveries[1].body).unwrap();
    assert_eq!(body["status"], "draft");
    // The publication date stays part of the record; the publisher does not.
    assert!(body["published_at"].is_string());
    assert!(body["published_by"].is_null());
}

#[tokio::test]
async fn publishing_a_single_page_notifies_the_webhook() {
    let (url, received) = start_webhook_receiver(StatusCode::OK).await;
    let app =
        test_app_with_notifier(Arc::new(WebhookNotifier::new(vec![url], None).unwrap())).await;

    send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&app.admin_token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/publish",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let deliveries = wait_for_webhooks(&received, 1).await;
    let body: Value = serde_json::from_slice(&deliveries[0].body).unwrap();
    assert_eq!(body["event"], "single_page.published");
    assert_eq!(body["page"], "home");
    assert!(body.get("collection").is_none());
    // Without a configured secret the payload is delivered unsigned.
    assert!(deliveries[0].signature.is_none());
}

#[tokio::test]
async fn an_unreachable_webhook_receiver_does_not_break_publishing() {
    // Nothing listens on this port, so delivery fails at connect time.
    let url = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}/hook")
    };
    let app =
        test_app_with_notifier(Arc::new(WebhookNotifier::new(vec![url], None).unwrap())).await;
    let item_id = create_sample_item(&app, "blog").await;

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&app.admin_token),
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "publishing must not depend on a receiver being up"
    );
    assert_eq!(body["status"], "published");

    // The content is public even though the webhook could not be delivered.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
