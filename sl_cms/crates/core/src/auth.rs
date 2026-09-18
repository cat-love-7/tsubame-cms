//! Authentication: credential verification, token issuing, and the user lifecycle.

pub mod cognito;
pub mod identity;
pub mod provisioner;
pub mod throttle;
pub mod token;

use std::sync::Arc;
use std::time::Instant;

use crate::auth::identity::{Identity, TokenVerifier};
use crate::auth::provisioner::{AccountProvisioner, NewAccount};
use crate::models::error::HttpError;
use crate::models::user::{
    LoginResponse, NewAccountRequest, PasswordChangedResponse, Permission, UpdateUserRequest, User,
    UserId, UserResponse, is_plausible_email, is_plausible_username, normalize_username,
};
use crate::password_reset::{PasswordResetError, PasswordResetIssuer, PasswordResetLink};
use crate::repositories::local_credentials::LocalCredentials;
use crate::repositories::user_repository::UserRepository;
use throttle::LoginThrottle;
use token::TokenIssuer;

/// Shortest password accepted when creating a user.
pub const MIN_PASSWORD_LENGTH: usize = 8;

/// Message returned for both "unknown account" and "wrong password" so that the endpoint
/// cannot be used to enumerate accounts.
const BAD_CREDENTIALS: &str = "invalid username or password";

pub struct AuthService<R: UserRepository> {
    repository: Arc<R>,
    issuer: TokenIssuer,
    /// Who a bearer token belongs to. On-premises this is the issuer; a deployment that uses an
    /// identity provider installs that provider's verifier instead.
    verifier: Arc<dyn TokenVerifier>,
    /// Usernames (or addresses) an operator named in advance, allowed to become the first
    /// administrators by signing in. Empty where the deployment creates its first account with
    /// `ADMIN_USERNAME` / `ADMIN_PASSWORD`.
    bootstrap_admins: Vec<String>,
    /// The identity provider's side of account management, where one owns the credential.
    provisioner: Option<Arc<dyn AccountProvisioner>>,
    /// Mints the links an administrator hands out for an account to set its own password.
    password_resets: PasswordResetIssuer,
    /// Counts failed sign-ins per account, so guessing a password is not free.
    throttle: LoginThrottle,
}

impl<R: UserRepository> AuthService<R> {
    pub fn new(
        repository: Arc<R>,
        issuer: TokenIssuer,
        password_resets: PasswordResetIssuer,
    ) -> Self {
        AuthService {
            repository,
            verifier: Arc::new(issuer.clone()) as Arc<dyn TokenVerifier>,
            bootstrap_admins: Vec::new(),
            provisioner: None,
            issuer,
            password_resets,
            throttle: LoginThrottle::new(),
        }
    }
    pub async fn has_any_user(&self) -> Result<bool, HttpError> {
        Ok(!self
            .repository
            .list_users()
            .await
            .map_err(internal)?
            .is_empty())
    }
    /// Resolve a bearer token to the (active) user it belongs to.
    /// Install the verifier for a deployment whose tokens are not this CMS's.
    pub fn with_external_verifier(mut self, verifier: Arc<dyn TokenVerifier>) -> Self {
        self.verifier = verifier;
        self
    }

    /// The people an operator named in advance, who may become administrators by signing in.
    pub fn with_bootstrap_admins(mut self, usernames: Vec<String>) -> Self {
        self.bootstrap_admins = usernames;
        self
    }

    /// Let the deployment's identity provider look after the accounts themselves.
    pub fn with_account_provisioner(mut self, provisioner: Arc<dyn AccountProvisioner>) -> Self {
        self.provisioner = Some(provisioner);
        self
    }

    /// Create an account, asking the identity provider first when one is configured.
    ///
    /// No credential is chosen here. An account signs in either through a provider, or with a
    /// password **its owner set** by following a reset link - never with one an administrator
    /// typed. The single exception is [`AuthService::bootstrap_admin`], which exists because a
    /// deployment needs a way in before anyone can hand out links.
    pub async fn create_user(&self, request: NewAccountRequest) -> Result<UserResponse, HttpError> {
        let username = normalize_username(&request.username);
        if !is_plausible_username(&username) {
            return Err(HttpError::BadRequest(
                "a username of up to 128 letters, digits or + = , . @ _ - is required",
            )
            .with_code("invalid_username"));
        }
        // The address is optional; when it is given it has to look like one.
        let email = request
            .email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map(|email| {
                if is_plausible_email(email) {
                    Ok(email.to_ascii_lowercase())
                } else {
                    Err(HttpError::BadRequest(
                        "that is not a plausible email address",
                    ))
                }
            })
            .transpose()?;
        if self
            .repository
            .get_user_from_username(&username)
            .await
            .map_err(internal)?
            .is_some()
        {
            return Err(
                HttpError::Conflict("a user with that username already exists")
                    .with_code("username_taken"),
            );
        }

        if let Some(provisioner) = &self.provisioner {
            let account = NewAccount {
                username: username.clone(),
                email: email.clone(),
            };
            provisioner.create(&account).await.map_err(|e| {
                // The provider's reason is worth logging, but not worth handing to a client.
                tracing::warn!("could not create {username} at the identity provider: {e}");
                HttpError::InternalServerError("the identity provider refused the account")
            })?;
        }

        let permission = if request.is_admin {
            Permission::admin()
        } else {
            request.permission
        };
        let mut user = User::new(&username, request.is_admin, permission);
        user.email = email;
        self.repository.add_user(&user).await.map_err(internal)?;
        Ok(user.to_response())
    }
    pub async fn user_from_token(&self, token: &str) -> Result<User, HttpError> {
        match self.verifier.verify(token).await? {
            Identity::Local {
                user_id,
                token_version,
            } => {
                let user = self
                    .repository
                    .get_user_from_id(&user_id)
                    .await
                    .map_err(internal)?
                    .ok_or_else(|| {
                        HttpError::Unauthorized("invalid or expired token")
                            .with_code("invalid_token")
                    })?;

                // A password change ends every session that was issued before it. The check is
                // against the stored account, so it holds however long the token has left.
                if token_version != user.token_version {
                    return Err(HttpError::Unauthorized(
                        "this session ended when the password changed",
                    )
                    .with_code("session_ended"));
                }
                if !user.is_active {
                    return Err(
                        HttpError::Forbidden("account is disabled").with_code("account_disabled")
                    );
                }
                Ok(user)
            }
            Identity::External {
                external_id,
                username,
                email,
            } => {
                self.user_from_external_identity(external_id, username, email)
                    .await
            }
        }
    }

