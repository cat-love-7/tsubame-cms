//! The on-premises CMS server: rkv (LMDB) storage and image files on disk.
//!
//! Everything shared is in `sl-cms-core`; this binary is the composition root — read the
//! configuration, build the module, add the routes only this backend has, and serve.

use std::sync::Arc;

use sl_cms_core::config::{self, Config};
use sl_cms_core::http;

#[tokio::main]
async fn main() {
    init_tracing();

    if let Err(error) = run().await {
        fatal(error);
    }
}

/// A startup failure has to be visible even when the log filter is set to something quiet.
fn fatal(error: Box<dyn std::error::Error>) {
    tracing::error!("fatal: {error}");
    eprintln!("fatal: {error}");
    std::process::exit(1);
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    if config.jwt_secret_is_ephemeral {
        tracing::warn!(
            "JWT_SECRET is not set: using a random secret for this process only, so issued \
             tokens stop working after a restart. Set JWT_SECRET (at least {} characters) \
             to make them stable.",
            config::MIN_JWT_SECRET_LEN
        );
    }

    let module = Arc::new(sl_cms_on_premises::build_app_module(&config));
    bootstrap_admin(&module, &config)?;

    // The shared router, plus the routes for serving and accepting image bytes: this backend
    // keeps them itself, so they are part of what it composes rather than of what every
    // backend has.
    let router = sl_cms_on_premises::build_router(
        module,
        http::cors_layer(&config.cors_allowed_origins),
    );

    let addr = config.socket_addr()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");

    axum::serve(listener, router).await?;
    Ok(())
}

/// Create the initial administrator when the user store is empty.
///
/// The server refuses to start in that state without credentials, rather than coming up with
/// an unauthenticated CMS.
fn bootstrap_admin(
    module: &sl_cms_core::app_module::AppModule<sl_cms_on_premises::repository::Repository>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(user) = module.auth_service.bootstrap_admin(
        config.admin_username.as_deref(),
        config.admin_password.as_deref(),
        config.admin_email.as_deref(),
    )? {
        tracing::info!("created the initial administrator account: {}", user.username);
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // Ignore the error when a subscriber is already installed (e.g. in tests).
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
