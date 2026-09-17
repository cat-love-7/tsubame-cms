//! AWS storage adapter: DynamoDB (single table) for structured data, S3 for image bytes.
//!
//! The shape is settled in `doc/aws-plan.md` and `doc/aws-dynamodb-design.md`.
//!
//! The repository traits are async, which is what this adapter always wanted to be: there used
//! to be a bridge that ran each call on a runtime thread so synchronous traits could be served,
//! and it is gone (see `doc/aws-dynamodb-design.md` §7.1).
//!
//! What is here: the composition root, all five repository traits (collections, users, single
//! pages, composite fields, images), the DynamoDB helpers (keys, JSON records, atomic ids) and
//! the S3 upload target. Still to come: the Lambda entry point and Cognito.

// Until the Lambda entry point exists (`doc/aws-plan.md` P3) nothing in the server binary drives
// the adapter: `run` builds the composition root and reports that it has nowhere to serve it.
// The data path is therefore compiled but never called, which is what every "never used" in this
// module is about; saying so once here beats a wall of them.
#![cfg_attr(not(test), allow(dead_code))]

use aws_sdk_dynamodb::Client;

use sl_cms_core::app_module::AppModule;
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::config::Config;
use sl_cms_core::password_reset::PasswordResetIssuer;
use sl_cms_core::auth::cognito::{CognitoSettings, CognitoVerifier, HttpJwks};
use sl_cms_core::preview_link::PreviewLinkIssuer;

pub mod lambda;
pub mod repository;
pub mod settings;

pub use repository::AwsRepository;
pub use settings::{AwsSettings, ImageDelivery};

/// Where a sign-in that happened at the provider is finished, or `None` when this deployment
/// signs users in itself (there is nothing to exchange then).
///
/// The token endpoint lives on the pool's own domain, which is what the sign-in URL names, so the
/// deployment does not need to be told it twice.
fn cognito_login(settings: &AwsSettings) -> Option<sl_cms_core::http::cognito_login::TokenEndpoint> {
    let endpoint = sl_cms_core::http::cognito_login::token_endpoint(settings.login_url.as_deref()?)?;
    Some(sl_cms_core::http::cognito_login::TokenEndpoint {
        client_id: settings.client_id.clone(),
        endpoint,
    })
}

/// Credentials for a local emulator, or `None` to let the SDK's own chain find them.
///
/// A deployment has nothing to put here: on Lambda the chain resolves the execution role. An
/// emulator, unlike DynamoDB Local, verifies the signature, so the CMS has to be told the same
/// keys MinIO was started with.
///
/// **The endpoint decides, not the variable.** Lambda sets `AWS_ACCESS_KEY_ID`,
/// `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN` to the execution role's temporary credentials,
/// so treating those names as "an emulator is configured" would replace the role - with the same
/// key and secret but *without* the session token - and every request would then be signed for an
/// identity that does not exist. A session token in the environment is carried along for the same
/// reason: an emulator may be started with one, and dropping it would break that too.
fn emulator_credentials(settings: &AwsSettings) -> Option<aws_sdk_dynamodb::config::Credentials> {
    if settings.endpoint_url.is_none() && settings.s3_endpoint_url.is_none() {
        return None;
    }
    match (&settings.access_key_id, &settings.secret_access_key) {
        (Some(access_key_id), Some(secret_access_key)) => Some(
            aws_sdk_dynamodb::config::Credentials::new(
                access_key_id.clone(),
                secret_access_key.clone(),
                settings.session_token.clone(),
                None,
                "cms",
            ),
        ),
        _ => None,
    }
}

/// Build a DynamoDB client pointed at `settings`, which may be a local emulator.
async fn dynamodb_client(settings: &AwsSettings) -> Client {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_dynamodb::config::Region::new(settings.region.clone()))
        .endpoint_url(settings.endpoint_url());
    if let Some(credentials) = emulator_credentials(settings) {
        loader = loader.credentials_provider(credentials);
    }
    Client::new(&loader.load().await)
}

