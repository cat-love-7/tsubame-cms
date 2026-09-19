//! The HTTP layer itself: public routes, the bearer token, status codes, CORS, bodies.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

#[tokio::test]
async fn root_is_public_and_unknown_routes_404_rather_than_401() {
    let app = test_app().await;

    let (status, _) = send_raw(&app.router, Method::GET, "/", None, None).await;
    assert_eq!(status, StatusCode::OK);

    // The auth middleware is applied per-route, so an unknown path must not be reported
    // as an authentication failure.
    let (status, _) = send_raw(&app.router, Method::GET, "/not-a-route", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ------------------------------------------------------------------------ authentication

#[tokio::test]
async fn model_routes_require_a_bearer_token() {
    let app = test_app().await;

    let (status, _) = send_raw(&app.router, Method::GET, "/models/collections", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send_raw(
        &app.router,
        Method::GET,
        "/models/collections",
        Some("garbage"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A token without the `Bearer ` prefix is not accepted either.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/models/collections")
        .header(header::AUTHORIZATION, &app.admin_token)
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// A missing resource is a 404 and a duplicate is a 409; both used to be reported as 400,
/// which made "you asked for something that is not there" indistinguishable from "your
/// request was malformed".
#[tokio::test]
async fn missing_resources_are_404_and_duplicates_are_409() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Nothing exists yet, so every one of these is "not found".
    let not_found = [
        (Method::DELETE, "/models/collections/nope", None),
        (
            Method::PUT,
            "/models/collections/nope/schema",
            Some(sample_schema()),
        ),
        (Method::DELETE, "/models/single_pages/nope", None),
        (
            Method::PUT,
            "/models/single_pages/nope/item",
            Some(json!({})),
        ),
        (
            Method::PUT,
            "/models/composite_fields/nope",
            Some(json!([])),
        ),
        (Method::DELETE, "/models/composite_fields/nope", None),
    ];
    for (method, uri, body) in not_found {
        let (status, _) = send_raw(&app.router, method.clone(), uri, Some(&token), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }

    // Creating the same thing twice conflicts with the current state.
    let duplicates = [
        ("/models/collections/dup/schema", sample_schema()),
        ("/models/single_pages/dup/schema", sample_schema()),
        ("/models/composite_fields/dup", sample_schema()),
    ];
    for (uri, body) in duplicates {
        let (status, _) = send_raw(
            &app.router,
            Method::POST,
            uri,
            Some(&token),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "first POST {uri} should succeed");

        let (status, _) = send_raw(&app.router, Method::POST, uri, Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "second POST {uri}");
    }

    // A genuinely invalid payload is still a 400.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/bad/schema",
        Some(&token),
        Some(json!([{ "name": "", "field_type": "Number", "required": false, "width": 12, "height": 1 }])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cors_allows_only_configured_origins() {
    let app = test_app().await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(header::ORIGIN, "http://localhost:4200")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "http://localhost:4200"
    );

    let request = Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

/// Mutations answer with an empty body on purpose; see [`expect_empty_body`].
#[tokio::test]
async fn mutations_answer_with_an_empty_body() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let schema = sample_schema();

    expect_empty_body(
        &app,
        &token,
        Method::POST,
        "/models/collections/blog/schema",
        Some(schema.clone()),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        "/models/collections/blog/schema",
        Some(schema.clone()),
    )
    .await;

    // Creating an item answers with its id, so it is checked separately from the mutations.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = body.as_u64().expect("the item id");

    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        &format!("/models/collections/blog/items/{id}"),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::DELETE,
        &format!("/models/collections/blog/items/{id}"),
        None,
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::DELETE,
        "/models/collections/blog",
        None,
    )
    .await;

    expect_empty_body(
        &app,
        &token,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(schema.clone()),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        "/models/single_pages/home/schema",
        Some(schema.clone()),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::DELETE,
        "/models/single_pages/home",
        None,
    )
    .await;

    expect_empty_body(
        &app,
        &token,
        Method::POST,
        "/models/composite_fields/seo",
        Some(schema.clone()),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        "/models/composite_fields/seo",
        Some(schema.clone()),
    )
    .await;
    expect_empty_body(
        &app,
        &token,
        Method::DELETE,
        "/models/composite_fields/seo",
        None,
    )
    .await;
}

/// The admin UI loads a page with several requests in flight at once: the item window, the
/// statuses, and the navigation. LMDB refuses to open a database while another transaction
/// is active, so the adapter has to keep storage operations from overlapping. When it did
/// not, a page load answered 500 ("attempted to open DB during transaction").
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_do_not_break_the_storage_environment() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let created = create_published_items(&app, "blog", 3).await;

    // The window the UI opens, split across the worker threads the way a browser's
    // separate connections are.
    let targets = [
        "/models/collections/blog/items?limit=25&offset=0",
        "/models/collections/blog/items/metadata",
        "/models/collections/blog/items?limit=25&offset=0",
        "/models/collections",
        "/models/single_pages",
        "/models/collections/blog/schema",
    ];

    // Several rounds: a race that misses in one may land in the next.
    for round in 0..5 {
        let mut handles = Vec::new();
        for target in targets {
            let router = app.router.clone();
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                send(&router, Method::GET, target, Some(&token), None).await
            }));
        }

        for handle in handles {
            let (status, body) = handle.await.expect("the request task panicked");
            assert_eq!(status, StatusCode::OK, "round {round}: {body}");
        }
    }

    // The items are still readable afterwards.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items?limit=25&offset=0",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().unwrap().len(), created.len());
}

// ------------------------------------------------------------------------------- body limits

/// A body over the limit is refused before anything reads it, in the shape of every other
/// refusal: the status says what happened, the code lets a client say it in its own language,
/// and the message names the size and the limit for whoever is reading a log.
#[tokio::test]
async fn a_body_over_the_request_limit_is_refused_in_the_cms_own_shape() {
    use sl_cms_core::config::DEFAULT_MAX_REQUEST_BYTES;

    let app = test_app().await;
    // The JSON around the padding counts too, so the body is measured as it is sent rather than
    // by the length of the string inside it.
    let envelope = json!({ "padding": "" }).to_string().len();
    let value = json!({ "padding": "a".repeat(DEFAULT_MAX_REQUEST_BYTES + 1 - envelope) });
    let sent = value.to_string().len();
    assert!(sent > DEFAULT_MAX_REQUEST_BYTES);

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&app.admin_token),
        Some(value),
    )
    .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["code"], "payload_too_large");
    let message = body["message"].as_str().expect("a message");
    assert!(
        message.contains(&sent.to_string())
            && message.contains(&DEFAULT_MAX_REQUEST_BYTES.to_string()),
        "the message should name the size sent and the limit: {message}"
    );
}

/// The boundary itself: a body of exactly the limit is read and answered normally (here with the
/// schema's own refusal), so the limit refuses what is over it and nothing else.
#[tokio::test]
async fn a_body_of_exactly_the_request_limit_is_read() {
    use sl_cms_core::config::DEFAULT_MAX_REQUEST_BYTES;

    let app = test_app().await;
    let envelope = json!({ "padding": "" }).to_string().len();
    let value = json!({ "padding": "a".repeat(DEFAULT_MAX_REQUEST_BYTES - envelope) });
    assert_eq!(value.to_string().len(), DEFAULT_MAX_REQUEST_BYTES);

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&app.admin_token),
        Some(value),
    )
    .await;

    assert_ne!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    // The body arrived and was read: what refused it is its shape (the route wants an array of
    // fields), which axum answers as 422 before the handler ever sees it.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}
