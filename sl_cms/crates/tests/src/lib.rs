//! The HTTP contract suite, and the harness that runs it against a storage backend.
//!
//! The suite itself is `suite.rs` at the root of this package. It is compiled once per backend
//! by the files in `tests/`, each of which defines which backend it is and then includes it:
//!
//! ```text
//! tests/on_premises.rs -> type Backend = OnPremises; mod contract { include!("../suite.rs") }
//! tests/aws.rs         -> type Backend = Aws;        mod contract { include!("../suite.rs") }
//! ```
//!
//! That is the reason this package exists. The suite is the contract, so a backend that passes
//! it is interchangeable with one that does — and one command (`cargo test -p sl-cms-tests`)
//! runs it against both, which is something a single crate selecting a backend with a Cargo
//! feature cannot do, because one build has one feature set.
//!
//! The AWS suite needs DynamoDB Local and MinIO from `sl_cms/docker-compose.yml`; it says so
//! loudly rather than passing quietly when they are missing. `scripts/test-rust.sh` checks for
//! them first and skips the whole file with a note.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use sl_cms_core::app_module::{AppModule, Storage};
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::password_reset::PasswordResetIssuer;
use sl_cms_core::preview_link::PreviewLinkIssuer;
use sl_cms_core::webhook::{NoopNotifier, Notifier};

pub mod backends;

/// The administrator every test signs in as.
pub const ADMIN_EMAIL: &str = "admin@example.com";
pub const ADMIN_PASSWORD: &str = "admin-password";
/// A second account, for the tests about permissions.
pub const VIEWER_EMAIL: &str = "viewer@example.com";
pub const VIEWER_PASSWORD: &str = "viewer-password";

/// What the suite needs from a storage backend.
///
/// Everything backend-specific about *running* the suite lives here: how a scratch storage is
/// made, how its routes are put together, and what it cannot do. The assertions themselves
/// know none of it.
pub trait TestBackend: Sized {
    type Storage: Storage;

    /// Whether this backend serves image bytes itself.
    ///
    /// An object-storage backend hands the browser an absolute, signed URL instead, so the
    /// tests that PUT bytes through this router only apply to the other kind.
    const SERVES_IMAGE_BYTES: bool;

    /// A storage of its own, with whatever scratch space it needs. Dropping it cleans up.
    ///
    /// `hint` names the scratch space; it is unique per test so tests can run at once.
    fn open(hint: &str) -> impl std::future::Future<Output = Self> + Send;

    fn storage(&self) -> Arc<Self::Storage>;

    /// The whole router for this backend: the shared one, plus any routes it adds.
    fn router(module: Arc<AppModule<Self::Storage>>) -> Router;
}

/// A running router plus the storage behind it.
pub struct TestApp<B: TestBackend> {
    pub router: Router,
    pub module: Arc<AppModule<B::Storage>>,
    pub admin_token: String,
    /// Kept for its `Drop`: the backend owns the scratch storage.
    #[allow(dead_code)]
    backend: B,
}

impl<B: TestBackend> TestApp<B> {
    pub async fn new() -> Self {
        Self::with_notifier(Arc::new(NoopNotifier)).await
    }

    /// The same app, but with webhooks wired to `notifier`.
    pub async fn with_notifier(notifier: Arc<dyn Notifier>) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let backend = B::open(&format!("test_http_{id}")).await;
        let module = Arc::new(AppModule::new(
            backend.storage(),
            TokenIssuer::new(b"integration-test-secret", 1),
            notifier,
            // Long enough that a link minted during a test is never expired, and signed with
            // the same secret the tokens use, as a deployment does.
            PreviewLinkIssuer::new(b"integration-test-secret", 60),
            PasswordResetIssuer::new(b"integration-test-secret", 30),
        ));

        module
            .auth_service
            .bootstrap_admin(Some(ADMIN_EMAIL), Some(ADMIN_PASSWORD), None)
            .await
            .unwrap_or_else(|e| panic!("bootstrap admin: {e}"))
            .expect("the store was empty, so an administrator is created");

        let router = B::router(module.clone());
        let (status, body) = login(&router, ADMIN_EMAIL, ADMIN_PASSWORD).await;
        assert_eq!(status, StatusCode::OK, "admin login failed: {body}");
        let admin_token = body["token"].as_str().unwrap().to_string();

        TestApp {
            router,
            module,
            admin_token,
            backend,
        }
    }
}

pub async fn login(router: &Router, email: &str, password: &str) -> (StatusCode, Value) {
    let (status, body) = send(
        router,
        Method::POST,
        "/auth/login",
        None,
        Some(json!({ "username": email, "password": password })),
    )
    .await;
    (status, body)
}

pub async fn send(
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

pub async fn send_raw(
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

/// Like [`send_raw`], but also hands back the response headers (for `X-Total-Count`).
pub async fn send_with_headers(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = builder.body(Body::empty()).expect("failed to build request");

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router failed");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body")
        .to_vec();
    (status, headers, bytes)
}
