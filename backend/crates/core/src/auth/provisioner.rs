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
//! clean up by hand. The same order holds for the rest of the trait - a reset the provider refuses
//! must not be reported as done, and an account disabled here while the provider still lets it in
//! is an account whose owner is told "no" one screen later than necessary.

use std::future::Future;
use std::pin::Pin;

/// The account as the provider needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    /// The sign-in name, already normalized the way this CMS stores it.
    pub username: String,
    pub email: Option<String>,
}

/// What the provider did when it was asked for an account.
///
/// The difference matters to the caller: an account the provider **created** has no credential of
/// its own yet (the CMS asks for one without a password), so an administrator has to hand over a
/// way in - while an account that was already there has its owner's own credential, and handing
/// over a new one would take that away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionedAccount {
    /// The provider's own identifier for the account, where the provider has one.
    pub external_id: Option<String>,
    /// Whether this call created the account, rather than finding one that already existed.
    pub created: bool,
}

/// Boxed for the same reason [`TokenVerifier`](crate::auth::identity::TokenVerifier) is: the
/// deployment picks its provisioner at composition time.
pub type ProvisionFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Creating, removing and re-credentialing accounts at the identity provider.
pub trait AccountProvisioner: Send + Sync + 'static {
    /// Create the account, or make sure it is already there, and answer with the provider's own
    /// identifier for it and whether it had to be created.
    ///
    /// That identifier is what the CMS has to keep: a token from the provider resolves to a local
    /// record *by it* (see `AuthService::user_from_external_identity`), so an account created here
    /// without it would exist in both places and still be refused at the door. Where the provider
    /// has no such notion, `None` is the honest answer.
    ///
    /// An account that already exists is not a failure: an operator may have created it in the
    /// provider's console first, and the CMS's job is then only to record what it may do - but its
    /// identifier is just as necessary, so it is looked up rather than skipped, and `created` says
    /// which of the two happened.
    fn create<'a>(&'a self, account: &'a NewAccount) -> ProvisionFuture<'a, ProvisionedAccount>;

    /// Remove the account.
    ///
    /// A missing account is not a failure either, so deleting a user twice is one outcome.
    fn delete<'a>(&'a self, username: &'a str) -> ProvisionFuture<'a, ()>;

    /// Give the account a new password, and hand it back for the administrator to pass on.
    ///
    /// The provider sets it as a temporary one that its owner has to change before the next
    /// sign-in completes, which is the whole reason an administrator may touch a password here:
    /// it is a value the person cannot keep.
    fn reset_password<'a>(&'a self, username: &'a str) -> ProvisionFuture<'a, String>;

    /// Whether the account may sign in at all.
    ///
    /// The CMS refuses an inactive account on every request, so this is not what protects the API;
    /// it stops the account signing in at the provider and being told afterwards that it may not
    /// come in. Called only when the value actually changes.
    fn set_active<'a>(&'a self, username: &'a str, active: bool) -> ProvisionFuture<'a, ()>;
}
