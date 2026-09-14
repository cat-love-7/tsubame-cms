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
use sl_cms_core::preview_link::PreviewLinkIssuer;

pub mod lambda;
pub mod repository;
pub mod settings;

pub use repository::AwsRepository;
pub use settings::AwsSettings;

/// Credentials for a local emulator, or `None` to let the SDK's own chain find them.
///
/// A deployment has nothing to put here: on Lambda the chain resolves the execution role. An
/// emulator, unlike DynamoDB Local, verifies the signature, so the CMS has to be told the same
/// keys MinIO was started with.
fn emulator_credentials(settings: &AwsSettings) -> Option<aws_sdk_dynamodb::config::Credentials> {
    match (&settings.access_key_id, &settings.secret_access_key) {
        (Some(access_key_id), Some(secret_access_key)) => Some(
            aws_sdk_dynamodb::config::Credentials::new(
                access_key_id.clone(),
                secret_access_key.clone(),
                None,
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
pub async fn build_app_module(
    config: &Config,
    settings: &AwsSettings,
) -> Result<AppModule<AwsRepository>, Box<dyn std::error::Error + Send + Sync>> {
    let repository = std::sync::Arc::new(AwsRepository::connect(settings).await);
    let token_issuer = TokenIssuer::new(&config.jwt_secret, config.token_ttl_hours);
    let notifier = sl_cms_core::webhook::build_notifier(
        config.webhook_urls.clone(),
        config.webhook_secret.clone(),
    );
    // Same secret, different prefix for each kind of link, exactly as on premises: a reset
    // signature can never be replayed as a preview link or a token.
    let preview_links =
        PreviewLinkIssuer::new(&config.jwt_secret, config.preview_link_ttl_minutes);
    let password_resets =
        PasswordResetIssuer::new(&config.jwt_secret, config.password_reset_ttl_minutes);
    Ok(AppModule::new(
        repository,
        token_issuer,
        notifier,
        preview_links,
        password_resets,
    ))
}

/// The whole HTTP surface for this backend.
///
/// Identical to the shared router: image bytes are the object store's business, so this
/// backend adds no routes of its own.
pub fn build_router(
    module: std::sync::Arc<AppModule<AwsRepository>>,
    cors: tower_http::cors::CorsLayer,
) -> axum::Router {
    build_router_with_login(module, cors, None)
}

/// The same, saying where users sign in when the deployment knows its provider's page.
pub fn build_router_with_login(
    module: std::sync::Arc<AppModule<AwsRepository>>,
    cors: tower_http::cors::CorsLayer,
    login_url: Option<String>,
) -> axum::Router {
    // Sign-in belongs to Cognito here, so the password endpoints do not exist. They are
    // registered anyway, answering 501 with a sentence that says where to sign in: a client
    // that guessed the path learns something, and a 404 would only say "wrong URL".
    let message = "this deployment signs users in through Cognito, so the CMS does not handle \
                   passwords; GET /auth/capabilities says where to sign in";
    let extra_public = sl_cms_core::http::password_auth::unavailable_public(message).merge(
        sl_cms_core::http::capabilities::routes(
            sl_cms_core::models::capabilities::Capabilities::aws(login_url),
        ),
    );
    let extra_protected = sl_cms_core::http::password_auth::unavailable_protected(message);

    sl_cms_core::http::router_with(module, cors, extra_public, extra_protected)
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

    let module = std::sync::Arc::new(build_app_module(config, &settings).await?);
    let router = build_router_with_login(
        module,
        sl_cms_core::http::cors_layer(&config.cors_allowed_origins),
        settings.login_url.clone(),
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
    // The same composition as the Lambda entry point, so what a developer drives locally is what
    // runs on AWS — including which verifier decides who the caller is.
    let module = std::sync::Arc::new(build_app_module(config, &settings).await?);
    warn_about_the_environment(config, &settings);

    let router = build_router(
        module,
        sl_cms_core::http::cors_layer(&config.cors_allowed_origins),
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
        // The emulator from docker-compose.yml checks the signature, so these have to match it.
        access_key_id: Some(test_access_key()),
        secret_access_key: Some(test_secret_key()),
        bootstrap_admin_usernames: Vec::new(),
    };
    let repository = std::sync::Arc::new(AwsRepository::connect(&settings).await);
    repository.create_table().await?;
    Ok((repository, table))
}
