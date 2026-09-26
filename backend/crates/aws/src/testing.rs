//! What a test needs to drive this adapter against the emulators.
//!
//! Public because the contract suite is a crate of its own (`crates/tests`): the alternative is a
//! mock, and then nothing would be testing this adapter. `lib.rs` re-exports these, so the paths
//! callers already use keep working.

use super::*;
use tsubame_core::auth::provisioner::{AccountProvisioner, ProvisionedAccount};

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

/// A user pool that only remembers what it was asked.
///
/// There is no Cognito emulator to run against (`docs/aws-decisions.md`), so the contract suite needs
/// something that answers like the admin API and writes nothing anywhere. It is deliberately not a
/// clever fake: a call succeeds, and every call is kept, so a test can see that the provider was
/// asked and with what. What the SDK calls themselves do is read rather than run here.
#[derive(Default)]
pub struct InMemoryCognito {
    calls: std::sync::Mutex<Vec<String>>,
}

impl InMemoryCognito {
    /// Everything asked of this pool, oldest first.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl CognitoAdmin for InMemoryCognito {
    fn create_user<'a>(
        &'a self,
        username: &'a str,
        email: Option<&'a str>,
    ) -> AdminFuture<'a, ProvisionedAccount> {
        Box::pin(async move {
            self.record(format!("create {username} email={}", email.unwrap_or("-")));
            // What a pool answers with, so a record written from this has something a token could
            // resolve to. This double always makes the account, which is the case the tests that
            // use it are about.
            Ok(ProvisionedAccount {
                external_id: Some(format!("sub-of-{username}")),
                created: true,
            })
        })
    }

    fn delete_user<'a>(&'a self, username: &'a str) -> AdminFuture<'a, ()> {
        Box::pin(async move {
            self.record(format!("delete {username}"));
            Ok(())
        })
    }

    fn set_temporary_password<'a>(
        &'a self,
        username: &'a str,
        password: &'a str,
    ) -> AdminFuture<'a, ()> {
        Box::pin(async move {
            self.record(format!("temporary {username} {password}"));
            Ok(())
        })
    }

    fn set_enabled<'a>(&'a self, username: &'a str, enabled: bool) -> AdminFuture<'a, ()> {
        Box::pin(async move {
            self.record(format!("enabled {username} {enabled}"));
            Ok(())
        })
    }
}

/// The provisioner a local run or a test gets: the real mapping over a pool that is not there.
///
/// A deployment builds its own over the SDK client ([`super::build_deployed_module`]); this is what
/// keeps `POST /auth/users` and a password reset answering the AWS *shape* without a service to talk
/// to.
pub fn in_memory_provisioner() -> std::sync::Arc<dyn AccountProvisioner> {
    std::sync::Arc::new(CognitoAccountProvisioner::with_admin(std::sync::Arc::new(
        InMemoryCognito::default(),
    )))
}

/// A user with exactly the policy the deployment grants, and the credentials to sign as it.
///
/// The emulator's root credentials can do anything, so a code path that needs a permission the
/// deployment does not grant passed here and failed in the cloud - which is how a missing
/// `s3:GetObject` stayed invisible. Running the same code as a user with the deployment's policy
/// puts the same wall in front of it.
///
/// Two steps, because the emulator splits them: the account is created through the gateway's admin
/// API inside the container (`--iam-dir` has no identity policies, so a user starts with nothing),
/// and the deployment's policy is then attached to the bucket as a **bucket** policy naming that
/// user - the same statements, with the user as their `Principal` ([`crate::policy`]). Without the
/// statement an action is denied, which is what a deployment's implicit deny looks like.
///
/// The name is letters and digits only, because these are not only S3 credentials: a test hands
/// them to the DynamoDB client too, and **DynamoDB Local refuses an access key id with anything
/// else in it** (`cms-...` reads as "The Access Key ID or security token is invalid").
///
/// `None` when the emulator is not running at all, which is a test that skips rather than a test
/// that lies. Everything after that check is this test's own setup, and a setup step that fails is
/// a failure ([`run_in_emulator`] says why).
#[cfg(test)]
pub async fn user_with_the_deployment_policy(
    s3: &aws_sdk_s3::Client,
    bucket: &str,
    list_bucket: bool,
) -> Option<(String, String)> {
    if !crate::repository::emulator_reachable(&test_s3_endpoint()) {
        return None;
    }
    let user = format!("cms{}", uuid::Uuid::new_v4().simple());
    let secret = uuid::Uuid::new_v4().simple().to_string();
    // The admin API's own credentials are the gateway's root ones, and the `--role user` keeps the
    // account out of the admin API: what it may do is only ever what the bucket policy says.
    run_in_emulator(&format!(
        "set -e; versitygw admin --endpoint-url http://127.0.0.1:7071 -a {root} -s {password} create-user -a {user} -s {secret} --role user",
        root = test_access_key(),
        password = test_secret_key(),
    ));
    s3.put_bucket_policy()
        .bucket(bucket)
        .policy(crate::policy::deployment_bucket_policy(
            bucket,
            &user,
            list_bucket,
        ))
        .send()
        .await
        .unwrap_or_else(|e| {
            panic!(
                "the emulator refused the deployment's bucket policy: {}",
                crate::repository::describe(&e)
            )
        });
    Some((user, secret))
}

/// Run a shell line inside the emulator container, and fail loudly when it does not run.
///
/// Answering `None` for a failed command, the way an absent emulator is answered, hides the
/// failures that matter: a policy the gateway will not accept, a user it will not create, a command
/// whose syntax is wrong. The test then reports success having checked nothing, which is exactly
/// what happened to this helper (`cargo test` was green; the policy was never applied). The caller
/// has already established that there is an emulator; from there, a failure is a failure.
#[cfg(test)]
fn run_in_emulator(script: &str) {
    // The compose file is at the root of the crate's workspace (`backend/`), which is two levels up
    // from this crate. A path that does not resolve is a mistake in this line, not a missing
    // emulator.
    let compose = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docker-compose.yml");
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
        preview_site_url: None,
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
        preview_site_url: None,
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
        preview_site_url: None,
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