/// Build an S3 client pointed at `settings`, which may be a local emulator.
///
/// Path-style addressing is what MinIO serves; against real AWS the SDK's virtual-host style is
/// the default, so it is only switched on when an endpoint override is in play.
async fn s3_client(settings: &AwsSettings) -> aws_sdk_s3::Client {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new(settings.region.clone()));
    if let Some(credentials) = emulator_credentials(settings) {
        loader = loader.credentials_provider(credentials);
    }
    let endpoint = settings.s3_endpoint_url();
    if let Some(endpoint) = &endpoint {
        loader = loader.endpoint_url(endpoint);
    }
    let shared = loader.load().await;

    let mut builder = aws_sdk_s3::config::Builder::from(&shared);
    if endpoint.is_some() {
        builder = builder.force_path_style(true);
    }
    aws_sdk_s3::Client::from_conf(builder.build())
}

/// Build the AWS composition root described by `config`.
///
/// Same shape as the on-premises one, with one difference: this has to reach the network, so it
/// is `async` — the SDK clients are built from configuration the SDK loads.
/// Everything a module is built from, so the two composers below cannot drift in anything but
/// which verifier decides who the caller is.
struct ModuleParts {
    repository: std::sync::Arc<AwsRepository>,
    token_issuer: TokenIssuer,
    notifier: std::sync::Arc<dyn sl_cms_core::webhook::Notifier>,
    preview_links: PreviewLinkIssuer,
    password_resets: PasswordResetIssuer,
}

async fn module_parts(config: &Config, settings: &AwsSettings) -> ModuleParts {
    ModuleParts {
        repository: std::sync::Arc::new(AwsRepository::connect(settings).await),
        token_issuer: TokenIssuer::new(&config.jwt_secret, config.token_ttl_hours),
        notifier: sl_cms_core::webhook::build_notifier(
            config.webhook_urls.clone(),
            config.webhook_secret.clone(),
        ),
        // Same secret, different prefix for each kind of link, exactly as on premises: a reset
        // signature can never be replayed as a preview link or a token.
        preview_links: PreviewLinkIssuer::new(&config.jwt_secret, config.preview_link_ttl_minutes),
        password_resets: PasswordResetIssuer::new(
            &config.jwt_secret,
            config.password_reset_ttl_minutes,
        ),
    }
}

/// The module a local run drives: the CMS issues and checks its own tokens.
///
/// Used by `run_local` and by the adapter tests, which sign in with a token minted from
/// `JWT_SECRET`. A deployment does **not** use this (see [`build_deployed_module`]).
pub async fn build_app_module(
    config: &Config,
    settings: &AwsSettings,
) -> Result<AppModule<AwsRepository>, Box<dyn std::error::Error + Send + Sync>> {
    let parts = module_parts(config, settings).await;
    Ok(AppModule::new(
        parts.repository,
        parts.token_issuer,
        parts.notifier,
        parts.preview_links,
        parts.password_resets,
    ))
}

/// The module a deployment runs: whoever signs in is vouched for by the identity provider.
///
/// Deliberately not the same composition as a local run. The CMS's own verifier is HS256 under
/// `JWT_SECRET`, and that secret is also what signs password-reset and preview links in this very
/// deployment - so a Lambda that accepted self-issued tokens would mint an administrator's token
/// for anyone who learned it. Here the only tokens that count are the pool's, and
/// `BOOTSTRAP_ADMIN_USERNAMES` names the people who may become the first administrators by
/// signing in.
pub async fn build_deployed_module(
    config: &Config,
    settings: &AwsSettings,
) -> Result<AppModule<AwsRepository>, Box<dyn std::error::Error + Send + Sync>> {
    let parts = module_parts(config, settings).await;
    let verifier = std::sync::Arc::new(CognitoVerifier::new(
        cognito_settings(settings),
        HttpJwks::new(cognito_settings(settings).jwks_url()),
    ));
    Ok(AppModule::new_with_verifier(
        parts.repository,
        parts.token_issuer,
        parts.notifier,
        parts.preview_links,
        parts.password_resets,
        verifier,
        settings.bootstrap_admin_usernames.clone(),
    ))
}

