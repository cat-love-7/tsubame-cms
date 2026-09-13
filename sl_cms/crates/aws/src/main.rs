//! The AWS CMS entry point: DynamoDB and S3, running as a Lambda function.
//!
//! There is nothing to serve yet (the Lambda entry point is `doc/aws-plan.md` P3), so this
//! builds the composition root — which is also how a deployment's configuration is checked to
//! be usable — and reports what is missing rather than starting a listener nobody asked for.

use sl_cms_core::config::Config;

#[tokio::main]
async fn main() {
    init_tracing();

    let config = Config::from_env().unwrap_or_else(|error| fatal(error.into()));
    if let Err(error) = sl_cms_aws::run(&config).await {
        fatal(error);
    }
}

/// A startup failure has to be visible even when the log filter is set to something quiet.
fn fatal(error: Box<dyn std::error::Error>) -> ! {
    tracing::error!("fatal: {error}");
    eprintln!("fatal: {error}");
    std::process::exit(1);
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