    /// Resolve an identity provider's account to a local one, creating it when an operator
    /// named this person in advance.
    ///
    /// The provider says who someone is; it has no idea what they may do here. So the first
    /// sign-in of a name on the bootstrap list creates an administrator record bound to that
    /// provider identifier, and every sign-in after that resolves through it. Anyone else is
    /// refused until an administrator creates their account — a provider that anyone in the
    /// organisation can sign in to must not mean anyone can edit the site.
    async fn user_from_external_identity(
        &self,
        external_id: String,
        username: String,
        email: Option<String>,
    ) -> Result<User, HttpError> {
        if let Some(user) = self
            .repository
            .get_user_from_external_id(&external_id)
            .await
            .map_err(internal)?
        {
            if !user.is_active {
                return Err(
                    HttpError::Forbidden("account is disabled").with_code("account_disabled")
                );
            }
            return Ok(user);
        }

        let wanted = normalize_username(&username);
        let address = email.as_deref().map(normalize_username);
        let allowed = self
            .bootstrap_admins
            .iter()
            .any(|name| *name == wanted || Some(name) == address.as_ref());
        if !allowed {
            return Err(HttpError::Forbidden(
                "this account has not been given access to the CMS; an administrator has to \
                 create it first",
            )
            .with_code("not_provisioned"));
        }

        let mut user = User::new(&username, true, Permission::admin());
        user.external_id = Some(external_id);
        user.email = email;
        self.repository.add_user(&user).await.map_err(internal)?;
        tracing::info!("provisioned an administrator on first sign-in: {username}");
        Ok(user)
    }
    /// Change an account's role, or whether it may sign in at all.
    ///
    /// Refuses to leave the CMS without an active administrator, so the last one cannot be
    /// demoted or disabled — by another administrator or by themselves.
    pub async fn update_user(
        &self,
        id: &UserId,
        request: UpdateUserRequest,
    ) -> Result<UserResponse, HttpError> {
        let mut user = self.require_user(id).await?;
        // Only losing an administrator has to be guarded; a viewer staying a viewer is not
        // what would leave the CMS without one.
        let was_active_admin = user.is_admin && user.is_active;

        if let Some(is_admin) = request.is_admin {
            user.is_admin = is_admin;
        }
        if let Some(is_active) = request.is_active {
            user.is_active = is_active;
        }
        if let Some(permission) = request.permission {
            user.permission = permission;
        }
        // The maps are replaced whole when they are given, so an administrator can take an
        // override back as well as add one.
        if let Some(collection_permissions) = request.collection_permissions {
            user.collection_permissions = collection_permissions;
        }
        if let Some(single_page_permissions) = request.single_page_permissions {
            user.single_page_permissions = single_page_permissions;
        }

        if was_active_admin && !(user.is_admin && user.is_active) {
            self.ensure_another_active_admin(id).await?;
        }

        self.repository
            .update_user(id, &user)
            .await
            .map_err(internal)?;
        Ok(user.to_response())
    }
    /// Delete an account, refusing to remove the last active administrator.
    pub async fn delete_user(&self, id: &UserId) -> Result<(), HttpError> {
        let user = self.require_user(id).await?;
        if user.is_admin && user.is_active {
            self.ensure_another_active_admin(id).await?;
        }
        // The provider first, so a refusal leaves the CMS exactly as it was rather than with a
        // record whose account can still sign in.
        if let Some(provisioner) = &self.provisioner {
            provisioner.delete(&user.username).await.map_err(|e| {
                tracing::warn!(
                    "could not remove {} at the identity provider: {e}",
                    user.username
                );
                HttpError::InternalServerError(
                    "the identity provider refused to remove the account",
                )
            })?;
        }
        self.repository.delete_user(id).await.map_err(internal)
    }
    async fn require_user(&self, id: &UserId) -> Result<User, HttpError> {
        self.repository
            .get_user_from_id(id)
            .await
            .map_err(internal)?
            .ok_or_else(|| HttpError::NotFound(&format!("user '{id}' does not exist")))
    }
    /// At least one active administrator has to remain, or nobody could manage accounts or
    /// change the shape of the site again.
    async fn ensure_another_active_admin(&self, excluding: &UserId) -> Result<(), HttpError> {
        let others = self
            .repository
            .list_users()
            .await
            .map_err(internal)?
            .into_iter()
            .filter(|(id, user)| id != excluding && user.is_admin && user.is_active)
            .count();
        if others == 0 {
            return Err(HttpError::Conflict(
                "the last administrator cannot be demoted, disabled or deleted",
            )
            .with_code("last_administrator"));
        }
        Ok(())
    }
    pub async fn list_users(&self) -> Result<Vec<UserResponse>, HttpError> {
        let mut users: Vec<User> = self
            .repository
            .list_users()
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(_, user)| user)
            .collect();
        users.sort_by(|a, b| a.email.cmp(&b.email));
        Ok(users.iter().map(User::to_response).collect())
    }
}

