use std::error::Error;
use std::future::Future;

use crate::models::user::{User, UserId};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// What the CMS stores about accounts.
///
/// The methods return `impl Future<Output = ...> + Send` rather than being declared `async fn`.
/// The two are the same thing except for one word: the promise that the future is `Send`. The
/// HTTP layer is generic over the storage (`R: Storage`), so without that promise in the
/// contract the compiler cannot tell axum that a handler which waits on one of these is safe to
/// run on any worker thread, and the router would not build. Writing `async fn` in the trait
/// looks tidier and silently loses the guarantee; the implementations may still use `async fn`.
pub trait UserRepository: Send + Sync + 'static {
    fn get_user_from_id(
        &self,
        user_id: &UserId,
    ) -> impl Future<Output = Result<Option<User>, BoxError>> + Send;

    /// The account an identity provider's identifier belongs to, if any.
    fn get_user_from_external_id(
        &self,
        external_id: &str,
    ) -> impl Future<Output = Result<Option<User>, BoxError>> + Send;

    fn get_user_from_username(
        &self,
        username: &str,
    ) -> impl Future<Output = Result<Option<User>, BoxError>> + Send;

    fn add_user(&self, user: &User) -> impl Future<Output = Result<UserId, BoxError>> + Send;

    fn update_user(
        &self,
        user_id: &UserId,
        user: &User,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// Record that an account signed in, without writing back anything else it holds.
    ///
    /// A sign-in reads the account to check the password, and checking a password takes long
    /// enough for an administrator to change the same account in between. Writing the whole
    /// account back from that read would undo their answer - a disabled account re-enabled, a
    /// permission change or a session invalidation rolled back. This is the write a sign-in makes:
    /// the read of `last_login` and the write of it are one step.
    fn record_login(&self, user_id: &UserId, at: chrono::DateTime<chrono::Utc>) -> impl Future<Output = Result<(), BoxError>> + Send;

    fn get_all_users(&self) -> impl Future<Output = Result<Vec<(UserId, User)>, BoxError>> + Send;

    fn delete_user(&self, user_id: &UserId) -> impl Future<Output = Result<(), BoxError>> + Send;
}
