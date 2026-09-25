//! The password a *local* deployment stores for an account.
//!
//! Where sign-in belongs to an identity provider (the AWS backend uses Cognito), the CMS never
//! sees a password: the browser signs in against the provider and all the CMS does with the
//! result is verify a token. There is therefore no password to hash, store, verify or reset —
//! and an adapter that has no such provider implements this trait and serves the endpoints that
//! use it, while one that delegates simply does not.
//!
//! The trait speaks plaintext, never a hash: how a password is turned into something storable
//! (Argon2id, a PHC string) is the local adapter's business and stays inside it. That is also
//! why [`User`](crate::models::user::User) no longer has a password field at all — a record
//! that cannot carry a credential is one that cannot leak it.

use std::future::Future;

use crate::models::user::UserId;
use crate::repositories::user_repository::BoxError;

/// Where a local deployment keeps the password of an account.
///
/// All three are idempotent from the caller's point of view: setting a password replaces any
/// previous one, verifying an account that has none is `false` rather than an error, and
/// deleting one that is not there succeeds.
pub trait LocalCredentials: Send + Sync + 'static {
    /// Store `password` for `user_id`, replacing whatever was there.
    fn set_password(
        &self,
        user_id: &UserId,
        password: &str,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// Whether `password` is the one stored for `user_id`.
    ///
    /// A false answer covers both "wrong password" and "no password set": the caller must not
    /// be able to tell those apart, any more than it can tell a wrong password from an account
    /// that does not exist.
    fn verify_password(
        &self,
        user_id: &UserId,
        password: &str,
    ) -> impl Future<Output = Result<bool, BoxError>> + Send;

    /// Forget the password for `user_id`.
    ///
    /// Called when the account goes away, so a credential cannot outlive it.
    fn delete_password(
        &self,
        user_id: &UserId,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}
