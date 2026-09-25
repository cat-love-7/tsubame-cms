//! Account management against Cognito's admin API.
//!
//! The account screen is the only place an administrator manages accounts, and on this deployment
//! it has to move two things that have to agree: the record here (what someone may do) and the
//! user at Cognito (whether they can sign in at all). `AuthService` asks this side first, so a
//! refusal from Cognito leaves no half-created account behind.
//!
//! Two consequences of the credential living at Cognito rather than here:
//!
//! * **A reset means a temporary password.** The CMS cannot choose a password for an account whose
//!   credential it does not hold, so `AdminSetUserPassword` sets one that is *not* permanent and
//!   the provider makes the person change it before the sign-in completes - the hosted page handles
//!   that challenge itself. The administrator copies the value out of the account screen and passes
//!   it on, which is what they do with a reset link on premises.
//! * **Nothing is mailed.** `AdminCreateUser` runs with `MessageAction::Suppress`: the pool has no
//!   verified address and this deployment sends no mail, so the invitation Cognito would normally
//!   email would simply never arrive. The account exists, and its password comes from the reset
//!   above, chosen by the administrator.
//!
//! The Cognito calls sit behind [`CognitoAdmin`] because **there is no Cognito emulator** to point
//! them at (`docs/aws-decisions.md`): what is worth testing is the mapping - which call, with which
//! arguments, and which refusals mean "that was already the case" - and that is what the tests here
//! drive with a fake. The calls themselves are thin enough to read, and a deployment is where they
//! meet the real service.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aws_sdk_cognitoidentityprovider::Client;
use aws_sdk_cognitoidentityprovider::operation::admin_create_user::AdminCreateUserError;
use aws_sdk_cognitoidentityprovider::operation::admin_delete_user::AdminDeleteUserError;
use aws_sdk_cognitoidentityprovider::operation::admin_disable_user::AdminDisableUserError;
use aws_sdk_cognitoidentityprovider::types::{AttributeType, MessageActionType};
use tsubame_core::auth::provisioner::{AccountProvisioner, NewAccount, ProvisionFuture};

/// The four Cognito calls account management needs, as one seam.
///
/// `username` is the pool's username, which is what the CMS stores as an account's name; the pool
/// signs people in by name (`username_attributes = []` in `infra/cognito.tf`), so the two are the
/// same string and nothing has to be mapped.
pub trait CognitoAdmin: Send + Sync + 'static {
    /// Create the user, or find it if it is already there, and answer with its `sub` - the value
    /// a token from this pool carries and the CMS resolves an account by.
    fn create_user<'a>(
        &'a self,
        username: &'a str,
        email: Option<&'a str>,
    ) -> AdminFuture<'a, Option<String>>;
    /// Remove the user. Already gone is success too.
    fn delete_user<'a>(&'a self, username: &'a str) -> AdminFuture<'a, ()>;
    /// Give the user a password they have to change before the sign-in completes.
    fn set_temporary_password<'a>(
        &'a self,
        username: &'a str,
        password: &'a str,
    ) -> AdminFuture<'a, ()>;
    /// Whether the user may sign in at all.
    fn set_enabled<'a>(&'a self, username: &'a str, enabled: bool) -> AdminFuture<'a, ()>;
}

/// Boxed for the same reason [`ProvisionFuture`] is: the client is the SDK's, and the tests' is
/// not.
pub type AdminFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// The CMS's account management, backed by a Cognito user pool.
pub struct CognitoAccountProvisioner {
    admin: Arc<dyn CognitoAdmin>,
}

impl CognitoAccountProvisioner {
    /// The real one: the pool, through the SDK.
    pub fn new(client: Client, user_pool_id: impl Into<String>) -> Self {
        CognitoAccountProvisioner {
            admin: Arc::new(CognitoApi {
                client,
                user_pool_id: user_pool_id.into(),
            }),
        }
    }

