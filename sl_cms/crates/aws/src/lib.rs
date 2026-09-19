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
use sl_cms_core::auth::cognito::{CognitoSettings, CognitoVerifier, HttpJwks};
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::config::Config;
use sl_cms_core::password_reset::PasswordResetIssuer;
use sl_cms_core::preview_link::PreviewLinkIssuer;

pub mod lambda;
pub mod repository;
pub mod settings;

mod credentials;
mod policy;
mod testing;

use credentials::emulator_credentials;

pub use policy::*;
pub use repository::AwsRepository;
pub use settings::{AwsSettings, ImageDelivery};
pub use testing::*;

/// Where a sign-in that happened at the provider is finished, or `None` when this deployment
/// signs users in itself (there is nothing to exchange then).
///
/// The token endpoint lives on the pool's own domain, which is what the sign-in URL names, so the
/// deployment does not need to be told it twice.
fn cognito_login(
    settings: &AwsSettings,
) -> Option<sl_cms_core::http::cognito_login::TokenEndpoint> {
    let endpoint =
        sl_cms_core::http::cognito_login::token_endpoint(settings.login_url.as_deref()?)?;
    Some(sl_cms_core::http::cognito_login::TokenEndpoint::new(
        settings.client_id.clone(),
        endpoint,
    ))
}

/// Build a DynamoDB client pointed at `settings`, which may be a local emulator.
async fn dynamodb_client(settings: &AwsSettings) -> Client {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_dynamodb::config::Region::new(
            settings.region.clone(),
        ))
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
        config.limits,
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
        config.limits,
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
            sl_cms_core::models::capabilities::Capabilities::aws(
                login_url.clone(),
                module.limits.max_image_bytes,
            ),
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
            axum::Router::new().route("/images/by-id/{id}", axum::routing::get(redirect_to_image)),
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
        .image_url(sl_cms_core::models::image::ImageId::from_u64(id))
        .await?
    {
        Some(url) => Ok((
            [(axum::http::header::CACHE_CONTROL, "no-cache")],
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

/// Where a deployment keeps the signing secret: Secrets Manager, when it says so.
///
/// `JWT_SECRET` still wins - a deployment that sets both asked for the environment variable, and
/// a local run has neither. An ARN is what makes the process fetch one, and the fetch is what
/// keeps the secret out of the Lambda configuration and out of Terraform's state.
///
/// Split out from the call so the decision is testable without reaching AWS.
fn secret_arn_to_fetch<'a>(has_env_secret: bool, arn: Option<&'a str>) -> Option<&'a str> {
    if has_env_secret {
        return None;
    }
    arn.map(str::trim).filter(|arn| !arn.is_empty())
}

/// Read the signing secret from Secrets Manager when the deployment names one there.
///
/// Called before anything is built, so a secret that cannot be read stops the process rather than
/// leaving it to mint tokens with an ephemeral key that nobody else knows (see
/// `Config::jwt_secret_is_ephemeral`).
pub async fn resolve_jwt_secret(
    config: &mut sl_cms_core::config::Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    use sl_cms_core::config::MIN_JWT_SECRET_LEN;

    let arn = std::env::var("JWT_SECRET_ARN").ok();
    let Some(arn) = secret_arn_to_fetch(!config.jwt_secret_is_ephemeral, arn.as_deref()) else {
        return Ok(());
    };

    // Region and credentials come from the environment, as they do for the execution role.
    let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let answer = aws_sdk_secretsmanager::Client::new(&shared)
        .get_secret_value()
        .secret_id(arn)
        .send()
        .await
        .map_err(|e| {
            format!(
                "could not read the signing secret from Secrets Manager: {}",
                crate::repository::describe(&e)
            )
        })?;

    let Some(secret) = answer.secret_string() else {
        return Err("the signing secret has no string value".into());
    };
    if secret.len() < MIN_JWT_SECRET_LEN {
        return Err(format!(
            "the signing secret in Secrets Manager is {} characters; {MIN_JWT_SECRET_LEN} is the minimum",
            secret.len()
        )
        .into());
    }

    config.jwt_secret = secret.as_bytes().to_vec();
    config.jwt_secret_is_ephemeral = false;
    tracing::info!("the signing key was read from Secrets Manager");
    Ok(())
}

#[cfg(test)]
mod secret_tests {
    use super::secret_arn_to_fetch;

    /// A secret is fetched only when the deployment asked for one and did not set the variable.
    #[test]
    fn a_secret_is_fetched_only_when_the_deployment_names_one() {
        let arn = "arn:aws:secretsmanager:eu-west-1:1:secret:sl-cms/jwt-AbCdEf";
        assert_eq!(secret_arn_to_fetch(false, Some(arn)), Some(arn));
        assert_eq!(
            secret_arn_to_fetch(false, Some("   ")),
            None,
            "blank is not an ARN"
        );
        assert_eq!(
            secret_arn_to_fetch(false, None),
            None,
            "a local run has neither"
        );
        assert_eq!(
            secret_arn_to_fetch(true, Some(arn)),
            None,
            "JWT_SECRET was set: that is the one the deployment asked for"
        );
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
