//! Who the caller is, whichever way the deployment authenticates them.
//!
//! Two kinds of token reach this CMS. The on-premises deployment issues its own (HS256, signed
//! with the secret it already has) and knows the account id directly. A deployment that uses an
//! identity provider receives a token it did not issue: the subject is the *provider's*
//! identifier for the account, which has to be resolved to a local record. Everything above
//! this — the middleware, the permission checks — only cares about the [`Identity`], and
//! everything below it is the deployment's business.

use std::future::Future;
use std::pin::Pin;

use crate::models::error::HttpError;
use crate::models::user::UserId;

/// A verified caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// A token this CMS issued.
    ///
    /// The subject is the account id, and the generation it carries is checked against the
    /// stored record: that is how changing a password ends every session issued before it.
    Local {
        user_id: UserId,
        token_version: u64,
    },
    /// A token an identity provider issued and signed.
    ///
    /// The subject is *its* identifier for the account (Cognito's `sub`); the local record is
    /// found by [`User::external_id`](crate::models::user::User::external_id). The username and
    /// address are carried because a deployment may need to create that record the first time
    /// someone signs in.
    External {
        external_id: String,
        username: String,
        email: Option<String>,
    },
}

/// Verifies the bearer token of a request.
///
/// Errors are [`HttpError::Unauthorized`] with a message that says nothing about *why*: which
/// part of a token was wrong is not something a caller should be able to probe.
/// Boxed rather than `impl Future` because the service holds one of these behind a `dyn` — a
/// deployment picks its verifier at composition time, not at compile time. Same shape as
/// [`Notifier`](crate::webhook::Notifier), for the same reason.
pub type VerifyFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Identity, HttpError>> + Send + 'a>>;

pub trait TokenVerifier: Send + Sync + 'static {
    fn verify<'a>(&'a self, token: &'a str) -> VerifyFuture<'a>;
}
