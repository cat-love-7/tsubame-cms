//! Signed, expiring links to unpublished content.
//!
//! The admin can already look at a working copy, but reviewing it with someone who has no
//! account (a client, a translator) meant handing over a token. A preview link is the
//! smallest thing that works instead: the CMS signs what the link points at together with
//! the moment it stops being valid, and the signature is the whole credential.
//!
//! Two properties follow from signing the target rather than storing the link:
//!
//! * **A link cannot be pointed somewhere else.** Editing the collection, the item id or the
//!   expiry in the URL invalidates the signature, so the holder of one link cannot read any
//!   other unpublished item with it.
//! * **Nothing has to be stored or cleaned up.** Expiry is part of the signed message, so
//!   there is no table of live links to prune.
//!
//! The link is still a bearer credential: anyone who has it can read that one working copy
//! until it expires. That is the point, and it is worth saying out loud in the docs.

use chrono::{DateTime, Duration, Utc};

use crate::signing;

/// Prefix of the signed message. It keeps a preview signature from ever being confused with
/// another signed message (a webhook body, say) and leaves room to change the format later.
const PREFIX: &str = "tsubame-preview:v1";

/// What a link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewTarget {
    CollectionItem { collection: String, item_id: u64 },
    SinglePage { page: String },
}

impl PreviewTarget {
    /// The public route that serves this target, without the token.
    ///
    /// The whole path, [`crate::API_PREFIX`] included: a link that is copied and opened has to be
    /// an address that works, not one a client has to know to prefix.
    pub fn path(&self) -> String {
        match self {
            PreviewTarget::CollectionItem {
                collection,
                item_id,
            } => {
                format!(
                    "{}/preview/collections/{collection}/items/{item_id}",
                    crate::API_PREFIX
                )
            }
            PreviewTarget::SinglePage { page } => {
                format!("{}/preview/single_pages/{page}", crate::API_PREFIX)
            }
        }
    }

    /// The message the signature covers: what, and until when.
    fn signed_message(&self, expires: i64) -> String {
        match self {
            PreviewTarget::CollectionItem {
                collection,
                item_id,
            } => {
                format!("{PREFIX}:collection:{collection}:{item_id}:{expires}")
            }
            PreviewTarget::SinglePage { page } => format!("{PREFIX}:page:{page}:{expires}"),
        }
    }
}

/// A preview link, ready to hand to whoever is reviewing.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PreviewLink {
    /// Route plus token, relative to the API the caller reaches the CMS through.
    pub path: String,
    pub expires_at: DateTime<Utc>,
}

/// Why a link was refused. Kept apart so the answer can say "expired" rather than pretending
/// the link was never valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewLinkError {
    /// Not shaped like a link: no separator, or an expiry that is not a number.
    Malformed,
    /// The signature is not ours, or not for this target and expiry.
    Invalid,
    /// Genuine, but its time is up.
    Expired,
}

impl PreviewLinkError {
    /// The message a client sees. Deliberately says nothing about the secret.
    pub fn message(self) -> &'static str {
        match self {
            PreviewLinkError::Malformed => "this is not a preview link",
            PreviewLinkError::Invalid => "this preview link is not valid",
            PreviewLinkError::Expired => "this preview link has expired",
        }
    }
}

#[derive(Clone)]
pub struct PreviewLinkIssuer {
    key: Vec<u8>,
    ttl: Duration,
}

impl PreviewLinkIssuer {
    /// `ttl_minutes` is clamped to at least one, so a misconfigured value cannot mint links
    /// that are dead on arrival.
    pub fn new(secret: &[u8], ttl_minutes: i64) -> Self {
        PreviewLinkIssuer {
            key: secret.to_vec(),
            ttl: Duration::minutes(ttl_minutes.max(1)),
        }
    }

    /// Mint a link for `target`, valid until `now + ttl`.
    ///
    /// `now` is a parameter so tests can issue a link that is already expired without
    /// waiting.
    pub fn issue(&self, target: &PreviewTarget, now: DateTime<Utc>) -> PreviewLink {
        let expires_at = now + self.ttl;
        let expires = expires_at.timestamp();
        let signature = signing::sign(&self.key, target.signed_message(expires).as_bytes());
        PreviewLink {
            path: format!("{}?token={expires}.{signature}", target.path()),
            expires_at,
        }
    }