/// The same service, for a deployment that stores passwords itself.
///
/// Nothing here exists where sign-in belongs to an identity provider: there is no password to
/// verify and no credential to set, so the endpoints that call these are not part of that
/// backend's router either (see [`LocalCredentials`](crate::repositories::local_credentials::LocalCredentials)).
impl<R: UserRepository + LocalCredentials> AuthService<R> {
    /// Verify credentials and issue a token.
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse, HttpError> {
        let username = normalize_username(username);
        // Checked before the password is even looked at, and for unknown identifiers too, so
        // the limiter cannot be used to find out which accounts exist.
        if let Some(wait) = self.throttle.retry_after(&username, Instant::now()) {
            return Err(HttpError::TooManyRequests(wait));
        }

        let user = self
            .repository
            .get_user_from_username(&username)
            .await
            .map_err(internal)?;

        let Some(mut user) = user else {
            self.throttle.record_failure(&username, Instant::now());
            return Err(HttpError::Unauthorized(BAD_CREDENTIALS).with_code("invalid_credentials"));
        };

        if !self
            .repository
            .verify_password(&user.id, password)
            .await
            .map_err(internal)?
        {
            self.throttle.record_failure(&username, Instant::now());
            return Err(HttpError::Unauthorized(BAD_CREDENTIALS).with_code("invalid_credentials"));
        }
        if !user.is_active {
            return Err(HttpError::Forbidden("account is disabled").with_code("account_disabled"));
        }
        self.throttle.record_success(&username);

        // Recording the login is a side effect; a failure here must not deny a valid login, so it
        // is logged and swallowed. It is its own write: the account was read before the password
        // was checked, and writing the whole of it back would undo an administrator's change made
        // while that was happening (see `UserRepository::record_login`).
        let now = chrono::Utc::now();
        user.last_login = Some(now);
        if let Err(e) = self.repository.record_login(&user.id, now).await {
            tracing::warn!("failed to record last_login for {}: {e}", user.username);
        }

        let (token, expires_at) = self.issuer.issue(&user).map_err(internal)?;
        Ok(LoginResponse {
            token,
            expires_at,
            user: user.to_response(),
        })
    }
    /// Issue a link that lets one account set a new password, for an administrator to pass on.
    ///
    /// Nothing is mailed: the administrator decides how the link reaches its owner, which is
    /// the only thing that works for an account with no address on file.
    pub async fn issue_password_reset(&self, id: &UserId) -> Result<PasswordResetLink, HttpError> {
        let user = self.require_user(id).await?;
        // A disabled account cannot sign in, so a link for it would only mislead.
        if !user.is_active {
            return Err(HttpError::Forbidden("account is disabled").with_code("account_disabled"));
        }
        Ok(self.password_resets.issue(&user, chrono::Utc::now()))
    }
    /// Set a new password using an issued link, and sign the caller in with the result.
    ///
    /// Completing this ends every session that exists (see
    /// [`AuthService::change_own_password`]), which is also what makes the link single use: the
    /// token carries the generation it was issued for, and the account moves on.
    pub async fn complete_password_reset(
        &self,
        token: &str,
        new_password: &str,
    ) -> Result<PasswordChangedResponse, HttpError> {
        let claims = self
            .password_resets
            .verify(token, chrono::Utc::now())
            .map_err(|error| match error {
                PasswordResetError::Expired => HttpError::Forbidden(error.message()),
                PasswordResetError::AlreadyUsed => HttpError::Forbidden(error.message()),
                PasswordResetError::Malformed | PasswordResetError::Invalid => {
                    // The same answer for every unusable token: a stranger learns nothing
                    // about which accounts exist.
                    HttpError::Unauthorized(PasswordResetError::Invalid.message())
                }
            })?;

        // An unknown account is answered exactly like an unusable token: this endpoint is
        // public, so it must not confirm which accounts exist.
        let user = self
            .repository
            .get_user_from_id(&claims.user_id)
            .await
            .map_err(internal)?;
        let Some(mut user) = user else {
            return Err(HttpError::Unauthorized(
                PasswordResetError::Invalid.message(),
            ));
        };
        // Guessing a token is no easier than guessing a password, but the attempt is counted
        // the same way so neither can be hammered.
        if let Some(wait) = self.throttle.retry_after(&user.username, Instant::now()) {
            return Err(HttpError::TooManyRequests(wait));
        }
        // The link was issued for one generation of this account; anything that changed the
        // password since (this link being used once already included) makes it refuse.
        if claims.token_version != user.token_version {
            self.throttle.record_failure(&user.username, Instant::now());
            return Err(HttpError::Forbidden(
                PasswordResetError::AlreadyUsed.message(),
            ));
        }
        if !user.is_active {
            return Err(HttpError::Forbidden("account is disabled").with_code("account_disabled"));
        }
        validate_password(new_password)?;

        self.repository
            .set_password(&user.id, new_password)
            .await
            .map_err(internal)?;
        user.end_existing_sessions();
        self.repository
            .update_user(&user.id, &user)
            .await
            .map_err(internal)?;
        self.throttle.record_success(&user.username);

        let (token, expires_at) = self.issuer.issue(&user).map_err(internal)?;
        Ok(PasswordChangedResponse { token, expires_at })
    }
    /// Set a password for an account, ending every session it has.
    ///
    /// Called by the two paths that are allowed to choose a password for someone: a reset link the
    /// person completed themselves, and the initial account the deployment bootstraps from its
    /// configuration. There is deliberately no administrator "set their password" route - an
    /// administrator hands over a reset link instead, so the password is never known to anyone but
    /// its owner. The caller keeps their own session: the version belongs to the account whose
    /// password changed, not to whoever changed it.
    pub async fn set_password(&self, id: &UserId, password: &str) -> Result<(), HttpError> {
        let mut user = self.require_user(id).await?;
        validate_password(password)?;
        self.repository
            .set_password(id, password)
            .await
            .map_err(internal)?;
        user.end_existing_sessions();
        self.repository
            .update_user(id, &user)
            .await
            .map_err(internal)
    }
    /// Change your own password, proving you know the current one so that a stolen session
    /// is not enough to lock the owner out.
    ///
    /// This ends every existing session, the caller's included - that is the point, since
    /// a password change is how someone reacts to a leaked token. A token for the new
    /// generation comes back so the caller is not thrown out of the screen they are on.
    pub async fn change_own_password(
        &self,
        id: &UserId,
        current: &str,
        new: &str,
    ) -> Result<PasswordChangedResponse, HttpError> {
        let mut user = self.require_user(id).await?;
        // The current password is guessed the same way a login is, so it is counted the
        // same way too.
        if let Some(wait) = self.throttle.retry_after(&user.username, Instant::now()) {
            return Err(HttpError::TooManyRequests(wait));
        }
        if !self
            .repository
            .verify_password(&user.id, current)
            .await
            .map_err(internal)?
        {
            self.throttle.record_failure(&user.username, Instant::now());
            return Err(HttpError::Forbidden("current password is incorrect"));
        }
        validate_password(new)?;
        self.throttle.record_success(&user.username);
        self.repository
            .set_password(id, new)
            .await
            .map_err(internal)?;
        user.end_existing_sessions();
        self.repository
            .update_user(id, &user)
            .await
            .map_err(internal)?;

        let (token, expires_at) = self.issuer.issue(&user).map_err(internal)?;
        Ok(PasswordChangedResponse { token, expires_at })
    }
    /// Create the first administrator if the user store is still empty.
    ///
    /// Refuses to start with an empty store and no credentials, because the alternative
    /// is exposing an unauthenticated CMS.
    pub async fn bootstrap_admin(
        &self,
        username: Option<&str>,
        password: Option<&str>,
        email: Option<&str>,
    ) -> Result<Option<UserResponse>, HttpError> {
        if self.has_any_user().await? {
            return Ok(None);
        }
        let missing = || {
            HttpError::InternalServerError(
                "no users exist yet: set ADMIN_USERNAME and ADMIN_PASSWORD to create the initial administrator",
            )
        };
        let password = password.ok_or_else(missing)?;
        let request = NewAccountRequest {
            username: username.ok_or_else(missing)?.to_string(),
            email: email.map(str::to_string),
            is_admin: true,
            permission: Permission::admin(),
        };
        let created = self.create_user(request).await?;
        // The one account whose password is chosen by someone other than its owner: nobody can sign
        // in yet, so nobody could follow a reset link. If the credential cannot be stored, the
        // record goes with it - otherwise the next start would find a user, skip the bootstrap, and
        // leave a deployment that nobody can enter.
        let id = UserId::from(created.id.as_str());
        if let Err(failure) = self.set_password(&id, password).await {
            let _ = self.repository.delete_user(&id).await;
            return Err(failure);
        }
        Ok(Some(created))
    }
}

