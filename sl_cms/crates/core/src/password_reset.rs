//! Password reset links, issued by an administrator and completed by the account's owner.
//!
//! The point of a link rather than the administrator typing a new password is that nobody but
//! the owner ever knows the password. Nothing has to be mailed for that to work: the CMS hands
//! the link to whoever asked for it, and they pass it on however they like.
//!
//! Two properties come from signing the account's *token generation* along with the deadline:
//!
//! * **A link is single use.** Completing a reset bumps `token_version` (see
//!   [`crate::models::user::User::end_existing_sessions`]), so the version inside the signed
//!   message no longer matches and the same link is refused from then on.
//! * **Nothing has to be stored or cleaned up.** Expiry and single use are both values the
//!   account already carries, so there is no table of outstanding resets to prune.
//!
//! The link is a bearer credential while it is alive, which is why the default lifetime is
//! short: whoever has the link can set the account's password.

use chrono::{DateTime, Duration, Utc};

use crate::models::user::{User, UserId};
use crate::signing;

/// Prefix of the signed message. It keeps a reset signature from ever being replayed as
/// another kind of signed message (a preview link, a webhook body) and leaves room to change
/// the format later.
const PREFIX: &str = "password-reset:v1";

/// A freshly issued link. The token is what goes in the URL; the moment it dies travels
/// alongside so the caller can tell the administrator how long it lasts.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PasswordResetLink {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// Why a link was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordResetError {
    /// Not shaped like a link: wrong number of parts, or values that are not numbers.
    Malformed,
    /// The signature is not ours, or not for this account, generation and deadline.
    Invalid,
    /// Genuine, but its time is up.
    Expired,
    /// The account's generation has moved on: the link was used already, or a password change
    /// (an administrator's reset, or the owner's own change) happened after it was issued.
    AlreadyUsed,
}

impl PasswordResetError {
    /// The message a client sees. Deliberately says nothing about the secret.
    pub fn message(self) -> &'static str {
        match self {
            PasswordResetError::Malformed | PasswordResetError::Invalid => {
                "this password reset link is not valid"
            }
            PasswordResetError::Expired => "this password reset link has expired",
            PasswordResetError::AlreadyUsed => {
                "this password reset link has already been used or is no longer valid"
            }
        }
    }
}

#[derive(Clone)]
pub struct PasswordResetIssuer {
    key: Vec<u8>,
    ttl: Duration,
}

impl PasswordResetIssuer {
    /// `ttl_minutes` is clamped to at least one, so a misconfigured value cannot mint links
    /// that are dead on arrival.
    pub fn new(secret: &[u8], ttl_minutes: i64) -> Self {
        PasswordResetIssuer {
            key: secret.to_vec(),
            ttl: Duration::minutes(ttl_minutes.max(1)),
        }
    }

    /// Mint a link for `user`, valid until `now + ttl`.
    ///
    /// `now` is a parameter so tests can issue an already-expired link without waiting.
    pub fn issue(&self, user: &User, now: DateTime<Utc>) -> PasswordResetLink {
        let deadline = now + self.ttl;
        // The token carries whole seconds, so the reported deadline is the same value the
        // verification uses: no "valid until 12:00:00.4" that the token would refuse.
        let expires = deadline.timestamp();
        let message = signed_message(&user.id, user.token_version, expires);
        PasswordResetLink {
            token: format!(
                "{}.{}.{}.{}",
                user.id,
                user.token_version,
                expires,
                signing::sign(&self.key, message.as_bytes())
            ),
            expires_at: DateTime::from_timestamp(expires, 0).unwrap_or(deadline),
        }
    }

    /// Check the token on its own: is it ours, is it in date, and which generation of the
    /// account does it belong to?
    ///
    /// The caller then compares that generation with the stored account, which is what makes
    /// the link single use.
    pub fn verify(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<ResetClaims, PasswordResetError> {
        let mut parts = token.split('.');
        let (Some(id), Some(version), Some(expires), Some(signature), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(PasswordResetError::Malformed);
        };
        if id.is_empty() {
            return Err(PasswordResetError::Malformed);
        }
        let version: u64 = version.parse().map_err(|_| PasswordResetError::Malformed)?;
        let expires: i64 = expires.parse().map_err(|_| PasswordResetError::Malformed)?;

        let message = signed_message(&UserId::from(id), version, expires);
        if !signing::verify(&self.key, message.as_bytes(), signature) {
            return Err(PasswordResetError::Invalid);
        }

        // Only after the signature: the deadline is what the signature covers, so an unsigned
        // token must not be able to claim either "still valid" or "expired".
        if expires <= now.timestamp() {
            return Err(PasswordResetError::Expired);
        }

        Ok(ResetClaims {
            user_id: UserId::from(id),
            token_version: version,
            expires_at: DateTime::from_timestamp(expires, 0).unwrap_or(now),
        })
    }
}

/// What a verified token says: whose account, which generation, until when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetClaims {
    pub user_id: UserId,
    pub token_version: u64,
    pub expires_at: DateTime<Utc>,
}

