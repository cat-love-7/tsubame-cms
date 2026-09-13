// Exactly one storage backend may be selected: both provide `AppModule` and the image
// store, and they are not meant to be linked together.
#[cfg(all(feature = "on-premises", any(feature = "aws", feature = "gcp", feature = "azure")))]
compile_error!(
    "enable exactly one storage backend feature: `on-premises` (default) or `aws`"
);
#[cfg(all(feature = "aws", any(feature = "gcp", feature = "azure")))]
compile_error!(
    "enable exactly one storage backend feature: `on-premises` (default) or `aws`"
);
#[cfg(all(feature = "gcp", feature = "azure"))]
compile_error!("only one of the `gcp` / `azure` features may be enabled");

#[cfg(feature = "aws")]
compile_error!(
    "the `aws` backend (DynamoDB + S3) is not implemented yet; build with the default \
     `on-premises` feature"
);

mod app_module;
mod auth;
mod config;
mod http;
mod models;
mod preview_link;
mod repositories;
mod services;
mod signing;
mod webhook;

#[cfg(feature = "on-premises")]
mod on_premises;

#[tokio::main]
async fn main() {
    init_tracing();

    #[cfg(feature = "on-premises")]
    if let Err(error) = run_on_premises().await {
        // `eprintln!` as well as the log: a bind failure must be visible even when the
        // log filter is set to something quiet.
        tracing::error!("fatal: {error}");
        eprintln!("fatal: {error}");
        std::process::exit(1);
    }
}

/// Serve the CMS over HTTP with on-premises storage.
#[cfg(feature = "on-premises")]
async fn run_on_premises() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::from_env()?;

    if config.jwt_secret_is_ephemeral {
        tracing::warn!(
            "JWT_SECRET is not set: using a random secret for this process only, so issued \
             tokens stop working after a restart. Set JWT_SECRET (at least {} characters) \
             to make them stable.",
            config::MIN_JWT_SECRET_LEN
        );
    }

    let module = std::sync::Arc::new(on_premises::build_app_module(&config));
    bootstrap_admin(&module, &config)?;

    let router = http::router(module, http::cors_layer(&config.cors_allowed_origins));

    let addr = config.socket_addr()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");

    axum::serve(listener, router).await?;
    Ok(())
}

/// Create the initial administrator when the user store is empty.
///
/// The server refuses to start in that state without credentials, rather than coming up
/// with an unauthenticated CMS.
#[cfg(feature = "on-premises")]
fn bootstrap_admin(
    module: &app_module::AppModule<on_premises::repository::Repository>,
    config: &config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(user) = module.auth_service.bootstrap_admin(
        config.admin_email.as_deref(),
        config.admin_password.as_deref(),
    )? {
        tracing::info!("created the initial administrator account: {}", user.email);
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // Ignore the error when a subscriber is already installed (e.g. in tests).
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