/// What the verifier has to be told, from what the deployment was given.
fn cognito_settings(settings: &AwsSettings) -> CognitoSettings {
    CognitoSettings {
        region: settings.region.clone(),
        user_pool_id: settings.user_pool_id.clone(),
        client_id: settings.client_id.clone(),
    }
}

/// The whole HTTP surface for this backend, with nothing said about where users sign in.
///
/// Used by the adapter tests, which sign in with a token this crate mints itself.
pub fn build_router(
    module: std::sync::Arc<AppModule<AwsRepository>>,
    cors: tower_http::cors::CorsLayer,
) -> axum::Router {
    build_router_with(module, cors, None, None)
}

/// The same, told where users sign in and where a sign-in the provider started is finished.
///
/// Both come from the same settings: `login_url` is what the sign-in screen sends the browser to,
/// and the token endpoint is derived from it, so a deployment cannot advertise one page and
/// finish the sign-in at another.
pub fn build_router_with(
    module: std::sync::Arc<AppModule<AwsRepository>>,
    cors: tower_http::cors::CorsLayer,
    login_url: Option<String>,
    exchange: Option<sl_cms_core::http::cognito_login::TokenEndpoint>,
) -> axum::Router {
    // Sign-in belongs to Cognito here, so the password endpoints do not exist. They are
    // registered anyway, answering 501 with a sentence that says where to sign in: a client
    // that guessed the path learns something, and a 404 would only say "wrong URL".
    let message = "this deployment signs users in through Cognito, so the CMS does not handle \
                   passwords; GET /auth/capabilities says where to sign in";
    let extra_public = sl_cms_core::http::password_auth::unavailable_public(message)
        .merge(sl_cms_core::http::capabilities::routes(
            sl_cms_core::models::capabilities::Capabilities::aws(login_url.clone()),
        ))
        // Finishing a sign-in the provider started: the code it sent back becomes a session.
        .merge(
            exchange
                .map(sl_cms_core::http::cognito_login::routes)
                .unwrap_or_default(),
        )
        // The durable link to an image: the id resolves to wherever the bytes are now. Here that
        // is object storage, so this points at it rather than passing bytes through the API.
        .merge(
            axum::Router::new()
                .route("/images/by-id/{id}", axum::routing::get(redirect_to_image)),
        );
    let extra_protected = sl_cms_core::http::password_auth::unavailable_protected(message);

    sl_cms_core::http::router_with(module, cors, extra_public, extra_protected)
}

