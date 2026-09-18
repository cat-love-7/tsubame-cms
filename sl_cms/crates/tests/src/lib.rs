//! The HTTP contract suite, and the harness that runs it against a storage backend.
//!
//! The suite itself is `suite/` at the root of this package: one file per topic, with `suite/mod.rs`
//! holding the harness. It is compiled once per backend by the files in `tests/`, each of which
//! defines which backend it is and then pulls the directory in as a module:
//!
//! ```text
//! tests/on_premises.rs -> type Backend = OnPremises; #[path = "../suite/mod.rs"] mod contract;
//! tests/aws.rs         -> type Backend = Aws;        #[path = "../suite/mod.rs"] mod contract;
//! ```
//!
//! `suite/mod.rs` rather than `suite.rs` beside a `suite/` directory is deliberate, and the one
//! place in the workspace where the `mod.rs` shape is right: a module pulled in with `#[path]` does
//! not get a directory named after the file, so `mod collections;` in `suite.rs` would be looked
//! for beside the runner rather than in `suite/`. Everywhere else a directory module is
//! `<dir>.rs` next to `<dir>/` (`core/src/auth.rs`, `repository.rs`).
//!
//! That is the reason this package exists. The suite is the contract, so a backend that passes
//! it is interchangeable with one that does — and one command (`cargo test -p sl-cms-tests`)
//! runs it against both, which is something a single crate selecting a backend with a Cargo
//! feature cannot do, because one build has one feature set.
//!
//! The AWS suite needs DynamoDB Local and MinIO from `sl_cms/docker-compose.yml`; it says so
//! loudly rather than passing quietly when they are missing. `scripts/test-rust.sh` checks for
//! them first and skips the whole file with a note.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use sl_cms_core::app_module::{AppModule, Storage};
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::models::user::{Permission, User};
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

    /// Whether the CMS itself verifies passwords.
    ///
    /// The on-premises deployment does, and the tests about signing in, reset links and
    /// changing a password are its tests. Where an identity provider signs users in there is no
    /// password endpoint at all, so those tests have nothing to drive.
    const PASSWORD_LOGIN: bool;

    /// A storage of its own, with whatever scratch space it needs. Dropping it cleans up.
    ///
    /// `hint` names the scratch space; it is unique per test so tests can run at once.
    fn open(hint: &str) -> impl std::future::Future<Output = Self> + Send;

    fn storage(&self) -> Arc<Self::Storage>;

    /// The whole router for this backend: the shared one, plus any routes it adds.
    fn router(module: Arc<AppModule<Self::Storage>>) -> Router;

    /// Create the administrator the suite uses and mint the token it signs in with.
    ///
    /// Where the CMS checks passwords this really does bootstrap an account and POST to
    /// `/auth/login`, so every test that needs a token also exercises signing in. Where an
    /// identity provider does it, there is no endpoint to post to: the account record is
    /// written and the token minted, which is what a deployment does once the provider has
    /// authenticated someone.
    fn sign_in_admin(
        &self,
        module: &Arc<AppModule<Self::Storage>>,
    ) -> impl std::future::Future<Output = String> + Send;

    /// The account record for `username`, for a suite helper that has to answer "who am I" the
    /// way a real sign-in would where there is no sign-in endpoint to ask.
    fn user_for(&self, username: &str) -> impl std::future::Future<Output = User> + Send;

    /// Write an account record, the way this deployment creates one.
    ///
    /// The credential is not part of it: on-premises sets the password through the endpoint the
    /// suite calls, and AWS leaves it to Cognito.
    fn create_account(
        &self,
        username: &str,
        is_admin: bool,
        permission: Permission,
    ) -> impl std::future::Future<Output = Result<User, String>> + Send;
}

/// The token issuer every test deployment is built with.
///
/// The harness and the backends have to agree on the secret for a minted token to verify.
pub const TEST_SECRET: &[u8] = b"integration-test-secret";
pub const TEST_TOKEN_TTL_HOURS: i64 = 1;

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
            TokenIssuer::new(TEST_SECRET, TEST_TOKEN_TTL_HOURS),
            notifier,
            // Long enough that a link minted during a test is never expired, and signed with
            // the same secret the tokens use, as a deployment does.
            PreviewLinkIssuer::new(TEST_SECRET, 60),
            PasswordResetIssuer::new(TEST_SECRET, 30),
        ));

        let admin_token = backend.sign_in_admin(&module).await;
        let router = B::router(module.clone());

        TestApp {
            router,
            module,
            admin_token,
            backend,
        }
    }

    /// Whether the password endpoints exist in this deployment.
    pub fn password_login(&self) -> bool {
        B::PASSWORD_LOGIN
    }

    /// The backend, for the few things a test has to do without the HTTP layer (writing an
    /// account record where there is no endpoint that would).
    pub fn backend(&self) -> &B {
        &self.backend
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
        None => builder
            .body(Body::empty())
            .expect("failed to build request"),
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
    let request = builder
        .body(Body::empty())
        .expect("failed to build request");

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
