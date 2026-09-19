//! The HTTP contract suite: written once, run against every storage backend.
//!
//! These drive the real router with `tower::ServiceExt::oneshot`, so routing, extractors, the
//! auth middleware, status codes and JSON shapes are all exercised - not just the services
//! underneath. Storage is a real adapter, built by the harness (`sl_cms_tests::TestBackend`), so
//! the adapter is covered too: a backend that passes this directory is interchangeable with one
//! that does.
//!
//! The tests are the contract, so they may only use what every backend promises. The one
//! exception is image bytes, which an object-storage backend leaves to the object store; those
//! tests ask `Backend::SERVES_IMAGE_BYTES` first.
//!
//! This file is the harness: the imports, the `TestApp` alias that specialises the suite to one
//! backend, and the helpers every test file shares (through `use super::*`). Each topic is a file
//! of its own - what they are is in the `mod` list at the bottom.
//!
//! It is `mod.rs` rather than `suite.rs` because the runners pull the directory in with `#[path]`,
//! and such a module does not get a directory named after its file (see `src/lib.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::models::collection::{CollectionItemId, CollectionName};
use sl_cms_core::models::pagination::DEFAULT_PAGE_LIMIT;
use sl_cms_core::models::single_page::SinglePageName;
use sl_cms_core::models::user::Permission;
use sl_cms_core::preview_link::PreviewTarget;
use sl_cms_core::webhook::{Notifier, WebhookNotifier};
use sl_cms_tests::{
    ADMIN_EMAIL, ADMIN_PASSWORD, TEST_SECRET, TEST_TOKEN_TTL_HOURS, TestBackend, VIEWER_EMAIL,
    VIEWER_PASSWORD, send, send_raw, send_with_headers,
};

/// Which backend this copy drives. The runner the suite is compiled by (`tests/on_premises.rs`,
/// `tests/aws.rs`) defines the alias; nothing in here knows which one it is.
use crate::Backend;

/// Read a timestamp out of a JSON body, failing loudly when it is missing.
fn timestamp(value: &Value) -> chrono::DateTime<chrono::FixedOffset> {
    let raw = value
        .as_str()
        .unwrap_or_else(|| panic!("expected a timestamp, got {value}"));
    chrono::DateTime::parse_from_rfc3339(raw).expect("an RFC3339 timestamp")
}

/// A schema with the shape the Angular client sends.
fn sample_schema() -> Value {
    json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "tags", "field_type": { "TextEnum": ["news", "blog"] }, "required": false, "width": 12, "height": 1 }
    ])
}

/// The harness, specialised to the backend this copy of the suite runs against.
///
/// Every test names `TestApp`, so which storage it drives is decided by this one line.
type TestApp = sl_cms_tests::TestApp<Backend>;

async fn test_app() -> TestApp {
    TestApp::new().await
}

/// Same app, but with webhooks wired to `notifier`.
async fn test_app_with_notifier(notifier: Arc<dyn Notifier>) -> TestApp {
    TestApp::with_notifier(notifier).await
}

async fn create_user(app: &TestApp, email: &str, password: &str, is_admin: bool) -> StatusCode {
    create_account(
        app,
        json!({ "username": email, "password": password, "is_admin": is_admin }),
    )
    .await
    .0
}