fn signed_message(id: &UserId, version: u64, expires: i64) -> String {
    format!("{PREFIX}:{id}:{version}:{expires}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::user::Permission;

    /// The lifetime these tests ask for; the deployment's default is
    /// `config::DEFAULT_PASSWORD_RESET_TTL_MINUTES`.
    const TTL_MINUTES: i64 = 30;

    fn account() -> User {
        User::new(
            "ops",
            false, Permission::viewer())
    }

    fn issuer() -> PasswordResetIssuer {
        PasswordResetIssuer::new(b"reset-secret", TTL_MINUTES)
    }

    #[test]
    fn a_fresh_link_verifies_for_its_own_account() {
        let issuer = issuer();
        let user = account();
        let now = Utc::now();
        let link = issuer.issue(&user, now);

        // The deadline travels in whole seconds, so compare what the token actually carries.
        assert_eq!(
            link.expires_at.timestamp(),
            (now + Duration::minutes(TTL_MINUTES)).timestamp()
        );
        let claims = issuer.verify(&link.token, now).unwrap();
        assert_eq!(claims.user_id, user.id);
        assert_eq!(claims.token_version, user.token_version);
        assert_eq!(claims.expires_at, link.expires_at);

        // The token is URL-safe: nothing in it needs escaping.
        assert!(link
            .token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
    }

    #[test]
    fn a_link_stops_working_when_its_time_is_up() {
        let issuer = issuer();
        let user = account();
        let now = Utc::now();
        let link = issuer.issue(&user, now);

        assert!(issuer
            .verify(&link.token, link.expires_at - Duration::seconds(1))
            .is_ok());
        assert_eq!(
            issuer.verify(&link.token, link.expires_at),
            Err(PasswordResetError::Expired)
        );
        assert_eq!(
            issuer.verify(&link.token, link.expires_at + Duration::days(1)),
            Err(PasswordResetError::Expired)
        );
    }

    /// The generation is signed, so pushing it (or the deadline, or the account) cannot make an
    /// old link work again.
    #[test]
    fn the_signed_values_cannot_be_edited() {
        let issuer = issuer();
        let user = account();
        let now = Utc::now();
        let link = issuer.issue(&user, now);
        let signature = link.token.rsplit('.').next().unwrap();

        let forged_version = format!("{}.{}.{}.{}", user.id, 9, link.expires_at.timestamp(), signature);
        assert_eq!(
            issuer.verify(&forged_version, now),
            Err(PasswordResetError::Invalid)
        );

        let forged_expiry = format!(
            "{}.{}.{}.{}",
            user.id,
            user.token_version,
            (now + Duration::days(30)).timestamp(),
            signature
        );
        assert_eq!(
            issuer.verify(&forged_expiry, now),
            Err(PasswordResetError::Invalid)
        );

        let someone_else = User::new(
            "other",
            false, Permission::viewer());
        let forged_account = format!(
            "{}.{}.{}.{}",
            someone_else.id,
            user.token_version,
            link.expires_at.timestamp(),
            signature
        );
        assert_eq!(
            issuer.verify(&forged_account, now),
            Err(PasswordResetError::Invalid)
        );

        // A link signed with a different secret is not ours either.
        let other_secret = PasswordResetIssuer::new(b"another-secret", TTL_MINUTES);
        assert_eq!(
            other_secret.verify(&link.token, now),
            Err(PasswordResetError::Invalid)
        );
    }

    #[test]
    fn refuses_things_that_are_not_links() {
        let issuer = issuer();
        let now = Utc::now();
        for token in [
            "",
            "abc",
            "a.b.c",
            "id.1.2.3.4",
            ".1.9999999999.deadbeef",
            "id.not-a-number.9999999999.deadbeef",
            "id.1.not-a-number.deadbeef",
            "id.1.9999999999.not-hex",
        ] {
            let result = issuer.verify(token, now);
            assert!(
                matches!(
                    result,
                    Err(PasswordResetError::Malformed) | Err(PasswordResetError::Invalid)
                ),
                "{token:?} → {result:?}"
            );
        }
    }

    /// A misconfigured lifetime must not mint links that are dead on arrival.
    #[test]
    fn a_non_positive_ttl_is_clamped() {
        for ttl in [0, -30] {
            let now = Utc::now();
            let link = PasswordResetIssuer::new(b"secret", ttl).issue(&account(), now);
            assert!(
                (link.expires_at - now).num_seconds() >= 59,
                "ttl={ttl} gave {:?}",
                link.expires_at - now
            );
            // The same secret, a one-minute lifetime: it verifies, so it is short rather than
            // already dead.
            assert!(PasswordResetIssuer::new(b"secret", 1)
                .verify(&link.token, now)
                .is_ok());
        }
    }
}
