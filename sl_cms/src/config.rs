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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Interface to bind. Local development defaults to loopback; deployments that need
    /// to be reached from outside the process (containers, Lambda Web Adapter) set
    /// `HOST=0.0.0.0`.
    pub host: String,
    /// TCP port to listen on (`PORT`).
    pub port: u16,
    /// Root directory for on-premises storage (`DATA_ROOT`).
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
    /// Credentials for the initial administrator (`ADMIN_EMAIL` / `ADMIN_PASSWORD`).
    /// Only consulted when the user store is still empty.
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
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
            admin_email: None,
            admin_password: None,
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
        config.admin_email = non_empty_env("ADMIN_EMAIL");
        config.admin_password = non_empty_env("ADMIN_PASSWORD");
        Ok(config)
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, String> {
        let raw = format!("{}:{}", self.host, self.port);
        raw.parse()
            .map_err(|e| format!("invalid listen address {raw:?}: {e}"))
    }

    /// On-premises storage root, e.g. `./data/on_premises`.
    pub fn on_premises_dir(&self) -> PathBuf {
        self.data_root.join("on_premises")
    }

    pub fn rkv_dir(&self) -> PathBuf {
        self.on_premises_dir().join("rkv_data")
    }

    pub fn images_dir(&self) -> PathBuf {
        self.on_premises_dir().join("images")
    }
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
        // Without an explicit secret the process must know its secret is ephemeral.
        assert!(config.jwt_secret_is_ephemeral);
        assert!(config.jwt_secret.len() >= MIN_JWT_SECRET_LEN);
    }

    #[test]
    fn ephemeral_secrets_differ_between_configs() {
        assert_ne!(Config::default().jwt_secret, Config::default().jwt_secret);
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
    fn derives_storage_paths_from_data_root() {
        let config = Config {
            data_root: PathBuf::from("/tmp/cms"),
            ..Config::default()
        };
        assert_eq!(config.rkv_dir(), PathBuf::from("/tmp/cms/on_premises/rkv_data"));
        assert_eq!(config.images_dir(), PathBuf::from("/tmp/cms/on_premises/images"));
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
}
