//! The identity provider's side of account management.
//!
//! Where the CMS stores the password itself, creating an account *is* writing a record. Where an
//! identity provider does, creating the account means two things that have to agree: a user in
//! the provider (so they can sign in at all) and a record here (so the CMS knows what they may
//! do). This is the first half; [`UserRepository`](crate::repositories::user_repository::UserRepository)
//! is the second.
//!
//! The CMS asks the provider **first**: a provider that refuses leaves nothing behind, whereas a
//! record written first would be an account that cannot sign in and that an administrator has to
//! clean up by hand.

use std::future::Future;
use std::pin::Pin;

/// The account as the provider needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    /// The sign-in name, already normalized the way this CMS stores it.
    pub username: String,
    pub email: Option<String>,
}

/// Boxed for the same reason [`TokenVerifier`](crate::auth::identity::TokenVerifier) is: the
/// deployment picks its provisioner at composition time.
pub type ProvisionFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Creating and removing accounts at the identity provider.
pub trait AccountProvisioner: Send + Sync + 'static {
    /// Create the account, or make sure it is already there.
    ///
    /// An account that exists is not a failure: an operator may have created it in the provider
    /// console first, and the CMS's job is then only to record what it may do.
    fn create<'a>(&'a self, account: &'a NewAccount) -> ProvisionFuture<'a, ()>;

    /// Remove the account.
    ///
    /// A missing account is not a failure either, so deleting a user twice is one outcome.
    fn delete<'a>(&'a self, username: &'a str) -> ProvisionFuture<'a, ()>;
}
