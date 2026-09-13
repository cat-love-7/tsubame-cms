//! Process configuration, read from the environment.
//!
//! Originally there was no configuration layer at all: the on-premises data directory
//! (`./data/on_premises/...`) and the listen address (`127.0.0.1:8080`) were hard-coded
//! in several places. That made the binary impossible to retarget, and impossible to run
//! behind the Lambda Web Adapter / on any host that assigns the port dynamically.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Default listen port. AWS Lambda's Web Adapter also uses 8080 and passes it via `$PORT`.
pub const DEFAULT_PORT: u16 = 8080;

/// Origins allowed by the CORS layer when `CORS_ALLOWED_ORIGINS` is not set.
/// This matches the Angular dev server (`ng serve` defaults to port 4200).
pub const DEFAULT_CORS_ORIGINS: &str = "http://localhost:4200";

/// Minimum accepted `JWT_SECRET` length. Shorter secrets are rejected outright rather
/// than silently accepted.
pub const MIN_JWT_SECRET_LEN: usize = 32;

/// Default token lifetime in hours (`TOKEN_TTL_HOURS`).
pub const DEFAULT_TOKEN_TTL_HOURS: i64 = 12;

/// Default lifetime of a shareable preview link in minutes (`PREVIEW_LINK_TTL_MINUTES`).
pub const DEFAULT_PREVIEW_LINK_TTL_MINUTES: i64 = 60;

/// Default lifetime of an administrator-issued password reset link in minutes
/// (`PASSWORD_RESET_TTL_MINUTES`).
pub const DEFAULT_PASSWORD_RESET_TTL_MINUTES: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Interface to bind. Local development defaults to loopback; deployments that need
    /// to be reached from outside the process (containers, Lambda Web Adapter) set
    /// `HOST=0.0.0.0`.
    pub host: String,
    /// TCP port to listen on (`PORT`).
    pub port: u16,
    /// Root directory for on-premises storage (`DATA_ROOT`). Only that backend reads it;
    /// where its files live under this root is the adapter's own business
    /// (`on_premises::storage_dir`).
    pub data_root: PathBuf,
    /// Allowed CORS origins (`CORS_ALLOWED_ORIGINS`, comma separated).
    /// A single `*` entry allows any origin.
    pub cors_allowed_origins: Vec<String>,
    /// HMAC secret used to sign JWTs (`JWT_SECRET`).
    pub jwt_secret: Vec<u8>,
    /// True when no `JWT_SECRET` was configured and a random one was generated for this
    /// process only, so issued tokens stop working after a restart.
    pub jwt_secret_is_ephemeral: bool,
    /// Token lifetime in hours (`TOKEN_TTL_HOURS`).
    pub token_ttl_hours: i64,
    /// How long a signed preview link stays valid (`PREVIEW_LINK_TTL_MINUTES`).
    pub preview_link_ttl_minutes: i64,
    /// How long an issued password reset link stays valid (`PASSWORD_RESET_TTL_MINUTES`).
    pub password_reset_ttl_minutes: i64,
    /// Sign-in identifier for the initial administrator (`ADMIN_USERNAME`, or `ADMIN_EMAIL`
    /// as the older name of the same setting). Only consulted when the user store is still
    /// empty, and only by the on-premises backend: an `aws` deployment authenticates with
    /// Cognito and bootstraps its first administrator through `BOOTSTRAP_ADMIN_USERNAMES`.
    pub admin_username: Option<String>,
    /// Contact address for the initial administrator (`ADMIN_EMAIL`), if one is wanted.
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    /// Webhook receivers notified when content is published or unpublished
    /// (`WEBHOOK_URLS`, comma separated). Empty disables webhooks entirely.
    pub webhook_urls: Vec<String>,
    /// HMAC-SHA256 secret used to sign webhook bodies (`WEBHOOK_SECRET`). Optional: without
    /// it, payloads are delivered unsigned.
    pub webhook_secret: Option<Vec<u8>>,
    /// AWS region (`AWS_REGION`). The `aws` backend only.
    pub aws_region: Option<String>,
    /// Where the AWS endpoints are (`AWS_ENDPOINT_URL`). Only for a local emulator such as
    /// DynamoDB Local or MinIO; unset means the real endpoints.
    pub aws_endpoint_url: Option<String>,
    /// DynamoDB table holding everything structured (`DYNAMODB_TABLE`).
    pub dynamodb_table: Option<String>,
    /// S3 bucket holding uploaded image bytes (`S3_BUCKET`).
    pub s3_bucket: Option<String>,
    /// Cognito user pool whose tokens are accepted (`COGNITO_USER_POOL_ID`).
    pub cognito_user_pool_id: Option<String>,
    /// Identities allowed to provision themselves as the first administrator
    /// (`BOOTSTRAP_ADMIN_USERNAMES`, comma separated). See `doc/aws-plan.md`.
    pub bootstrap_admin_usernames: Vec<String>,
}