/// Create an account, the way this deployment creates one.
///
/// Where the CMS stores the credential this is `POST /auth/users`, so the endpoint and its 201
/// and 409 are exercised. Where an identity provider owns the credential there is no such
/// endpoint yet — provisioning a Cognito user is `doc/aws-plan.md` P4 — so the account record is
/// written directly: the tests *about* permissions run against both deployments, and the ones
/// about passwords are gated on `Backend::PASSWORD_LOGIN`.
///
/// A `password` in the payload is not sent: creating an account chooses no credential. The helper
/// completes the two steps a deployment takes — create, then follow a reset link — so a test can
/// still ask for "an account that signs in with this password".
async fn create_account(app: &TestApp, payload: Value) -> (StatusCode, Value) {
    if app.password_login() {
        let mut payload = payload;
        let password = payload["password"].as_str().map(str::to_string);
        if let Some(object) = payload.as_object_mut() {
            object.remove("password");
        }
        let (status, created) = send(
            &app.router,
            Method::POST,
            "/auth/users",
            Some(&app.admin_token),
            Some(payload),
        )
        .await;
        let Some(password) = password.filter(|_| status == StatusCode::CREATED) else {
            return (status, created);
        };
        let id = created["id"].as_str().expect("the new account's id");
        let (status, link) = send(
            &app.router,
            Method::POST,
            &format!("/auth/users/{id}/password-reset-link"),
            Some(&app.admin_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "a reset link for a new account");
        let token = link["token"].as_str().expect("the reset token").to_string();
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/auth/password-reset",
            None,
            Some(json!({ "token": token, "new_password": password })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "the owner choosing a password");
        return (StatusCode::CREATED, created);
    }
    let username = payload["username"].as_str().expect("a username");
    let is_admin = payload["is_admin"].as_bool().unwrap_or(false);
    let permission: Permission = serde_json::from_value(payload["permission"].clone())
        .unwrap_or_else(|_| Permission::viewer());
    match app
        .backend()
        .create_account(username, is_admin, permission)
        .await
    {
        Ok(user) => (
            StatusCode::CREATED,
            serde_json::to_value(user.to_response()).expect("a serialisable account"),
        ),
        Err(_) => (StatusCode::CONFLICT, Value::Null),
    }
}

/// Sign in, the way this deployment signs in.
///
/// Where the CMS checks passwords this posts to `/auth/login`, so a test that needs a token
/// also exercises signing in. Where an identity provider does it there is no endpoint to post
/// to, and the harness mints the token the way a deployment does once the provider has
/// authenticated someone. The tests *about* passwords are gated on `Backend::PASSWORD_LOGIN`;
/// this is for the ones that only need to be someone.
async fn login(app: &TestApp, username: &str, password: &str) -> (StatusCode, Value) {
    if app.password_login() {
        return sl_cms_tests::login(&app.router, username, password).await;
    }
    // The same answer the real endpoint gives, assembled from the account record: the suite's
    // password tests are gated, and this keeps the rest reading like a sign-in.
    let user = app.backend().user_for(username).await;
    let (token, expires_at) = TokenIssuer::new(TEST_SECRET, TEST_TOKEN_TTL_HOURS)
        .issue(&user)
        .expect("a token");
    (
        StatusCode::OK,
        json!({ "token": token, "expires_at": expires_at, "user": user.to_response() }),
    )
}

/// The bytes a test uploads when it has no opinion about them. Twelve characters, so a test that
/// announces a size and one that sends bytes can be kept in step by saying `PNG_BYTES.len()`.
const PNG_BYTES: &[u8] = b"PNG-BYTES";

/// Put some bytes where the upload URL says, the way the browser does - announcing them first,
/// because that is what the upload URL is signed for on a deployment whose bytes go to S3.
async fn put_bytes(app: &TestApp, token: &str, upload_url: &str) {
    put_bytes_of(app, token, upload_url, PNG_BYTES).await;
}

/// The same, with the bytes a test chooses - a size test is about the size.
async fn put_bytes_of(app: &TestApp, token: &str, upload_url: &str, bytes: &[u8]) {
    let request = Request::builder()
        .method(Method::PUT)
        .uri(upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(bytes.to_vec()))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert!(
        response.status().is_success(),
        "the upload was refused: {}",
        response.status()
    );
}

/// The stored file name at the end of an upload URL, whichever deployment handed it out.
///
/// Backend-relative (`/images/<file>?key=...`) on-premises, an absolute presigned S3 URL on AWS;
/// both put the object's key in the last path segment, before any query.
fn file_name_of(upload_url: &str) -> String {
    upload_url
        .split('?')
        .next()
        .unwrap_or(upload_url)
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// One delivery the CMS posted to the test receiver.
#[derive(Debug, Clone)]
struct ReceivedWebhook {
    event: Option<String>,
    delivery: Option<String>,
    signature: Option<String>,
    body: Vec<u8>,
}

/// A local receiver for webhook deliveries, answering `status` to everything.
async fn start_webhook_receiver(
    status: StatusCode,
) -> (String, Arc<tokio::sync::Mutex<Vec<ReceivedWebhook>>>) {
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let sink = received.clone();

    let app = Router::new().route(
        "/hook",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let sink = sink.clone();
                async move {
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string)
                    };
                    sink.lock().await.push(ReceivedWebhook {
                        event: header("x-cms-event"),
                        delivery: header("x-cms-delivery"),
                        signature: header("x-cms-signature"),
                        body: body.to_vec(),
                    });
                    status
                }
            },
        ),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}/hook"), received)
}

