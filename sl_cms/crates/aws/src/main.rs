//! The AWS CMS binary: a Lambda function, or a local HTTP server for driving it by hand.
//!
//! The runtime sets `AWS_LAMBDA_RUNTIME_API` for a function; anything else means someone ran the
//! binary themselves, which is how the browser end-to-end suite reaches this backend.

use sl_cms_core::config::Config;

#[tokio::main]
async fn main() {
    init_tracing();
    // The process has to pick a rustls crypto provider before any HTTPS client is built (see
    // `sl_cms_core::webhook::install_crypto_provider`); saying it here makes the requirement
    // visible at the entry point rather than hidden in whichever client happens to be built first.
    sl_cms_core::webhook::install_crypto_provider();

    let config = Config::from_env().unwrap_or_else(|error| fatal(error.into()));

    let running_on_lambda = std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok();
    let result = if running_on_lambda {
        sl_cms_aws::run_lambda(&config).await
    } else {
        sl_cms_aws::run_local(&config).await
    };
    if let Err(error) = result {
        fatal(error);
    }
}

/// A startup failure has to be visible even when the log filter is set to something quiet.
fn fatal(error: Box<dyn std::error::Error + Send + Sync>) -> ! {
    tracing::error!("fatal: {error}");
    eprintln!("fatal: {error}");
    std::process::exit(1);
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
