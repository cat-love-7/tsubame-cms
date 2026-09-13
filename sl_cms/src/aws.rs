//! AWS storage adapter: DynamoDB (single table) for structured data, S3 for image bytes.
//!
//! The shape is settled in `doc/aws-plan.md` and `doc/aws-dynamodb-design.md`; this module is
//! being filled in. The repository traits are synchronous and the AWS SDK is not, so the
//! implementation is written `async` (what it wants to be) and the synchronous traits are
//! served through [`bridge::BlockingRuntime`]. That bridge is temporary: when the traits
//! themselves become async, it and the short wrappers go away and the async implementation
//! stays.
//!
//! What is here: the bridge, the DynamoDB helpers (keys, JSON records, atomic ids) and
//! `CollectionRepository`, verified against DynamoDB Local. Still to come: the other four
//! traits (users, single pages, composite fields, images), the S3 upload target, the
//! composition root (`build_app_module`, which needs all five traits) and the Lambda entry
//! point.

use aws_sdk_dynamodb::Client;

use crate::config::{AwsSettings, Config};

pub mod bridge;
pub mod repository;

#[cfg_attr(not(test), allow(unused_imports))] // used by `build_app_module`, which needs all five traits
pub use repository::AwsRepository;

/// Build a DynamoDB client pointed at `settings`, which may be a local emulator.
async fn dynamodb_client(settings: &AwsSettings) -> Client {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_dynamodb::config::Region::new(settings.region.clone()))
        .credentials_provider(aws_sdk_dynamodb::config::Credentials::new(
            "test", "test", None, None, "cms",
        ))
        .endpoint_url(settings.endpoint_url())
        .load()
        .await;
    Client::new(&config)
}

/// Start the CMS on AWS.
///
/// The storage adapter is being built; the Lambda entry point (`lambda_http`) is the last step
/// of `doc/aws-plan.md`'s P3, so this reports that rather than pretending to serve.
pub async fn run(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let settings = config.aws_settings()?;
    tracing::info!(
        region = %settings.region,
        table = %settings.table,
        bucket = %settings.bucket,
        jwks = %settings.jwks_url(),
        "aws backend starting"
    );

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

/// The endpoint the adapter tests talk to: `CMS_TEST_DYNAMODB_ENDPOINT`, or the emulator from
/// `docker-compose.yml`.
#[cfg(test)]
pub fn test_endpoint() -> String {
    std::env::var("CMS_TEST_DYNAMODB_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:8000".to_string())
}

/// A repository over a freshly created table, for tests against a local DynamoDB.
///
/// The table name is unique per call so tests do not share state; the caller drops it when it
/// is done.
#[cfg(test)]
pub async fn open_test_repository(table_hint: &str) -> (std::sync::Arc<AwsRepository>, String) {
    let table = format!("{table_hint}_{}", uuid::Uuid::new_v4().simple());
    let settings = AwsSettings {
        region: "us-east-1".to_string(),
        table: table.clone(),
        bucket: "unused-bucket".to_string(),
        user_pool_id: "unused_pool".to_string(),
        endpoint_url: Some(test_endpoint()),
        bootstrap_admin_usernames: Vec::new(),
    };
    let client = dynamodb_client(&settings).await;
    let repository = std::sync::Arc::new(AwsRepository::new(client, table.clone()));
    repository
        .create_table()
        .await
        .expect("could not create the test table");
    (repository, table)
}
