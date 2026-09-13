//! HTTP-level integration tests (T1.8).
//!
//! These drive the real router with `tower::ServiceExt::oneshot`, so routing, extractors,
//! the auth middleware, status codes and JSON shapes are all exercised — not just the
//! services underneath. Storage is the real on-premises adapter on a throwaway directory,
//! so the adapter is covered too.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use rkv::backend::{SafeMode, SafeModeEnvironment};
use rkv::{Manager, Rkv};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::app_module::AppModule;
use crate::auth::token::TokenIssuer;
use crate::http;
use crate::on_premises::repository::Repository;

const ADMIN_EMAIL: &str = "admin@example.com";
const ADMIN_PASSWORD: &str = "admin-password";
const VIEWER_EMAIL: &str = "viewer@example.com";
const VIEWER_PASSWORD: &str = "viewer-password";

/// A running router plus the temporary storage directory behind it.
struct TestApp {
    router: Router,
    dir: PathBuf,
    admin_token: String,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Best effort: the rkv environment may still be open.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn login(router: &Router, email: &str, password: &str) -> (StatusCode, Value) {
    let (status, body) = send(
        router,
        Method::POST,
        "/auth/login",
        None,
        Some(json!({ "email": email, "password": password })),
    )
    .await;
    (status, body)
}

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, bytes) = send_raw(router, method, uri, token, body).await;
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn send_raw(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .expect("failed to build request"),
        None => builder.body(Body::empty()).expect("failed to build request"),
    };

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router failed");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body")
        .to_vec();
    (status, bytes)
}

/// A schema with the shape the Angular client sends.
fn sample_schema() -> Value {
    json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "tags", "field_type": { "TextEnum": ["news", "blog"] }, "required": false, "width": 12, "height": 1 }
    ])
}

async fn test_app() -> TestApp {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = PathBuf::from(format!("./data/on_premises/test_http_{id}"));
    std::fs::create_dir_all(&dir).unwrap();

    let env = {
        let mut manager = Manager::<SafeModeEnvironment>::singleton().write().unwrap();
        manager
            .get_or_create_with_capacity(dir.as_path(), 512, Rkv::with_capacity::<SafeMode>)
            .unwrap()
    };
    let repository = Arc::new(Repository::new(env, dir.join("images")));
    let module = Arc::new(AppModule::new(
        repository,
        TokenIssuer::new(b"integration-test-secret", 1),
    ));

    module
        .auth_service
        .bootstrap_admin(Some(ADMIN_EMAIL), Some(ADMIN_PASSWORD))
        .unwrap()
        .expect("bootstrap admin");

    let router = http::router(
        module.clone(),
        http::cors_layer(&["http://localhost:4200".to_string()]),
    );
    let (status, body) = login(&router, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "admin login failed: {body}");
    let admin_token = body["token"].as_str().unwrap().to_string();

    TestApp { router, dir, admin_token }
}

async fn create_user(app: &TestApp, email: &str, password: &str, is_admin: bool) -> StatusCode {
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/users",
        Some(&app.admin_token),
        Some(json!({ "email": email, "password": password, "is_admin": is_admin })),
    )
    .await;
    status
}

// ---------------------------------------------------------------- liveness / routing

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
async fn content_routes_require_a_bearer_token() {
    let app = test_app().await;

    let (status, _) = send_raw(&app.router, Method::GET, "/models/collections", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send_raw(&app.router, Method::GET, "/models/collections", Some("garbage"), None).await;
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

#[tokio::test]
async fn login_rejects_bad_credentials_without_revealing_which_part_was_wrong() {
    let app = test_app().await;

    let (unknown_status, unknown_body) = login(&app.router, "nobody@example.com", ADMIN_PASSWORD).await;
    let (wrong_status, wrong_body) = login(&app.router, ADMIN_EMAIL, "not-the-password").await;

    assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_body, wrong_body);
}

#[tokio::test]
async fn me_returns_the_current_user_without_the_password_hash() {
    let app = test_app().await;

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/auth/me",
        Some(&app.admin_token),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], ADMIN_EMAIL);
    assert_eq!(body["is_admin"], true);
    assert!(body.get("password_hash").is_none(), "leaked hash: {body}");
}