/// What the `aws` backend needs, once the environment has been read and checked.
///
/// Behind the feature flag because the on-premises build has no use for it - the fields above
/// are still read here so a deployment's environment is described in one place.
///
/// Checked here rather than at first use so a misconfigured deployment fails at startup with a
/// sentence, which is also what the on-premises backend does when it cannot open its storage.
#[cfg(feature = "aws")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsSettings {
    pub region: String,
    pub table: String,
    pub bucket: String,
    pub user_pool_id: String,
    /// Where the DynamoDB/S3 endpoints are, for a local emulator. `None` means the real AWS
    /// endpoints, which is what a deployment uses.
    pub endpoint_url: Option<String>,
    /// May be empty: then nobody can provision themselves and an administrator has to create
    /// the first account record another way.
    pub bootstrap_admin_usernames: Vec<String>,
}

#[cfg(feature = "aws")]
impl AwsSettings {
    /// The endpoint the SDK should talk to: a local emulator when set, AWS otherwise.
    pub fn endpoint_url(&self) -> String {
        self.endpoint_url
            .clone()
            .unwrap_or_else(|| format!("https://dynamodb.{}.amazonaws.com", self.region))
    }

    /// Where Cognito publishes the signing keys for this pool. Derived rather than configured,
    /// so a pool id and a region cannot disagree.
    pub fn jwks_url(&self) -> String {
        format!(
            "https://cognito-idp.{}.amazonaws.com/{}/.well-known/jwks.json",
            self.region, self.user_pool_id
        )
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "127.0.0.1".to_string(),
            port: DEFAULT_PORT,
            data_root: PathBuf::from("./data"),
            cors_allowed_origins: vec![DEFAULT_CORS_ORIGINS.to_string()],
            jwt_secret: generate_ephemeral_secret(),
            jwt_secret_is_ephemeral: true,
            token_ttl_hours: DEFAULT_TOKEN_TTL_HOURS,
            preview_link_ttl_minutes: DEFAULT_PREVIEW_LINK_TTL_MINUTES,
            password_reset_ttl_minutes: DEFAULT_PASSWORD_RESET_TTL_MINUTES,
            admin_username: None,
            admin_email: None,
            admin_password: None,
            webhook_urls: Vec::new(),
            webhook_secret: None,
            aws_region: None,
            aws_endpoint_url: None,
            dynamodb_table: None,
            s3_bucket: None,
            cognito_user_pool_id: None,
            bootstrap_admin_usernames: Vec::new(),
        }
    }
}