fn validate_password(password: &str) -> Result<(), HttpError> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(HttpError::BadRequest(&format!(
            "password must be at least {MIN_PASSWORD_LENGTH} characters"
        ))
        .with_code("weak_password"));
    }
    Ok(())
}

/// The same 500 as everywhere else: the detail is logged, not answered (see
/// [`crate::models::error::map_internal_error`]).
fn internal<E: std::fmt::Display>(e: E) -> HttpError {
    crate::models::error::map_internal_error(e)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use super::*;
    use crate::auth::identity::VerifyFuture;
    use crate::auth::provisioner::{AccountProvisioner, NewAccount, ProvisionFuture};
    use crate::models::user::NewAccountRequest;
    use crate::repositories::user_repository::BoxError;

    #[derive(Default)]
    struct InMemoryUsers {
        users: RwLock<HashMap<String, User>>,
        /// Passwords in the clear: this is a test double, and the adapter that does the real
        /// hashing has its own tests.
        passwords: RwLock<HashMap<String, String>>,
        /// The same for the provider-identifier index.
        external_ids: RwLock<HashMap<String, String>>,
        /// A store that refuses to keep a credential, for the failure the bootstrap has to survive.
        refuse_passwords: bool,
        /// Every account written whole. A sign-in must not do that: it records one field (see
        /// `UserRepository::record_login`), and this is how a test sees the difference.
        whole_writes: std::sync::atomic::AtomicUsize,
    }

    impl LocalCredentials for InMemoryUsers {
        async fn set_password(&self, user_id: &UserId, password: &str) -> Result<(), BoxError> {
            if self.refuse_passwords {
                return Err("the credential store is unavailable".into());
            }
            self.passwords
                .write()
                .unwrap()
                .insert(user_id.to_string(), password.to_string());
            Ok(())
        }
        async fn verify_password(
            &self,
            user_id: &UserId,
            password: &str,
        ) -> Result<bool, BoxError> {
            Ok(self
                .passwords
                .read()
                .unwrap()
                .get(&user_id.to_string())
                .is_some_and(|stored| stored == password))
        }
        async fn delete_password(&self, user_id: &UserId) -> Result<(), BoxError> {
            self.passwords.write().unwrap().remove(&user_id.to_string());
            Ok(())
        }
    }

    impl UserRepository for InMemoryUsers {
        async fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
            Ok(self
                .users
                .read()
                .unwrap()
                .get(&user_id.to_string())
                .cloned())
        }
        async fn get_user_from_external_id(
            &self,
            external_id: &str,
        ) -> Result<Option<User>, BoxError> {
            let id = self.external_ids.read().unwrap().get(external_id).cloned();
            match id {
                Some(id) => self.get_user_from_id(&UserId::from(id.as_str())).await,
                None => Ok(None),
            }
        }
        async fn get_user_from_username(&self, username: &str) -> Result<Option<User>, BoxError> {
            let username = normalize_username(username);
            Ok(self
                .users
                .read()
                .unwrap()
                .values()
                .find(|u| u.username == username)
                .cloned())
        }
        async fn add_user(&self, user: &User) -> Result<UserId, BoxError> {
            self.users
                .write()
                .unwrap()
                .insert(user.id.to_string(), user.clone());
            if let Some(external_id) = &user.external_id {
                self.external_ids
                    .write()
                    .unwrap()
                    .insert(external_id.clone(), user.id.to_string());
            }
            Ok(user.id.clone())
        }
        async fn record_login(
            &self,
            user_id: &UserId,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), BoxError> {
            // Only the one field, on the record as it is now: the whole point of the method (see
            // `UserRepository::record_login`).
            let mut users = self.users.write().unwrap();
            let Some(user) = users.get_mut(&user_id.to_string()) else {
                return Err("user not found".into());
            };
            user.last_login = Some(at);
            Ok(())
        }
        async fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError> {
            self.whole_writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.users
                .write()
                .unwrap()
                .insert(user_id.to_string(), user.clone());
            Ok(())
        }
        async fn list_users(&self) -> Result<Vec<(UserId, User)>, BoxError> {
            Ok(self
                .users
                .read()
                .unwrap()
                .values()
                .map(|u| (u.id.clone(), u.clone()))
                .collect())
        }
        async fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError> {
            self.users.write().unwrap().remove(&user_id.to_string());
            Ok(())
        }
    }

    fn service() -> (AuthService<InMemoryUsers>, Arc<InMemoryUsers>) {
        let repo = Arc::new(InMemoryUsers::default());
        let password_resets = PasswordResetIssuer::new(b"test-secret", 30);
        (
            AuthService::new(
                repo.clone(),
                TokenIssuer::new(b"test-secret", 1),
                password_resets,
            ),
            repo,
        )
    }

    /// An account with a password, in the two steps a deployment takes: create it, which is all the
    /// API does and leaves it without a credential, then set one, as the bootstrap or a completed
    /// reset does.
    async fn account_with_password(
        auth: &AuthService<InMemoryUsers>,
        username: &str,
        password: &str,
        is_admin: bool,
    ) -> UserResponse {
        let request = NewAccountRequest {
            username: username.to_string(),
            email: None,
            is_admin,
            permission: if is_admin {
                Permission::admin()
            } else {
                Permission::default()
            },
        };
        let created = auth.create_user(request).await.unwrap();
        auth.set_password(&UserId::from(created.id.as_str()), password)
            .await
            .unwrap();
        created
    }

    /// A sign-in records when it happened, and nothing else about the account.
    ///
    /// The account is read before the password is checked, and checking one takes long enough for
    /// an administrator to disable it in between: writing the whole account back from that read
    /// would undo that. The write is its own, and only touches `last_login`.
    #[tokio::test]
    async fn a_login_does_not_write_the_account_back() {
        let repository = Arc::new(InMemoryUsers::default());
        let auth = AuthService::new(
            repository.clone(),
            TokenIssuer::new(b"test-secret", 1),
            PasswordResetIssuer::new(b"test-secret", 30),
        );
        let created = account_with_password(&auth, "ops", "supersecret", false).await;
        // Creating the account writes it whole; the sign-in is what must not.
        let writes_before = repository
            .whole_writes
            .load(std::sync::atomic::Ordering::Relaxed);

        auth.login("ops", "supersecret").await.unwrap();

        assert_eq!(
            repository
                .whole_writes
                .load(std::sync::atomic::Ordering::Relaxed),
            writes_before,
            "a sign-in must not write the whole account: that would undo an admin change made \
             while the password was being checked"
        );
        let stored = repository
            .get_user_from_id(&UserId::from(created.id.as_str()))
            .await
            .unwrap()
            .expect("the account");
        assert!(stored.last_login.is_some(), "the sign-in is still recorded");
    }

    #[tokio::test]
    async fn login_succeeds_with_correct_credentials() {
        let (auth, _) = service();
        account_with_password(&auth, "Alice@Example.com", "supersecret", true).await;

        let response = auth
            .login("alice@example.com", "supersecret")
            .await
            .unwrap();
        assert_eq!(response.user.username, "alice@example.com");
        assert!(response.user.email.is_none(), "メールは必須ではない");
        assert!(response.user.is_admin);
        assert!(response.expires_at > chrono::Utc::now());

        // The issued token resolves back to the same user.
        let user = auth.user_from_token(&response.token).await.unwrap();
        assert_eq!(user.username, "alice@example.com");
    }

    /// Guessing a password is not free: after a few failures the account waits - even when
    /// the password is finally right - while other addresses carry on as before.
    #[tokio::test]
    async fn repeated_sign_in_failures_are_refused_for_a_while() {
        let (auth, _) = service();
        account_with_password(&auth, "a@example.com", "supersecret", false).await;

        // Two failures and then a success: the count starts over, so the next four are
        // ordinary rejections rather than a lock.
        assert!(auth.login("a@example.com", "wrong").await.is_err());
        assert!(auth.login("a@example.com", "wrong").await.is_err());
        assert!(auth.login("a@example.com", "supersecret").await.is_ok());
        for attempt in 1..=4 {
            assert_eq!(
                auth.login("a@example.com", "wrong")
                    .await
                    .unwrap_err()
                    .status_code,
                401,
                "{attempt} 回目はまだ普通の拒否"
            );
        }

        // The fifth failure trips the limit, and the right password now waits too.
        assert!(auth.login("a@example.com", "wrong").await.is_err());
        let locked = auth
            .login("a@example.com", "supersecret")
            .await
            .unwrap_err();
        assert_eq!(locked.status_code, 429);
        assert!(locked.message.contains("try again"), "{}", locked.message);

        // An address that does not exist is counted the same way, so the limiter itself
        // cannot be used to find out which accounts exist.
        for _ in 0..5 {
            assert_eq!(
                auth.login("ghost@example.com", "wrong")
                    .await
                    .unwrap_err()
                    .status_code,
                401
            );
        }
        assert_eq!(
            auth.login("ghost@example.com", "wrong")
                .await
                .unwrap_err()
                .status_code,
            429
        );

        // And a different address is still just wrong.
        assert_eq!(
            auth.login("b@example.com", "wrong")
                .await
                .unwrap_err()
                .status_code,
            401
        );
    }

    /// An administrator can hand out a link instead of choosing someone else's password; using
    /// it sets the account's own password and ends every session that came before.
    #[tokio::test]
    async fn a_reset_link_sets_the_password_once_and_then_refuses() {
        let (auth, _) = service();
        let account = account_with_password(&auth, "ops", "old-password", false).await;
        let id = UserId::from(account.id.to_string().as_str());

        let link = auth.issue_password_reset(&id).await.unwrap();
        assert!(link.expires_at > chrono::Utc::now());

        // An old session, to watch it die.
        let old_token = auth.login("ops", "old-password").await.unwrap().token;
        assert!(auth.user_from_token(&old_token).await.is_ok());

        let changed = auth
            .complete_password_reset(&link.token, "chosen-by-the-owner")
            .await
            .unwrap();

        // The password is the owner's, the caller is signed in, old sessions are gone...
        assert!(auth.login("ops", "old-password").await.is_err());
        assert!(auth.login("ops", "chosen-by-the-owner").await.is_ok());
        assert_eq!(
            auth.user_from_token(&old_token)
                .await
                .unwrap_err()
                .status_code,
            401
        );
        assert!(auth.user_from_token(&changed.token).await.is_ok());

        // ...and the link cannot be used twice.
        let again = auth
            .complete_password_reset(&link.token, "another-password")
            .await
            .unwrap_err();
        assert_eq!(again.status_code, 403, "{}", again.message);
        // The password the owner chose still stands.
        assert!(auth.login("ops", "chosen-by-the-owner").await.is_ok());
    }

    /// The public completion endpoint must not confirm which accounts exist.
    #[tokio::test]
    async fn a_link_for_an_unknown_account_is_refused_like_any_other_bad_token() {
        let (auth, _) = service();
        let account = User::new("ops", false, Permission::viewer());
        let link = PasswordResetIssuer::new(b"test-secret", 30).issue(&account, chrono::Utc::now());

        let error = auth
            .complete_password_reset(&link.token, "new-password")
            .await
            .unwrap_err();
        assert_eq!(error.status_code, 401);
        // A garbage token gets the same answer.
        assert_eq!(
            auth.complete_password_reset("not-a-token", "new-password")
                .await
                .unwrap_err()
                .status_code,
            401
        );
    }

    #[tokio::test]
    async fn a_disabled_account_gets_no_reset_link() {
        let (auth, _) = service();
        let admin = account_with_password(&auth, "admin", "admin-password", true).await;
        let account = account_with_password(&auth, "ops", "ops-password", false).await;

        auth.update_user(
            &UserId::from(account.id.to_string().as_str()),
            UpdateUserRequest {
                is_active: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let error = auth
            .issue_password_reset(&UserId::from(account.id.to_string().as_str()))
            .await
            .unwrap_err();
        assert_eq!(error.status_code, 403);
        // The administrator's own account still gets one.
        assert!(
            auth.issue_password_reset(&UserId::from(admin.id.to_string().as_str()))
                .await
                .is_ok()
        );
    }

    /// Changing your own password ends every session, including the one that asked for the
    /// change - and hands that caller a token for the new generation so they stay signed in.
    #[tokio::test]
    async fn changing_your_own_password_ends_the_sessions_that_came_before() {
        let (auth, _) = service();
        let created = account_with_password(&auth, "a@example.com", "old-password", false).await;

        let stolen = auth
            .login("a@example.com", "old-password")
            .await
            .unwrap()
            .token;
        assert!(auth.user_from_token(&stolen).await.is_ok());

        let changed = auth
            .change_own_password(&created.id, "old-password", "new-password")
            .await
            .unwrap();

        // The token that existed before the change is dead...
        assert_eq!(
            auth.user_from_token(&stolen).await.unwrap_err().status_code,
            401
        );
        // ...the caller's replacement works...
        assert!(auth.user_from_token(&changed.token).await.is_ok());
        assert!(changed.expires_at > chrono::Utc::now());
        // ...and the credentials are the new ones.
        assert!(auth.login("a@example.com", "old-password").await.is_err());
        assert!(auth.login("a@example.com", "new-password").await.is_ok());
    }

    /// An administrator resetting someone else's password ends that account's sessions and
    /// leaves the administrator's own session alone.
    #[tokio::test]
    async fn resetting_another_accounts_password_only_ends_that_accounts_sessions() {
        let (auth, _) = service();
        account_with_password(&auth, "admin@example.com", "admin-password", true).await;
        let target =
            account_with_password(&auth, "editor@example.com", "editor-password", false).await;

        let admin_token = auth
            .login("admin@example.com", "admin-password")
            .await
            .unwrap()
            .token;
        let editor_token = auth
            .login("editor@example.com", "editor-password")
            .await
            .unwrap()
            .token;

        auth.set_password(&target.id, "reset-password")
            .await
            .unwrap();

        assert_eq!(
            auth.user_from_token(&editor_token)
                .await
                .unwrap_err()
                .status_code,
            401
        );
        assert!(
            auth.user_from_token(&admin_token).await.is_ok(),
            "自分のセッションは残る"
        );
    }

    #[tokio::test]
    async fn login_failures_are_indistinguishable() {
        let (auth, _) = service();
        account_with_password(&auth, "a@example.com", "supersecret", false).await;

        let unknown = auth
            .login("nobody@example.com", "supersecret")
            .await
            .unwrap_err();
        let wrong = auth
            .login("a@example.com", "wrong-password")
            .await
            .unwrap_err();
        assert_eq!(unknown.status_code, 401);
        assert_eq!(wrong.status_code, 401);
        assert_eq!(unknown.message, wrong.message);
    }

    #[tokio::test]
    async fn rejects_short_passwords_and_unusable_usernames() {
        let (auth, _) = service();
        // A weak password is refused where a password is chosen, which is no longer creation: the
        // account exists first, and its owner (or the bootstrap) sets the credential.
        let created = auth.create_user(account("ops")).await.unwrap();
        assert_eq!(
            auth.set_password(&UserId::from(created.id.as_str()), "short")
                .await
                .unwrap_err()
                .status_code,
            400
        );
        // A name without an address is fine now; one that cannot be typed as an identifier is
        // not.
        for username in ["", "with space", "with:colon"] {
            assert_eq!(
                auth.create_user(account(username))
                    .await
                    .unwrap_err()
                    .status_code,
                400,
                "{username:?}"
            );
        }
        // An address, when one is given, still has to look like one.
        let mut request = account("ops@example.com");
        request.email = Some("not-an-email".to_string());
        assert_eq!(
            auth.create_user(request).await.unwrap_err().status_code,
            400
        );
    }

    #[tokio::test]
    async fn rejects_duplicate_usernames_regardless_of_case() {
        let (auth, _) = service();
        auth.create_user(account("a@example.com")).await.unwrap();
        let err = auth
            .create_user(account("A@EXAMPLE.COM"))
            .await
            .unwrap_err();
        assert_eq!(err.status_code, 409);
    }

    #[tokio::test]
    async fn token_for_a_deleted_user_is_rejected() {
        let (auth, repo) = service();
        let created = account_with_password(&auth, "a@example.com", "supersecret", false).await;
        let token = auth
            .login("a@example.com", "supersecret")
            .await
            .unwrap()
            .token;

        repo.delete_user(&created.id).await.unwrap();
        assert_eq!(
            auth.user_from_token(&token).await.unwrap_err().status_code,
            401
        );
    }

    #[tokio::test]
    async fn disabled_account_cannot_log_in_and_its_token_is_rejected() {
        let (auth, repo) = service();
        let created = account_with_password(&auth, "a@example.com", "supersecret", false).await;
        let token = auth
            .login("a@example.com", "supersecret")
            .await
            .unwrap()
            .token;

        {
            let mut users = repo.users.write().unwrap();
            let user = users.get_mut(&created.id.to_string()).unwrap();
            user.is_active = false;
        }

        assert_eq!(
            auth.login("a@example.com", "supersecret")
                .await
                .unwrap_err()
                .status_code,
            403
        );
        assert_eq!(
            auth.user_from_token(&token).await.unwrap_err().status_code,
            403
        );
    }

    #[tokio::test]
    async fn the_last_administrator_cannot_lock_the_cms_out() {
        let (auth, _) = service();
        let admin = account_with_password(&auth, "admin@example.com", "supersecret", true).await;
        let editor = account_with_password(&auth, "editor@example.com", "supersecret", false).await;

        // The only administrator cannot stop being one, stop being active, or be deleted.
        assert_eq!(
            auth.update_user(
                &admin.id,
                UpdateUserRequest {
                    is_admin: Some(false),
                    ..Default::default()
                }
            )
            .await
            .unwrap_err()
            .status_code,
            409
        );
        assert_eq!(
            auth.update_user(
                &admin.id,
                UpdateUserRequest {
                    is_active: Some(false),
                    ..Default::default()
                }
            )
            .await
            .unwrap_err()
            .status_code,
            409
        );
        assert_eq!(
            auth.delete_user(&admin.id).await.unwrap_err().status_code,
            409
        );

        // With a second administrator the demotion is allowed...
        let second = account_with_password(&auth, "second@example.com", "supersecret", true).await;
        assert!(
            !auth
                .update_user(
                    &admin.id,
                    UpdateUserRequest {
                        is_admin: Some(false),
                        ..Default::default()
                    }
                )
                .await
                .unwrap()
                .is_admin
        );

        // ...and then the remaining administrator is the protected one.
        assert_eq!(
            auth.delete_user(&second.id).await.unwrap_err().status_code,
            409
        );
        // The demoted account and a plain editor can be removed.
        auth.delete_user(&admin.id).await.unwrap();
        auth.delete_user(&editor.id).await.unwrap();
        assert_eq!(auth.list_users().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn roles_and_passwords_can_be_changed() {
        let (auth, _) = service();
        let viewer = account_with_password(&auth, "viewer@example.com", "supersecret", false).await;

        // A role change is exactly what the new permission says.
        let updated = auth
            .update_user(
                &viewer.id,
                UpdateUserRequest {
                    permission: Some(Permission::editor()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(updated.permission.can_edit);
        assert!(!updated.permission.can_publish);
        assert!(updated.is_active);

        // Disabling keeps the account but stops it signing in.
        auth.update_user(
            &viewer.id,
            UpdateUserRequest {
                is_active: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            auth.login("viewer@example.com", "supersecret")
                .await
                .unwrap_err()
                .status_code,
            403
        );
        auth.update_user(
            &viewer.id,
            UpdateUserRequest {
                is_active: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Your own password needs the current one.
        assert_eq!(
            auth.change_own_password(&viewer.id, "wrong", "newsupersecret")
                .await
                .unwrap_err()
                .status_code,
            403
        );
        auth.change_own_password(&viewer.id, "supersecret", "newsupersecret")
            .await
            .unwrap();
        assert!(
            auth.login("viewer@example.com", "newsupersecret")
                .await
                .is_ok()
        );

        // An administrator reset does not, but the minimum length still applies.
        assert_eq!(
            auth.set_password(&viewer.id, "short")
                .await
                .unwrap_err()
                .status_code,
            400
        );
        auth.set_password(&viewer.id, "resetpassword")
            .await
            .unwrap();
        assert!(
            auth.login("viewer@example.com", "resetpassword")
                .await
                .is_ok()
        );

        // A change to an account that does not exist is a 404.
        let missing = UserId::from("00000000-0000-0000-0000-000000000000");
        assert_eq!(
            auth.set_password(&missing, "supersecret")
                .await
                .unwrap_err()
                .status_code,
            404
        );
    }

    /// A provisioner that writes down what it was asked to do, and can refuse.
    #[derive(Default)]
    struct RecordingProvisioner {
        created: std::sync::Mutex<Vec<String>>,
        deleted: std::sync::Mutex<Vec<String>>,
        refuse: bool,
    }

    impl AccountProvisioner for RecordingProvisioner {
        fn create<'a>(&'a self, account: &'a NewAccount) -> ProvisionFuture<'a, ()> {
            Box::pin(async move {
                if self.refuse {
                    return Err("no".to_string());
                }
                self.created.lock().unwrap().push(account.username.clone());
                Ok(())
            })
        }
        fn delete<'a>(&'a self, username: &'a str) -> ProvisionFuture<'a, ()> {
            Box::pin(async move {
                if self.refuse {
                    return Err("no".to_string());
                }
                self.deleted.lock().unwrap().push(username.to_string());
                Ok(())
            })
        }
    }

    fn account(username: &str) -> NewAccountRequest {
        NewAccountRequest {
            username: username.to_string(),
            email: None,
            is_admin: false,
            permission: Permission::viewer(),
        }
    }

    /// Where an identity provider owns the credential, an account is two records that have to
    /// agree — and the provider is asked first, so a refusal leaves nothing behind.
    #[tokio::test]
    async fn creating_an_account_asks_the_provider_first() {
        let (auth, repository) = service();
        let provisioner = Arc::new(RecordingProvisioner::default());
        let auth = auth.with_account_provisioner(provisioner.clone());

        let created = auth.create_user(account("ops@example.com")).await.unwrap();
        assert_eq!(created.username, "ops@example.com");
        assert_eq!(
            provisioner.created.lock().unwrap().as_slice(),
            ["ops@example.com".to_string()]
        );
        assert_eq!(repository.users.read().unwrap().len(), 1);

        // The provider refuses: no record is written, so there is no account that cannot sign
        // in and no half-created user to clean up.
        let (refusing_auth, refusing_store) = service();
        let refusing_auth =
            refusing_auth.with_account_provisioner(Arc::new(RecordingProvisioner {
                refuse: true,
                ..Default::default()
            }));
        assert_eq!(
            refusing_auth
                .create_user(account("nope@example.com"))
                .await
                .unwrap_err(),
            HttpError::InternalServerError("the identity provider refused the account")
        );
        assert!(refusing_store.users.read().unwrap().is_empty());

        // Deleting asks the provider, so the account cannot sign in afterwards either.
        let user = repository
            .get_user_from_username("ops@example.com")
            .await
            .unwrap()
            .expect("the account");
        auth.delete_user(&user.id).await.unwrap();
        assert_eq!(
            provisioner.deleted.lock().unwrap().as_slice(),
            ["ops@example.com".to_string()]
        );
        assert!(repository.users.read().unwrap().is_empty());
    }

    /// A verifier that always says the same thing, so the *resolution* is what is under test.
    struct FixedVerifier(Identity);

    impl TokenVerifier for FixedVerifier {
        fn verify<'a>(&'a self, _token: &'a str) -> VerifyFuture<'a> {
            let identity = self.0.clone();
            Box::pin(async move { Ok(identity) })
        }
    }

    fn external(external_id: &str, username: &str) -> Arc<FixedVerifier> {
        Arc::new(FixedVerifier(Identity::External {
            external_id: external_id.to_string(),
            username: username.to_string(),
            email: None,
        }))
    }

    /// What an identity provider's token means here: an account resolved by its identifier, or
    /// created when an operator named the person in advance — and nothing else.
    #[tokio::test]
    async fn a_provider_identity_is_resolved_through_external_id() {
        // Two services over one store: neither can be cloned, and both have to see the same
        // accounts for the assertions to mean anything.
        let repository = Arc::new(InMemoryUsers::default());
        let build = |repository: Arc<InMemoryUsers>| {
            AuthService::new(
                repository,
                TokenIssuer::new(b"test-secret", 1),
                PasswordResetIssuer::new(b"test-secret", 30),
            )
        };
        let stranger = build(repository.clone())
            .with_external_verifier(external("sub-2", "stranger@example.com"));

        // Unknown and not named in advance: refused, not created. A pool anyone in the
        // organisation can sign in to must not mean anyone can edit the site.
        // Asserted by status and code rather than by the sentence: the wording is allowed to
        // change, the code is what a client keys its own wording off.
        let refused = stranger.user_from_token("token").await.unwrap_err();
        assert_eq!(
            (refused.status_code, refused.code),
            (403, "not_provisioned")
        );
        assert!(repository.users.read().unwrap().is_empty());

        // Named in advance: the first sign-in creates the administrator, bound to the
        // provider's identifier.
        let allowed = build(repository.clone())
            .with_external_verifier(external("sub-1", "ops@example.com"))
            .with_bootstrap_admins(vec!["ops@example.com".to_string()]);
        let user = allowed.user_from_token("token").await.unwrap();
        assert!(user.is_admin && user.is_active);
        assert_eq!(user.external_id.as_deref(), Some("sub-1"));
        assert_eq!(user.username, "ops@example.com");
        assert_eq!(repository.users.read().unwrap().len(), 1);

        // The second sign-in resolves through the index rather than creating another account.
        let again = allowed.user_from_token("token").await.unwrap();
        assert_eq!(again.id, user.id);
        assert_eq!(repository.users.read().unwrap().len(), 1);

        // A disabled account is refused even though its identity resolves.
        let mut disabled = user.clone();
        disabled.is_active = false;
        repository.update_user(&user.id, &disabled).await.unwrap();
        assert!(allowed.user_from_token("token").await.is_err());

        // And somebody else entirely still gets nowhere, even with the list in place.
        assert!(stranger.user_from_token("token").await.is_err());
    }

    #[tokio::test]
    async fn bootstrap_requires_credentials_only_when_the_store_is_empty() {
        let (auth, _) = service();
        // Empty store and no credentials -> refuse.
        assert_eq!(
            auth.bootstrap_admin(None, None, None)
                .await
                .unwrap_err()
                .status_code,
            500
        );

        let created = auth
            .bootstrap_admin(Some("ops"), Some("supersecret"), Some("ops@example.com"))
            .await
            .unwrap()
            .expect("admin should have been created");
        assert!(created.is_admin);
        assert_eq!(created.username, "ops");
        assert_eq!(created.email.as_deref(), Some("ops@example.com"));

        // Once seeded, bootstrap is a no-op even without credentials.
        assert!(
            auth.bootstrap_admin(None, None, None)
                .await
                .unwrap()
                .is_none()
        );
    }

    /// A bootstrap that cannot store the credential must not leave the account behind: the next
    /// start would find a user, skip the bootstrap, and leave a deployment nobody can enter.
    #[tokio::test]
    async fn a_bootstrap_that_cannot_store_its_password_leaves_no_account() {
        let repository = Arc::new(InMemoryUsers {
            refuse_passwords: true,
            ..Default::default()
        });
        let auth = AuthService::new(
            repository.clone(),
            TokenIssuer::new(b"test-secret", 1),
            PasswordResetIssuer::new(b"test-secret", 30),
        );

        assert!(
            auth.bootstrap_admin(Some("ops"), Some("supersecret"), None)
                .await
                .is_err()
        );
        assert!(
            repository.users.read().unwrap().is_empty(),
            "the account has to go with the credential it could not keep"
        );
    }
}