/// Wait for `count` deliveries. Delivery is asynchronous by design, so the test polls.
async fn wait_for_webhooks(
    received: &Arc<tokio::sync::Mutex<Vec<ReceivedWebhook>>>,
    count: usize,
) -> Vec<ReceivedWebhook> {
    for _ in 0..300 {
        {
            let received = received.lock().await;
            if received.len() >= count {
                return received.clone();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let received = received.lock().await;
    panic!(
        "expected {count} webhook deliveries, got {}",
        received.len()
    );
}

/// Assert that a mutating call succeeded *and* answered with no body.
///
/// The browser client parses every non-empty body as JSON, so a plain-text acknowledgement
/// is indistinguishable from a failure for it.
async fn expect_empty_body(
    app: &TestApp,
    token: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) {
    let (status, bytes) = send_raw(&app.router, method.clone(), uri, Some(token), body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}");
    assert!(
        bytes.is_empty(),
        "{method} {uri} answered with a body: {:?}",
        String::from_utf8_lossy(&bytes)
    );
}

/// Create `collection` with one item and return the new item's id.
async fn create_sample_item(app: &TestApp, collection: &str) -> u64 {
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/{collection}/schema"),
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/{collection}/item"),
        Some(&app.admin_token),
        Some(json!({ "title": "Hello", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body.as_u64().expect("the item id")
}

/// Create `count` items in a fresh collection and publish all of them.
async fn create_published_items(app: &TestApp, collection: &str, count: u64) -> Vec<u64> {
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/{collection}/schema"),
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut ids = Vec::new();
    for index in 0..count {
        let (status, body) = send(
            &app.router,
            Method::POST,
            &format!("/models/collections/{collection}/item"),
            Some(&app.admin_token),
            Some(json!({ "title": format!("Item {index}"), "tags": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let id = body.as_u64().expect("the item id");

        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/models/collections/{collection}/items/{id}/publish"),
            Some(&app.admin_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        ids.push(id);
    }
    ids
}

/// Signing in, the accounts behind it, and the endpoints that carry a password.
mod auth;
/// Collections and their items: the CRUD round trip, deletion, duplication.
mod collections;
/// The delivery API and the admin list: what is reachable, and how a page is bounded.
mod content_api;
/// The HTTP layer itself: public routes, the bearer token, status codes, CORS, bodies.
mod http;
/// Getting bytes in: the upload target, who may use it, and replacing an image.
mod image_uploads;
/// Images that exist: the trash, the rename, the list, and who is using one.
mod images;
/// What each role and each grant allows, and when a change to them takes effect.
mod permissions;
/// Preview links: one working copy, without a token, until they expire.
mod preview;
/// Publishing a working copy: who may, what is refused, and what a batch does.
mod publishing;
/// Single pages and the states their list reports.
mod single_pages;
/// Slugs: how they are normalised, and when a field may become one.
mod slugs;
/// When content changed, and the publication date the delivery API reports.
mod timestamps;
/// The uniqueness contract: the index, what is reserved, and what a value resolves to.
mod unique_fields;
/// What a schema accepts, what a value is on the wire, and how a refusal points at a field.
mod values;
/// Notifying the configured webhook, and what publishing does when it is unreachable.
mod webhooks;