    /// Whether `token` is ours, is for this target, and is still in date.
    pub fn verify(
        &self,
        target: &PreviewTarget,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<(), PreviewLinkError> {
        let (expires, signature) = token.split_once('.').ok_or(PreviewLinkError::Malformed)?;
        let expires: i64 = expires.parse().map_err(|_| PreviewLinkError::Malformed)?;

        if !signing::verify(
            &self.key,
            target.signed_message(expires).as_bytes(),
            signature,
        ) {
            return Err(PreviewLinkError::Invalid);
        }

        // Only after the signature: expiry is what the signature covers, so an unsigned
        // token must not be able to claim either "still valid" or "expired".
        if expires <= now.timestamp() {
            return Err(PreviewLinkError::Expired);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_link() -> PreviewTarget {
        PreviewTarget::CollectionItem {
            collection: "blog".to_string(),
            item_id: 7,
        }
    }

    fn page_link() -> PreviewTarget {
        PreviewTarget::SinglePage {
            page: "home".to_string(),
        }
    }

    /// The lifetime these tests ask for; the deployment's own default is
    /// `config::DEFAULT_PREVIEW_LINK_TTL_MINUTES`.
    const TTL_MINUTES: i64 = 60;

    fn issuer() -> PreviewLinkIssuer {
        PreviewLinkIssuer::new(b"preview-secret", TTL_MINUTES)
    }

    /// The token comes back out of the query string as written, so the link needs no
    /// escaping and the docs can show it verbatim.
    fn token_of(link: &PreviewLink) -> String {
        link.path
            .split_once("?token=")
            .expect("token in the path")
            .1
            .to_string()
    }

    #[test]
    fn a_fresh_link_verifies_for_its_own_target() {
        let issuer = issuer();
        let now = Utc::now();
        let link = issuer.issue(&item_link(), now);

        assert!(link.expires_at > now);
        assert_eq!(link.expires_at, now + Duration::minutes(TTL_MINUTES));
        assert_eq!(
            link.path,
            format!(
                "/api/preview/collections/blog/items/7?token={}",
                token_of(&link)
            )
        );
        assert!(issuer.verify(&item_link(), &token_of(&link), now).is_ok());

        // The token is URL-safe: nothing in it needs escaping.
        assert!(
            token_of(&link)
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        );
    }

    #[test]
    fn a_link_only_opens_the_target_it_was_made_for() {
        let issuer = issuer();
        let now = Utc::now();
        let token = token_of(&issuer.issue(&item_link(), now));

        // Another item, another collection, another kind of target: all refused, because the
        // target is part of what was signed.
        let others = [
            PreviewTarget::CollectionItem {
                collection: "blog".to_string(),
                item_id: 8,
            },
            PreviewTarget::CollectionItem {
                collection: "news".to_string(),
                item_id: 7,
            },
            page_link(),
        ];
        for other in others {
            assert_eq!(
                issuer.verify(&other, &token, now),
                Err(PreviewLinkError::Invalid),
                "{other:?}"
            );
        }

        // As is a link signed with a different secret.
        let other_secret = PreviewLinkIssuer::new(b"another-secret", TTL_MINUTES);
        assert_eq!(
            other_secret.verify(&item_link(), &token, now),
            Err(PreviewLinkError::Invalid)
        );
    }

    #[test]
    fn a_link_stops_working_when_its_time_is_up() {
        let issuer = issuer();
        let now = Utc::now();
        let link = issuer.issue(&item_link(), now);
        let token = token_of(&link);

        assert!(issuer.verify(&item_link(), &token, now).is_ok());
        // One second before the end it is still good; at the end it is not.
        assert!(
            issuer
                .verify(&item_link(), &token, link.expires_at - Duration::seconds(1))
                .is_ok()
        );
        assert_eq!(
            issuer.verify(&item_link(), &token, link.expires_at),
            Err(PreviewLinkError::Expired)
        );
        assert_eq!(
            issuer.verify(&item_link(), &token, link.expires_at + Duration::days(1)),
            Err(PreviewLinkError::Expired)
        );
    }

    /// Pushing the expiry out is the obvious attack on a link like this, and the signature is
    /// what stops it.
    #[test]
    fn the_expiry_cannot_be_edited() {
        let issuer = issuer();
        let now = Utc::now();
        let short = issuer.issue(&item_link(), now - Duration::hours(2));
        let (_, signature) = short.path.split_once('.').expect("token has a separator");

        let forged = format!("{}.{signature}", (now + Duration::days(365)).timestamp());
        assert_eq!(
            issuer.verify(&item_link(), &forged, now),
            Err(PreviewLinkError::Invalid)
        );

        // The original, untouched, is simply expired.
        assert_eq!(
            issuer.verify(&item_link(), &token_of(&short), now),
            Err(PreviewLinkError::Expired)
        );
    }

    #[test]
    fn refuses_things_that_are_not_links() {
        let issuer = issuer();
        let now = Utc::now();
        for token in [
            "",
            "123",
            "abc.def",
            "1.",
            ".abc",
            "99999999999999999999.00",
            "1.2.3",
        ] {
            let result = issuer.verify(&item_link(), token, now);
            assert!(
                matches!(
                    result,
                    Err(PreviewLinkError::Malformed) | Err(PreviewLinkError::Invalid)
                ),
                "{token:?} → {result:?}"
            );
        }
    }

    #[test]
    fn a_single_page_link_has_its_own_path() {
        let link = issuer().issue(&page_link(), Utc::now());
        assert!(
            link.path
                .starts_with("/api/preview/single_pages/home?token="),
            "{}",
            link.path
        );
    }

    /// A misconfigured lifetime must not mint links that are dead on arrival.
    #[test]
    fn a_non_positive_ttl_is_clamped() {
        for ttl in [0, -30] {
            let now = Utc::now();
            let link = PreviewLinkIssuer::new(b"secret", ttl).issue(&item_link(), now);
            assert_eq!(link.expires_at, now + Duration::minutes(1), "ttl={ttl}");
        }
    }
}