impl Config {
    /// Read configuration from the environment, falling back to [`Config::default`].
    ///
    /// Unparsable values are hard errors rather than silent fallbacks, because silently
    /// listening on the wrong port (or using a weak secret) is harder to diagnose.
    pub fn from_env() -> Result<Self, String> {
        let mut config = Config::default();

        if let Ok(host) = std::env::var("HOST") {
            if !host.is_empty() {
                config.host = host;
            }
        }
        if let Ok(port) = std::env::var("PORT") {
            if !port.is_empty() {
                config.port = port
                    .parse()
                    .map_err(|e| format!("invalid PORT {port:?}: {e}"))?;
            }
        }
        if let Ok(root) = std::env::var("DATA_ROOT") {
            if !root.is_empty() {
                config.data_root = PathBuf::from(root);
            }
        }
        if let Ok(origins) = std::env::var("CORS_ALLOWED_ORIGINS") {
            if !origins.trim().is_empty() {
                config.cors_allowed_origins = parse_origins(&origins);
            }
        }
        match std::env::var("JWT_SECRET") {
            Ok(secret) if secret.len() >= MIN_JWT_SECRET_LEN => {
                config.jwt_secret = secret.into_bytes();
                config.jwt_secret_is_ephemeral = false;
            }
            Ok(secret) => {
                return Err(format!(
                    "JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} characters (got {})",
                    secret.len()
                ))
            }
            // No secret configured: keep the generated one and warn at startup.
            Err(_) => {}
        }
        if let Ok(ttl) = std::env::var("TOKEN_TTL_HOURS") {
            if !ttl.trim().is_empty() {
                let hours: i64 = ttl
                    .trim()
                    .parse()
                    .map_err(|e| format!("invalid TOKEN_TTL_HOURS {ttl:?}: {e}"))?;
                if hours < 1 {
                    return Err(format!("TOKEN_TTL_HOURS must be at least 1 (got {hours})"));
                }
                config.token_ttl_hours = hours;
            }
        }
        if let Ok(ttl) = std::env::var("PREVIEW_LINK_TTL_MINUTES") {
            if !ttl.trim().is_empty() {
                let minutes: i64 = ttl
                    .trim()
                    .parse()
                    .map_err(|e| format!("invalid PREVIEW_LINK_TTL_MINUTES {ttl:?}: {e}"))?;
                if minutes < 1 {
                    return Err(format!(
                        "PREVIEW_LINK_TTL_MINUTES must be at least 1 (got {minutes})"
                    ));
                }
                config.preview_link_ttl_minutes = minutes;
            }
        }
        // An identifier can be anything (see `is_plausible_username`), so the old name of this
        // setting is only a fallback: `ADMIN_EMAIL` alone still bootstraps the same account.
        if let Ok(ttl) = std::env::var("PASSWORD_RESET_TTL_MINUTES") {
            if !ttl.trim().is_empty() {
                let minutes: i64 = ttl
                    .trim()
                    .parse()
                    .map_err(|e| format!("invalid PASSWORD_RESET_TTL_MINUTES {ttl:?}: {e}"))?;
                if minutes < 1 {
                    return Err(format!(
                        "PASSWORD_RESET_TTL_MINUTES must be at least 1 (got {minutes})"
                    ));
                }
                config.password_reset_ttl_minutes = minutes;
            }
        }
        config.admin_username =
            non_empty_env("ADMIN_USERNAME").or_else(|| non_empty_env("ADMIN_EMAIL"));
        config.aws_region = non_empty_env("AWS_REGION");
        config.aws_endpoint_url = non_empty_env("AWS_ENDPOINT_URL");
        config.dynamodb_table = non_empty_env("DYNAMODB_TABLE");
        config.s3_bucket = non_empty_env("S3_BUCKET");
        config.cognito_user_pool_id = non_empty_env("COGNITO_USER_POOL_ID");
        if let Ok(names) = std::env::var("BOOTSTRAP_ADMIN_USERNAMES") {
            config.bootstrap_admin_usernames = parse_usernames(&names);
        }
        config.admin_email = non_empty_env("ADMIN_EMAIL");
        config.admin_password = non_empty_env("ADMIN_PASSWORD");
        if let Ok(urls) = std::env::var("WEBHOOK_URLS") {
            config.webhook_urls = parse_webhook_urls(&urls)?;
        }
        config.webhook_secret = non_empty_env("WEBHOOK_SECRET").map(String::into_bytes);
        Ok(config)
    }

