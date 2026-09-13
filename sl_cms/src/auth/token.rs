//! Issuing and verifying JWTs (HS256).
//!
//! A shared secret keeps this dependency-free on the storage side, which matters because
//! the same tokens must be verifiable by an AWS deployment later (where Cognito can take
//! over issuing them without changing the verification path).

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::models::identity::StringId;
use crate::models::user::{User, UserId};

/// Token lifetime in hours when `TOKEN_TTL_HOURS` is not set.
pub const DEFAULT_TTL_HOURS: i64 = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// Subject: the user id.
    pub sub: String,
    /// Issued at (unix seconds).
    pub iat: i64,
    /// Expiry (unix seconds).
    pub exp: i64,
    /// Whether the user is an administrator, so the common case needs no storage lookup.
    pub adm: bool,
}

#[derive(Clone)]
pub struct TokenIssuer {
    encoding: EncodingKey,
    decoding: DecodingKey,
    ttl: Duration,
}

impl TokenIssuer {
    pub fn new(secret: &[u8], ttl_hours: i64) -> Self {
        TokenIssuer {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            ttl: Duration::hours(ttl_hours.max(1)),
        }
    }

    /// Issue a token for `user`, returning it with its expiry.
    pub fn issue(&self, user: &User) -> Result<(String, DateTime<Utc>), String> {
        let now = Utc::now();
        let expires_at = now + self.ttl;
        let claims = Claims {
            sub: user.id.to_string(),
            iat: now.timestamp(),
            exp: expires_at.timestamp(),
            adm: user.is_admin,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|e| format!("failed to issue token: {e}"))?;
        Ok((token, expires_at))
    }

    /// Verify a token, returning its claims.
    pub fn verify(&self, token: &str) -> Result<Claims, String> {
        let mut validation = Validation::new(Algorithm::HS256);
        // `exp` is validated by default; require it explicitly so a token without an
        // expiry can never be accepted.
        validation.required_spec_claims.insert("exp".to_string());
        decode::<Claims>(token, &self.decoding, &validation)
            .map(|data| data.claims)
            .map_err(|e| format!("invalid token: {e}"))
    }

    /// The user id carried by a verified token.
    pub fn verify_subject(&self, token: &str) -> Result<UserId, String> {
        let claims = self.verify(token)?;
        Ok(StringId::from(claims.sub.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::user::{Permission, User};

    fn user() -> User {
        User {
            id: StringId::from("u-1"),
            email: "a@example.com".to_string(),
            password_hash: "x".to_string(),
            is_active: true,
            is_admin: true,
            permission: Permission::default(),
            created_at: Utc::now(),
            last_login: None,
        }
    }

    #[test]
    fn issues_and_verifies_a_token() {
        let issuer = TokenIssuer::new(b"secret", 1);
        let (token, expires_at) = issuer.issue(&user()).unwrap();
        assert!(expires_at > Utc::now());

        let claims = issuer.verify(&token).unwrap();
        assert_eq!(claims.sub, "u-1");
        assert!(claims.adm);
        assert_eq!(issuer.verify_subject(&token).unwrap().to_string(), "u-1");
    }

    #[test]
    fn rejects_a_token_signed_with_another_secret() {
        let issuer = TokenIssuer::new(b"secret", 1);
        let other = TokenIssuer::new(b"different", 1);
        let (token, _) = issuer.issue(&user()).unwrap();
        assert!(other.verify(&token).is_err());
    }

    #[test]
    fn rejects_garbage_and_expired_tokens() {
        let issuer = TokenIssuer::new(b"secret", 1);
        assert!(issuer.verify("not-a-jwt").is_err());

        // ttl_hours is clamped to >= 1 hour, so build an already-expired token directly.
        let now = Utc::now();
        let expired = Claims {
            sub: "u-1".to_string(),
            iat: (now - Duration::hours(2)).timestamp(),
            exp: (now - Duration::hours(1)).timestamp(),
            adm: false,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &expired,
            &EncodingKey::from_secret(b"secret"),
        )
        .unwrap();
        assert!(issuer.verify(&token).is_err());
    }

    #[test]
    fn clamps_non_positive_ttl() {
        // A zero/negative TTL must not produce instantly-dead tokens.
        let issuer = TokenIssuer::new(b"secret", 0);
        let (token, _) = issuer.issue(&user()).unwrap();
        assert!(issuer.verify(&token).is_ok());
    }
}