    /// The same mapping over a stand-in, which is how the tests below reach it.
    pub fn with_admin(admin: Arc<dyn CognitoAdmin>) -> Self {
        CognitoAccountProvisioner { admin }
    }
}

impl AccountProvisioner for CognitoAccountProvisioner {
    fn create<'a>(&'a self, account: &'a NewAccount) -> ProvisionFuture<'a, Option<String>> {
        Box::pin(async move {
            self.admin
                .create_user(&account.username, account.email.as_deref())
                .await
        })
    }

    fn delete<'a>(&'a self, username: &'a str) -> ProvisionFuture<'a, ()> {
        Box::pin(async move { self.admin.delete_user(username).await })
    }

    fn reset_password<'a>(&'a self, username: &'a str) -> ProvisionFuture<'a, String> {
        Box::pin(async move {
            let password = temporary_password();
            self.admin
                .set_temporary_password(username, &password)
                .await?;
            Ok(password)
        })
    }

    fn set_active<'a>(&'a self, username: &'a str, active: bool) -> ProvisionFuture<'a, ()> {
        Box::pin(async move { self.admin.set_enabled(username, active).await })
    }
}

/// A temporary password the pool accepts and a person can type once.
///
/// `infra/cognito.tf` sets the minimum length and leaves the four character classes at the
/// provider's defaults - upper, lower, digit and symbol - so this takes three characters of each.
/// A UUID's own text would fail three of those four rules (it is lowercase hex), which is why it is
/// the source of randomness here rather than the value.
pub fn temporary_password() -> String {
    const LOWER: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const DIGITS: &[u8] = b"23456789";
    const SYMBOLS: &[u8] = b"!#%+*?";

    uuid::Uuid::new_v4()
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            let alphabet = match index % 4 {
                0 => LOWER,
                1 => UPPER,
                2 => DIGITS,
                _ => SYMBOLS,
            };
            alphabet[*byte as usize % alphabet.len()] as char
        })
        .collect()
}

/// Whether a refusal means the user is already there.
///
/// Separate from the call so the decision can be tested without the service, which is the one
/// thing about this module that cannot be run locally.
fn already_exists(error: &AdminCreateUserError) -> bool {
    matches!(error, AdminCreateUserError::UsernameExistsException(_))
}

/// The same for a delete: the user is gone, which is what the caller wanted.
fn already_gone(error: &AdminDeleteUserError) -> bool {
    matches!(error, AdminDeleteUserError::UserNotFoundException(_))
}

/// Ids and messages differ per call, and the caller only ever logs this.
fn describe<E: std::fmt::Debug + std::fmt::Display>(error: E) -> String {
    format!("{error}")
}

/// Cognito's own name for a user: the `sub` attribute, which is what a token from the pool carries
/// as the subject and what the CMS resolves an account by.
fn subject_of(attributes: &[AttributeType]) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.name() == "sub")
        .and_then(|attribute| attribute.value())
        .map(str::to_string)
}

/// The SDK, which is the only implementation that talks to anything.
struct CognitoApi {
    client: Client,
    user_pool_id: String,
}

impl CognitoApi {
    /// What the pool already calls this user, for the case where the create found it there.
    async fn user_id(&self, username: &str) -> Result<Option<String>, String> {
        self.client
            .admin_get_user()
            .user_pool_id(&self.user_pool_id)
            .username(username)
            .send()
            .await
            .map(|response| subject_of(response.user_attributes()))
            .map_err(describe)
    }
}

