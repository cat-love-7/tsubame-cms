//! AWS storage adapter: DynamoDB (single table) for structured data, S3 for image bytes.
//!
//! Not written yet. The shape is settled, and what is left is the work itself:
//!
//! * a `Repository` over one DynamoDB table (`Repository: Storage`), with the published copy,
//!   the working copy and the metadata as three records per item, an atomic counter for item
//!   ids, and `TransactWriteItems` for the publish that copies one to the other;
//! * S3 behind [`crate::repositories::image_repository::ImageRepository`]: a presigned PUT as
//!   the upload target, the object's own URL stored in the content (a *stable* URL - never a
//!   presigned one, which would rot in the content), and no local byte serving at all;
//! * [`build_app_module`] as the composition root, exactly like `on_premises::build_app_module`;
//! * a Lambda entry point that hands the shared router to `lambda_http`.
//!
//! Until that exists, selecting this backend is refused here rather than failing somewhere
//! less obvious. The plan, including what has to be decided first, is in `doc/aws-plan.md`.

compile_error!(
    "the `aws` backend (DynamoDB + S3) is not implemented yet; build with the default \
     `on-premises` feature"
);

use crate::config::Config;

/// Start the CMS on AWS.
///
/// Unreachable today - the `compile_error!` above refuses the build - but it is the shape the
/// rest of the work fills in: check the settings, build the DynamoDB + S3 composition root, and
/// hand the shared router to `lambda_http`. Writing it down here keeps the "what is missing"
/// honest and gives the configuration somewhere real to be used from.
#[allow(dead_code)]
pub async fn run(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let settings = config.aws_settings()?;
    tracing::info!(
        region = %settings.region,
        table = %settings.table,
        bucket = %settings.bucket,
        jwks = %settings.jwks_url(),
        "aws backend starting"
    );

    // The on-premises administrator settings have no effect here, which is worth saying out
    // loud rather than leaving an operator to wonder why the account never appears.
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

    Err("the aws backend is not implemented yet".into())
}