/// Send the reader to the object storage URL an image currently lives at.
///
/// The bytes are replaced under a new key, so what this points at can change; the answer must not
/// be cached, while the object it names may be.
async fn redirect_to_image(
    axum::extract::State(module): axum::extract::State<
        std::sync::Arc<sl_cms_core::app_module::AppModule<AwsRepository>>,
    >,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<axum::response::Response, sl_cms_core::models::error::HttpError> {
    use axum::response::IntoResponse;
    match module
        .image_service
        .image_url(sl_cms_core::models::image::ImageID::from_u64(id))
        .await?
    {
        Some(url) => Ok((
            [(
                axum::http::header::CACHE_CONTROL,
                "no-cache",
            )],
            axum::response::Redirect::temporary(&url),
        )
            .into_response()),
        None => Err(sl_cms_core::models::error::HttpError::NotFound(
            "Image not found",
        )),
    }
}

pub async fn run_lambda(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let settings = AwsSettings::from_env()?;
    tracing::info!(
        region = %settings.region,
        table = %settings.table,
        bucket = %settings.bucket,
        jwks = %settings.jwks_url(),
        "aws backend starting"
    );
    warn_about_the_environment(config, &settings);

    let module = std::sync::Arc::new(build_deployed_module(config, &settings).await?);
    let router = build_router_with(
        module,
        sl_cms_core::http::cors_layer(&config.cors_allowed_origins),
        settings.login_url.clone(),
        cognito_login(&settings),
    );

    lambda_http::run(tower::service_fn(move |request| {
        let router = router.clone();
        async move { lambda::dispatch(router, request).await }
    }))
    .await?;
    Ok(())
}

/// Serve this backend over plain HTTP instead, for driving it locally.
///
/// The deployment is Lambda, but the same adapter has to be reachable from a browser to be
/// tested end to end (and to be poked at while developing), which is what this is for.
pub async fn run_local(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let settings = AwsSettings::from_env()?;
    let repository = std::sync::Arc::new(AwsRepository::connect(&settings).await);
    if repository.ensure_table().await? {
        tracing::info!(table = %settings.table, "created the table for this local run");
    }
    // The same module as the Lambda entry point in everything but the verifier: a local run signs
    // in with a token minted from `JWT_SECRET` (there is no pool to talk to), while a deployment
    // takes the pool's tokens only. See `build_deployed_module`.
    let module = std::sync::Arc::new(build_app_module(config, &settings).await?);
    warn_about_the_environment(config, &settings);

    let router = build_router_with(
        module,
        sl_cms_core::http::cors_layer(&config.cors_allowed_origins),
        settings.login_url.clone(),
        cognito_login(&settings),
    );
    let addr = config.socket_addr()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}

/// Say the things a deployment is likely to have wrong before anything else fails.
fn warn_about_the_environment(config: &Config, settings: &AwsSettings) {
    if config.admin_username.is_some() || config.admin_password.is_some() {
        tracing::warn!(
            "ADMIN_USERNAME / ADMIN_PASSWORD are ignored by the aws backend; set \
             BOOTSTRAP_ADMIN_USERNAMES instead"
        );
    }
    if settings.bootstrap_admin_usernames.is_empty() {
        tracing::warn!(
            "BOOTSTRAP_ADMIN_USERNAMES is empty: nobody can provision the first administrator"
        );
    }
}

/// The endpoints the adapter tests talk to: `CMS_TEST_DYNAMODB_ENDPOINT` and
/// `CMS_TEST_S3_ENDPOINT`, or the emulators from `docker-compose.yml`.
pub fn test_endpoint() -> String {
    std::env::var("CMS_TEST_DYNAMODB_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:8000".to_string())
}

pub fn test_s3_endpoint() -> String {
    std::env::var("CMS_TEST_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string())
}

/// The credentials the emulators were started with (`docker-compose.yml`).
pub fn test_access_key() -> String {
    std::env::var("CMS_TEST_ACCESS_KEY_ID").unwrap_or_else(|_| "test".to_string())
}

pub fn test_secret_key() -> String {
    std::env::var("CMS_TEST_SECRET_ACCESS_KEY").unwrap_or_else(|_| "test-secret".to_string())
}

/// The S3 policy a deployment gives its function, as the emulator is told it.
///
/// A copy of `infra/lambda.tf`'s two statements. It is here so a test that depends on a permission
/// can say so out loud, and so that changing the deployment's policy is something the tests are
/// expected to follow - they are the only place the difference is visible.
pub fn deployment_s3_policy(bucket: &str, list_bucket: bool) -> String {
    let mut statements = vec![format!(
        r#"{{"Effect":"Allow","Action":["s3:GetObject","s3:PutObject","s3:DeleteObject"],"Resource":["arn:aws:s3:::{bucket}/*"]}}"#
    )];
    if list_bucket {
        statements.push(format!(
            r#"{{"Effect":"Allow","Action":["s3:ListBucket"],"Resource":["arn:aws:s3:::{bucket}"]}}"#
        ));
    }
    format!(
        r#"{{"Version":"2012-10-17","Statement":[{}]}}"#,
        statements.join(",")
    )
}

/// A MinIO user with exactly that policy, and the credentials to sign as it.
///
/// The emulator's root credentials can do anything, so a code path that needs a permission the
/// deployment does not grant passed here and failed in the cloud - which is how a missing
/// `s3:GetObject` stayed invisible. Running the same code as a user with the deployment's policy
/// puts the same wall in front of it.
///
/// The name is letters and digits only, because these are not only MinIO's credentials: a test
/// hands them to the DynamoDB client too, and **DynamoDB Local refuses an access key id with
/// anything else in it** (`cms-...` reads as "The Access Key ID or security token is invalid").
///
/// `None` when the emulator is not running at all, which is a test that skips rather than a test
/// that lies. Everything after that check is this test's own setup, and a setup step that fails is
/// a failure ([`run_in_emulator`] says why).
#[cfg(test)]
pub fn restricted_minio_user(bucket: &str, list_bucket: bool) -> Option<(String, String)> {
    if !crate::repository::emulator_reachable(&test_s3_endpoint()) {
        return None;
    }
    let user = format!("cms{}", uuid::Uuid::new_v4().simple());
    let secret = uuid::Uuid::new_v4().simple().to_string();
    let policy = deployment_s3_policy(bucket, list_bucket);
    // The JSON has no single quotes in it, so the shell can carry it as it is.
    let script = format!(
        "set -e;          mc alias set cms http://127.0.0.1:9000 {root} {password} >/dev/null;          printf '%s' '{policy}' > /tmp/{user}.json;          mc admin user add cms {user} {secret} >/dev/null;          mc admin policy create cms {user} /tmp/{user}.json >/dev/null;          mc admin policy attach cms {user} --user {user} >/dev/null",
        root = test_access_key(),
        password = test_secret_key(),
    );
    run_in_emulator(&script);
    Some((user, secret))
}

/// Run a shell line inside the emulator container, and fail loudly when it does not run.
///
/// Answering `None` for a failed command, the way an absent emulator is answered, hides the
/// failures that matter: a policy MinIO will not accept, a user it will not create, a command
/// whose syntax is wrong. The test then reports success having checked nothing, which is exactly
/// what happened to this helper (`cargo test` was green; the policy was never applied). The caller
/// has already established that there is an emulator; from there, a failure is a failure.
#[cfg(test)]
fn run_in_emulator(script: &str) {
    // The compose file is at the root of the crate's workspace (`sl_cms/`), which is two levels up
    // from this crate. A path that does not resolve is a mistake in this line, not a missing
    // emulator.
    let compose =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docker-compose.yml");
    assert!(
        compose.is_file(),
        "the compose file should be at {}",
        compose.display()
    );
    let output = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            compose.to_str().expect("a compose path that is not UTF-8"),
            "exec",
            "-T",
            "s3",
            "sh",
            "-c",
            script,
        ])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "could not run docker against the emulator at {}: {e}",
                compose.display()
            )
        });
    assert!(
        output.status.success(),
        "the emulator refused the setup: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository over a fresh table, serving images the way the given mode says.
///
/// What a test needs to see both deployments: the same CMS, one with a readable bucket and one
/// where every URL is a signature.
pub async fn open_test_repository_serving(
    table_hint: &str,
    image_delivery: ImageDelivery,
) -> Result<(std::sync::Arc<AwsRepository>, String), Box<dyn std::error::Error + Send + Sync>> {
    let table = format!("{table_hint}_{}", uuid::Uuid::new_v4().simple());
    let settings = AwsSettings {
        region: "us-east-1".to_string(),
        table: table.clone(),
        bucket: format!("cms-test-{}", uuid::Uuid::new_v4().simple()),
        user_pool_id: "unused_pool".to_string(),
        client_id: "unused_client".to_string(),
        login_url: None,
        endpoint_url: Some(test_endpoint()),
        s3_endpoint_url: Some(test_s3_endpoint()),
        image_base_url: None,
        image_delivery,
        access_key_id: Some(test_access_key()),
        secret_access_key: Some(test_secret_key()),
        session_token: None,
        bootstrap_admin_usernames: Vec::new(),
    };
    let repository = std::sync::Arc::new(AwsRepository::connect(&settings).await);
    repository.create_table().await?;
    Ok((repository, table))
}

/// A repository over a fresh table that talks to `bucket` as the given credentials.
///
/// What a test needs to run as a restricted user: the bucket is made by the caller (with the
/// emulator's root credentials), and everything else is an ordinary local repository.
pub async fn open_test_repository_as(
    table_hint: &str,
    bucket: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<(std::sync::Arc<AwsRepository>, String), Box<dyn std::error::Error + Send + Sync>> {
    let table = format!("{table_hint}_{}", uuid::Uuid::new_v4().simple());
    let settings = AwsSettings {
        region: "us-east-1".to_string(),
        table: table.clone(),
        bucket: bucket.to_string(),
        user_pool_id: "unused_pool".to_string(),
        client_id: "unused_client".to_string(),
        login_url: None,
        endpoint_url: Some(test_endpoint()),
        s3_endpoint_url: Some(test_s3_endpoint()),
        image_base_url: None,
        image_delivery: ImageDelivery::stable(),
        access_key_id: Some(access_key_id.to_string()),
        secret_access_key: Some(secret_access_key.to_string()),
        session_token: None,
        bootstrap_admin_usernames: Vec::new(),
    };
    let repository = std::sync::Arc::new(AwsRepository::connect(&settings).await);
    repository.create_table().await?;
    Ok((repository, table))
}

/// A repository over a freshly created table, for tests against a local DynamoDB.
///
/// The table name is unique per call so tests do not share state; the caller drops it when it
/// is done. This is public because the contract suite is a crate of its own — the alternative
/// is a mock, and then nothing would be testing this adapter.
pub async fn open_test_repository(
    table_hint: &str,
) -> Result<(std::sync::Arc<AwsRepository>, String), Box<dyn std::error::Error + Send + Sync>> {
    let table = format!("{table_hint}_{}", uuid::Uuid::new_v4().simple());
    let settings = AwsSettings {
        region: "us-east-1".to_string(),
        table: table.clone(),
        bucket: format!("cms-test-{}", uuid::Uuid::new_v4().simple()),
        user_pool_id: "unused_pool".to_string(),
        client_id: "unused_client".to_string(),
        login_url: None,
        endpoint_url: Some(test_endpoint()),
        s3_endpoint_url: Some(test_s3_endpoint()),
        image_base_url: None,
        image_delivery: ImageDelivery::stable(),
        // The emulator from docker-compose.yml checks the signature, so these have to match it.
        access_key_id: Some(test_access_key()),
        secret_access_key: Some(test_secret_key()),
        session_token: None,
        bootstrap_admin_usernames: Vec::new(),
    };
    let repository = std::sync::Arc::new(AwsRepository::connect(&settings).await);
    repository.create_table().await?;
    Ok((repository, table))
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    /// The fixture is the deployment's policy: the statements the function is granted, and nothing
    /// else. A test that depends on a permission reads the deployment, not this file.
    #[test]
    fn the_emulator_policy_is_the_deployments() {
        let allowed = deployment_s3_policy("cms-images", true);
        assert!(allowed.contains(r#""s3:GetObject","s3:PutObject","s3:DeleteObject""#));
        assert!(allowed.contains(r#""arn:aws:s3:::cms-images/*""#));
        assert!(allowed.contains(r#""s3:ListBucket""#));
        assert!(allowed.contains(r#""arn:aws:s3:::cms-images""#));

        // The other half of the puzzle: what the tests are like without it.
        let denied = deployment_s3_policy("cms-images", false);
        assert!(!denied.contains("ListBucket"));
        assert!(denied.contains("s3:GetObject"));
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    fn settings(endpoint: Option<&str>) -> AwsSettings {
        AwsSettings {
            region: "us-east-1".to_string(),
            table: "cms".to_string(),
            bucket: "cms-images".to_string(),
            user_pool_id: "pool".to_string(),
            client_id: "client".to_string(),
            login_url: None,
            endpoint_url: endpoint.map(str::to_string),
            s3_endpoint_url: None,
            image_base_url: None,
            image_delivery: ImageDelivery::stable(),
            access_key_id: Some("AKIA-FROM-THE-ENVIRONMENT".to_string()),
            secret_access_key: Some("secret".to_string()),
            session_token: Some("session".to_string()),
            bootstrap_admin_usernames: Vec::new(),
        }
    }

    /// The variables exist in a deployment too, holding the execution role's credentials, so the
    /// endpoint is what says "this is an emulator". Reading them as `Some` was how the role's
    /// session token came to be dropped, which signs for an identity that does not exist.
    #[test]
    fn explicit_credentials_are_only_for_an_endpoint_override() {
        assert!(emulator_credentials(&settings(None)).is_none());

        let credentials = emulator_credentials(&settings(Some("http://localhost:8000")))
            .expect("an emulator is configured");
        assert_eq!(credentials.access_key_id(), "AKIA-FROM-THE-ENVIRONMENT");
        assert_eq!(
            credentials.session_token(),
            Some("session"),
            "the session token travels with the credentials"
        );
    }

    /// An emulator started without a session token still works: the SDK wants `None`, not an
    /// empty string.
    #[test]
    fn credentials_without_a_session_token_are_plain() {
        let mut settings = settings(Some("http://localhost:8000"));
        settings.session_token = None;
        let credentials = emulator_credentials(&settings).expect("an emulator is configured");
        assert_eq!(credentials.session_token(), None);
    }
}

#[cfg(test)]
mod deployed_verifier_tests {
    use super::*;
    use sl_cms_core::auth::token::TokenIssuer;
    use sl_cms_core::models::user::{Permission, User};

    fn emulator_settings() -> AwsSettings {
        AwsSettings {
            region: "eu-west-1".to_string(),
            table: "cms".to_string(),
            bucket: "cms-images".to_string(),
            user_pool_id: "eu-west-1_abc".to_string(),
            client_id: "client-1".to_string(),
            login_url: None,
            endpoint_url: Some("http://localhost:8000".to_string()),
            s3_endpoint_url: None,
            image_base_url: None,
            image_delivery: ImageDelivery::stable(),
            access_key_id: Some("test".to_string()),
            secret_access_key: Some("test-secret".to_string()),
            session_token: None,
            bootstrap_admin_usernames: vec!["ops".to_string()],
        }
    }

    /// The deployment's tokens are the pool's. The CMS's own HS256 tokens are signed with the same
    /// secret that signs reset and preview links in that deployment, so accepting them here would
    /// hand an administrator's token to anyone holding it - which is why the Lambda composition is
    /// not the local one, and why this test exists rather than a comment.
    #[tokio::test]
    async fn the_deployed_module_does_not_accept_the_cms_own_tokens() {
        let config = Config::default();
        let settings = emulator_settings();
        let module = build_deployed_module(&config, &settings)
            .await
            .expect("the module builds without talking to anything");

        let (own_token, _) = TokenIssuer::new(&config.jwt_secret, 1)
            .issue(&User::new("ops", true, Permission::admin()))
            .expect("a token");
        let refusal = module
            .auth_service
            .user_from_token(&own_token)
            .await
            .expect_err("the deployment takes the pool's tokens, not the CMS's own");
        assert_eq!(refusal.status_code, 401);
    }

    /// The verifier is pointed at the deployment's own pool: a mismatch here would be a deployment
    /// that rejects every genuine token.
    #[test]
    fn the_verifier_is_told_which_pool_this_is() {
        let settings = emulator_settings();
        let cognito = cognito_settings(&settings);

        assert_eq!(
            cognito.issuer(),
            "https://cognito-idp.eu-west-1.amazonaws.com/eu-west-1_abc"
        );
        assert_eq!(cognito.client_id, "client-1");
        assert_eq!(cognito.jwks_url(), settings.jwks_url());
    }
}