impl CognitoAdmin for CognitoApi {
    fn create_user<'a>(
        &'a self,
        username: &'a str,
        email: Option<&'a str>,
    ) -> AdminFuture<'a, Option<String>> {
        Box::pin(async move {
            let mut request = self
                .client
                .admin_create_user()
                .user_pool_id(&self.user_pool_id)
                .username(username)
                // See the module docs: no mail is sent, so no message is asked for.
                .message_action(MessageActionType::Suppress);
            if let Some(email) = email {
                let attribute = AttributeType::builder()
                    .name("email")
                    .value(email)
                    .build()
                    .map_err(describe)?;
                request = request.user_attributes(attribute);
            }
            match request.send().await {
                Ok(response) => Ok(response
                    .user()
                    .and_then(|user| subject_of(user.attributes()))),
                // Already there: still not a failure, but its identifier has to be found rather
                // than skipped, or the record the CMS is about to write could never sign in.
                Err(error) => match error.as_service_error() {
                    Some(service) if already_exists(service) => self.user_id(username).await,
                    _ => Err(describe(error)),
                },
            }
        })
    }

    fn delete_user<'a>(&'a self, username: &'a str) -> AdminFuture<'a, ()> {
        Box::pin(async move {
            match self
                .client
                .admin_delete_user()
                .user_pool_id(&self.user_pool_id)
                .username(username)
                .send()
                .await
            {
                Ok(_) => Ok(()),
                Err(error) => match error.as_service_error() {
                    Some(service) if already_gone(service) => Ok(()),
                    _ => Err(describe(error)),
                },
            }
        })
    }

    fn set_temporary_password<'a>(
        &'a self,
        username: &'a str,
        password: &'a str,
    ) -> AdminFuture<'a, ()> {
        Box::pin(async move {
            // No tolerance here, unlike the two above: an account the pool does not have is an
            // administrator about to hand over a password that cannot work, and that has to be
            // said rather than swallowed.
            self.client
                .admin_set_user_password()
                .user_pool_id(&self.user_pool_id)
                .username(username)
                .password(password)
                .permanent(false)
                .send()
                .await
                .map(|_| ())
                .map_err(describe)
        })
    }

    fn set_enabled<'a>(&'a self, username: &'a str, enabled: bool) -> AdminFuture<'a, ()> {
        Box::pin(async move {
            if enabled {
                // Enabling an account the pool does not have is *not* the outcome asked for, so
                // this one reports it.
                self.client
                    .admin_enable_user()
                    .user_pool_id(&self.user_pool_id)
                    .username(username)
                    .send()
                    .await
                    .map(|_| ())
                    .map_err(describe)
            } else {
                match self
                    .client
                    .admin_disable_user()
                    .user_pool_id(&self.user_pool_id)
                    .username(username)
                    .send()
                    .await
                {
                    Ok(_) => Ok(()),
                    // An account the pool does not have cannot sign in, which is what disabling
                    // it was for.
                    Err(error)
                        if matches!(
                            error.as_service_error(),
                            Some(AdminDisableUserError::UserNotFoundException(_))
                        ) =>
                    {
                        Ok(())
                    }
                    Err(error) => Err(describe(error)),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// What the tests below drive: every call written down, and a way to refuse.
    #[derive(Default)]
    struct FakeCognito {
        calls: Mutex<Vec<String>>,
        refuse: bool,
    }

    impl FakeCognito {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) -> Result<(), String> {
            self.calls.lock().unwrap().push(call);
            if self.refuse {
                Err("the pool said no".to_string())
            } else {
                Ok(())
            }
        }
    }

    impl CognitoAdmin for FakeCognito {
        fn create_user<'a>(
            &'a self,
            username: &'a str,
            email: Option<&'a str>,
        ) -> AdminFuture<'a, Option<String>> {
            Box::pin(async move {
                self.record(format!("create {username} email={}", email.unwrap_or("-")))?;
                // What the pool answers with: its own name for the account, which the CMS keeps.
                Ok(Some(format!("sub-of-{username}")))
            })
        }

        fn delete_user<'a>(&'a self, username: &'a str) -> AdminFuture<'a, ()> {
            Box::pin(async move { self.record(format!("delete {username}")) })
        }

        fn set_temporary_password<'a>(
            &'a self,
            username: &'a str,
            password: &'a str,
        ) -> AdminFuture<'a, ()> {
            Box::pin(async move { self.record(format!("temporary {username} {password}")) })
        }

        fn set_enabled<'a>(&'a self, username: &'a str, enabled: bool) -> AdminFuture<'a, ()> {
            Box::pin(async move { self.record(format!("enabled {username} {enabled}")) })
        }
    }

    fn provisioner(fake: Arc<FakeCognito>) -> CognitoAccountProvisioner {
        CognitoAccountProvisioner::with_admin(fake)
    }

    #[tokio::test]
    async fn the_pool_is_told_everything_the_cms_knows() {
        let fake = Arc::new(FakeCognito::default());
        let accounts = provisioner(fake.clone());

        accounts
            .create(&NewAccount {
                username: "cat".to_string(),
                email: Some("cat@example.com".to_string()),
            })
            .await
            .expect("the fake agrees");
        accounts
            .create(&NewAccount {
                username: "dog".to_string(),
                email: None,
            })
            .await
            .expect("the fake agrees");
        accounts.delete("cat").await.expect("the fake agrees");
        accounts
            .set_active("cat", false)
            .await
            .expect("the fake agrees");
        accounts
            .set_active("cat", true)
            .await
            .expect("the fake agrees");

        assert_eq!(
            fake.calls(),
            vec![
                "create cat email=cat@example.com",
                "create dog email=-",
                "delete cat",
                "enabled cat false",
                "enabled cat true",
            ]
        );
    }

    #[tokio::test]
    async fn creating_an_account_answers_with_the_pools_own_name_for_it() {
        let fake = Arc::new(FakeCognito::default());
        let accounts = provisioner(fake.clone());

        let external_id = accounts
            .create(&NewAccount {
                username: "cat".to_string(),
                email: None,
            })
            .await
            .expect("the fake agrees");

        // The value the CMS keeps, and the only one a token from this pool can be resolved by.
        assert_eq!(external_id.as_deref(), Some("sub-of-cat"));
    }

    #[tokio::test]
    async fn a_reset_sets_the_password_it_hands_back() {
        let fake = Arc::new(FakeCognito::default());
        let accounts = provisioner(fake.clone());

        let password = accounts
            .reset_password("cat")
            .await
            .expect("the fake agrees");

        assert_eq!(
            fake.calls(),
            vec![format!("temporary cat {password}")],
            "the value the pool was given is the value the administrator gets"
        );
    }

    #[tokio::test]
    async fn a_refusal_never_looks_like_a_password_to_hand_over() {
        let fake = Arc::new(FakeCognito {
            refuse: true,
            ..FakeCognito::default()
        });
        let accounts = provisioner(fake);

        let refused = accounts.reset_password("cat").await;
        assert!(refused.is_err(), "nothing was set, so nothing is returned");
    }

    #[test]
    fn a_temporary_password_satisfies_every_rule_the_pool_checks() {
        // `infra/cognito.tf`: at least eight characters, and the provider's defaults for the four
        // classes. A run of these is what would notice a generator that drifted into hex.
        for _ in 0..32 {
            let password = temporary_password();
            assert!(password.len() >= 8, "{password} is too short");
            assert!(
                password.chars().any(|c| c.is_ascii_lowercase())
                    && password.chars().any(|c| c.is_ascii_uppercase())
                    && password.chars().any(|c| c.is_ascii_digit())
                    && password.chars().any(|c| !c.is_ascii_alphanumeric()),
                "{password} is missing a class"
            );
            assert!(
                password.chars().all(|c| c.is_ascii_graphic()),
                "{password} has something untypeable in it"
            );
        }
    }

    #[test]
    fn the_two_refusals_that_mean_already_done_are_recognised() {
        let exists = AdminCreateUserError::UsernameExistsException(
            aws_sdk_cognitoidentityprovider::types::error::UsernameExistsException::builder()
                .build(),
        );
        assert!(already_exists(&exists));

        let gone = AdminDeleteUserError::UserNotFoundException(
            aws_sdk_cognitoidentityprovider::types::error::UserNotFoundException::builder().build(),
        );
        assert!(already_gone(&gone));
    }
}