    /// The settings the `aws` backend cannot start without, or the first thing that is
    /// missing.
    #[cfg(feature = "aws")]
    pub fn aws_settings(&self) -> Result<AwsSettings, String> {
        let required = |value: &Option<String>, name: &str| {
            value
                .clone()
                .ok_or_else(|| format!("{name} must be set for the `aws` backend"))
        };
        Ok(AwsSettings {
            region: required(&self.aws_region, "AWS_REGION")?,
            table: required(&self.dynamodb_table, "DYNAMODB_TABLE")?,
            bucket: required(&self.s3_bucket, "S3_BUCKET")?,
            user_pool_id: required(&self.cognito_user_pool_id, "COGNITO_USER_POOL_ID")?,
            endpoint_url: self.aws_endpoint_url.clone(),
            bootstrap_admin_usernames: self.bootstrap_admin_usernames.clone(),
        })
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, String> {
        let raw = format!("{}:{}", self.host, self.port);
        raw.parse()
            .map_err(|e| format!("invalid listen address {raw:?}: {e}"))
    }

}

/// `BOOTSTRAP_ADMIN_USERNAMES` as a list, normalized the way accounts are stored so the
/// comparison at sign-in is a plain equality check.
fn parse_usernames(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(crate::models::user::normalize_username)
        .filter(|name| !name.is_empty())
        .collect()
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// 32 bytes from the OS CSPRNG (two v4 UUIDs), used only when `JWT_SECRET` is unset.
fn generate_ephemeral_secret() -> Vec<u8> {
    let mut secret = Vec::with_capacity(32);
    secret.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    secret.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    secret
}

fn parse_origins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Parse the webhook receiver list, rejecting anything that could not be delivered to.
///
/// A typo in a URL is a startup error rather than a surprise at the first publish, which
/// may be days later.
fn parse_webhook_urls(raw: &str) -> Result<Vec<String>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(|url| {
            let parsed = reqwest::Url::parse(url)
                .map_err(|e| format!("invalid WEBHOOK_URLS entry {url:?}: {e}"))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(format!(
                    "invalid WEBHOOK_URLS entry {url:?}: only http and https are supported"
                ));
            }
            if parsed.host_str().is_none() {
                return Err(format!("invalid WEBHOOK_URLS entry {url:?}: no host"));
            }
            Ok(parsed.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_loopback_and_localhost_origin() {
        let config = Config::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.data_root, PathBuf::from("./data"));
        assert_eq!(config.cors_allowed_origins, vec![DEFAULT_CORS_ORIGINS.to_string()]);
        assert_eq!(config.token_ttl_hours, DEFAULT_TOKEN_TTL_HOURS);
        assert_eq!(
            config.preview_link_ttl_minutes,
            DEFAULT_PREVIEW_LINK_TTL_MINUTES
        );
        assert_eq!(
            config.password_reset_ttl_minutes,
            DEFAULT_PASSWORD_RESET_TTL_MINUTES
        );
        // Without an explicit secret the process must know its secret is ephemeral.
        assert!(config.jwt_secret_is_ephemeral);
        assert!(config.jwt_secret.len() >= MIN_JWT_SECRET_LEN);
    }

    #[test]
    fn ephemeral_secrets_differ_between_configs() {
        assert_ne!(Config::default().jwt_secret, Config::default().jwt_secret);
    }

    #[test]
    fn parses_bootstrap_usernames_the_way_accounts_are_stored() {
        assert_eq!(
            parse_usernames(" Ops , Alice@Example.com ,, "),
            vec!["ops".to_string(), "alice@example.com".to_string()]
        );
        assert_eq!(parse_usernames(""), Vec::<String>::new());
        assert_eq!(parse_usernames(" , "), Vec::<String>::new());
    }

    /// The `aws` backend refuses to start without these, and says which one is missing.
    #[cfg(feature = "aws")]
    #[test]
    fn aws_settings_report_the_first_missing_value_and_derive_the_jwks_url() {
        let empty = Config::default();
        assert!(empty.aws_settings().unwrap_err().contains("AWS_REGION"));

        let partial = Config { aws_region: Some("eu-west-1".to_string()), ..Config::default() };
        assert!(partial.aws_settings().unwrap_err().contains("DYNAMODB_TABLE"));

        let full = Config {
            aws_region: Some("eu-west-1".to_string()),
            dynamodb_table: Some("cms".to_string()),
            s3_bucket: Some("cms-images".to_string()),
            cognito_user_pool_id: Some("eu-west-1_abc".to_string()),
            bootstrap_admin_usernames: parse_usernames("Ops"),
            ..Config::default()
        };
        let settings = full.aws_settings().unwrap();
        assert_eq!(settings.table, "cms");
        assert_eq!(settings.bootstrap_admin_usernames, vec!["ops".to_string()]);
        assert_eq!(
            settings.jwks_url(),
            "https://cognito-idp.eu-west-1.amazonaws.com/eu-west-1_abc/.well-known/jwks.json"
        );
    }

    #[test]
    fn parses_comma_separated_origins_and_trims_whitespace() {
        assert_eq!(
            parse_origins(" http://a.example , http://b.example ,"),
            vec!["http://a.example".to_string(), "http://b.example".to_string()]
        );
        assert!(parse_origins(" * ").iter().any(|o| o == "*"));
    }

    #[test]
    fn builds_socket_addr() {
        let config = Config {
            host: "0.0.0.0".to_string(),
            port: 9000,
            ..Config::default()
        };
        assert_eq!(config.socket_addr().unwrap().to_string(), "0.0.0.0:9000");
    }

    #[test]
    fn webhooks_are_disabled_by_default() {
        let config = Config::default();
        assert!(config.webhook_urls.is_empty());
        assert!(config.webhook_secret.is_none());
    }

    #[test]
    fn parses_comma_separated_webhook_urls_and_trims_whitespace() {
        assert_eq!(
            parse_webhook_urls(" https://a.example/hook , http://b.example:9000/hook ,").unwrap(),
            vec![
                "https://a.example/hook".to_string(),
                "http://b.example:9000/hook".to_string()
            ]
        );
    }

    #[test]
    fn rejects_webhook_urls_that_could_not_be_delivered_to() {
        // A typo has to be a startup error, not a silent no-op at publish time.
        assert!(parse_webhook_urls("not-a-url").is_err());
        assert!(parse_webhook_urls("ftp://example.com/hook").is_err());
        assert!(parse_webhook_urls("file:///etc/passwd").is_err());
    }
}