// ------------------------------------------------------------------------ authorization

#[tokio::test]
async fn viewer_can_read_but_not_write_and_cannot_manage_users() {
    let app = test_app().await;
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );

    let (status, body) = login(&app.router, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["is_admin"], false);
    let viewer_token = body["token"].as_str().unwrap().to_string();

    // Reads are allowed.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Writes are not.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&viewer_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Account management is administrator-only even for editors.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn rejects_duplicate_and_weak_user_registrations() {
    let app = test_app().await;
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    // Duplicate (different case) -> 409.
    assert_eq!(
        create_user(&app, "VIEWER@example.com", VIEWER_PASSWORD, false).await,
        StatusCode::CONFLICT
    );
    // Too short -> 400.
    assert_eq!(create_user(&app, "other@example.com", "short", false).await, StatusCode::BAD_REQUEST);
}

// ------------------------------------------------------------------------------- content

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

    // Validation is enforced through the HTTP layer too. Error bodies are plain text,
    // so read them raw rather than as JSON.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "tags": ["news"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("title"), "unexpected body: {text}");

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

/// Values are untyped on the wire because the schema already states each field's type.
#[tokio::test]
async fn item_values_are_untyped_and_mismatches_are_rejected() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;

    // Untagged values are accepted.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": ["blog"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ...and come back untagged, so request and response agree on the shape.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items/1",
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
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("expected a string"), "unexpected body: {text}");

    // An undeclared field is rejected so a client typo cannot silently drop content.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
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
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "ok", "tags": ["not-an-option"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
        "/models/collections/mixed/schema",
        Some(&token),
        Some(ambiguous),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("both Number and Image"), "unexpected body: {text}");

    // Record an image so the id resolves when it is read back.
    let (_, upload) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "a.png", "ext": "png" })),
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
        "/models/collections/gallery/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A bare id is accepted on write.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/gallery/item",
        Some(&token),
        Some(json!({ "covers": [image_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // It comes back as the `{id, url}` shape, and re-submitting that is accepted too.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/gallery/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["covers"][0]["id"], image_id);

    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/gallery/items/1",
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
            &format!("/models/collections/bad{index}/schema"),
            Some(&token),
            Some(schema.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "case {index} was accepted");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains(expected), "case {index}: unexpected body: {text}");
    }
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
        (Method::PUT, "/models/single_pages/nope/item", Some(json!({}))),
        (Method::PUT, "/models/composite_fields/nope", Some(json!([]))),
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
async fn deleting_a_collection_without_items_succeeds() {    let app = test_app().await;
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

#[tokio::test]
async fn single_page_crud_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // FieldValueResponse is untagged, so the text value comes back bare.
    assert_eq!(body["title"], "Home");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/single_pages/home",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// -------------------------------------------------------------------------------- images

#[tokio::test]
async fn image_upload_requires_auth_but_downloads_are_public() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "logo.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let upload_url = body["upload_url"].as_str().unwrap().to_string();
    assert!(upload_url.starts_with("/images/"), "unexpected url: {upload_url}");

    // Uploading without a token is refused even with a valid capability token.
    let (status, _) = send_raw(&app.router, Method::PUT, &upload_url, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // With the token it succeeds.
    let request = Request::builder()
        .method(Method::PUT)
        .uri(&upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("PNG-BYTES"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Downloading needs no token (an <img> tag cannot send one).
    let path = upload_url.split('?').next().unwrap();
    let (status, bytes) = send_raw(&app.router, Method::GET, path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"PNG-BYTES");

    // The capability token was consumed.
    let request = Request::builder()
        .method(Method::PUT)
        .uri(&upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("PNG-BYTES"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_unsafe_image_file_names_and_extensions() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Percent-encoded traversal: axum decodes path segments after routing, so this must be
    // rejected by the handler rather than escaping the images directory.
    let (status, _) = send_raw(
        &app.router,
        Method::GET,
        "/images/..%2F..%2F..%2FCargo.toml",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "x", "ext": "../../etc/passwd" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------------- CORS

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
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "http://localhost:4200"
    );

    let request = Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}
