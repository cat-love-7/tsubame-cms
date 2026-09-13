//! Authentication: credential verification, token issuing, and the user lifecycle.

pub mod password;
pub mod throttle;
pub mod token;

use std::sync::Arc;
use std::time::Instant;

use crate::models::error::HttpError;
use crate::models::user::{
    is_plausible_email, normalize_email, LoginResponse, NewUserRequest, PasswordChangedResponse,
    Permission, UpdateUserRequest, User, UserId, UserResponse,
};
use crate::repositories::user_repository::UserRepository;
use throttle::LoginThrottle;
use token::TokenIssuer;

/// Shortest password accepted when creating a user.
pub const MIN_PASSWORD_LENGTH: usize = 8;

/// Message returned for both "unknown email" and "wrong password" so that the endpoint
/// cannot be used to enumerate accounts.
const BAD_CREDENTIALS: &str = "invalid email or password";

pub struct AuthService<R: UserRepository> {
    repository: Arc<R>,
    issuer: TokenIssuer,
    /// Counts failed sign-ins per account, so guessing a password is not free.
    throttle: LoginThrottle,
}

impl<R: UserRepository> AuthService<R> {
    pub fn new(repository: Arc<R>, issuer: TokenIssuer) -> Self {
        AuthService {
            repository,
            issuer,
            throttle: LoginThrottle::new(),
        }
    }

    pub fn has_any_user(&self) -> Result<bool, HttpError> {
        Ok(!self
            .repository
            .get_all_users()
            .map_err(internal)?
            .is_empty())
    }

    /// Verify credentials and issue a token.
    pub fn login(&self, email: &str, password: &str) -> Result<LoginResponse, HttpError> {
        let email = normalize_email(email);
        // Checked before the password is even looked at, and for unknown addresses too, so
        // the limiter cannot be used to find out which accounts exist.
        if let Some(wait) = self.throttle.retry_after(&email, Instant::now()) {
            return Err(HttpError::TooManyRequests(wait));
        }

        let user = self
            .repository
            .get_user_from_email(&email)
            .map_err(internal)?;

        let Some(mut user) = user else {
            self.throttle.record_failure(&email, Instant::now());
            return Err(HttpError::Unauthorized(BAD_CREDENTIALS));
        };

        if !password::verify_password(password, &user.password_hash) {
            self.throttle.record_failure(&email, Instant::now());
            return Err(HttpError::Unauthorized(BAD_CREDENTIALS));
        }
        if !user.is_active {
            return Err(HttpError::Forbidden("account is disabled"));
        }
        self.throttle.record_success(&email);

        // Recording the login is a side effect; a failure here must not deny a valid
        // login, so it is logged and swallowed.
        user.last_login = Some(chrono::Utc::now());
        if let Err(e) = self.repository.update_user(&user.id, &user) {
            tracing::warn!("failed to record last_login for {}: {e}", user.email);
        }

        let (token, expires_at) = self.issuer.issue(&user).map_err(internal)?;
        Ok(LoginResponse {
            token,
            expires_at,
            user: user.to_response(),
        })
    }

    /// Resolve a bearer token to the (active) user it belongs to.
    pub fn user_from_token(&self, token: &str) -> Result<User, HttpError> {
        let claims = self.issuer.verify(token).map_err(|e| {
            // Do not echo the verifier's reason to the client.
            tracing::debug!("rejected token: {e}");
            HttpError::Unauthorized("invalid or expired token")
        })?;
        let user_id = UserId::from(claims.sub.as_str());

        let user = self
            .repository
            .get_user_from_id(&user_id)
            .map_err(internal)?
            .ok_or_else(|| HttpError::Unauthorized("invalid or expired token"))?;

        // A password change ends every session that was issued before it. The check is
        // against the stored account, so it holds however long the token has left to run.
        if claims.ver != user.token_version {
            return Err(HttpError::Unauthorized(
                "this session ended when the password changed",
            ));
        }

        if !user.is_active {
            return Err(HttpError::Forbidden("account is disabled"));
        }
        Ok(user)
    }

    pub fn create_user(&self, request: NewUserRequest) -> Result<UserResponse, HttpError> {
        let email = normalize_email(&request.email);
        if !is_plausible_email(&email) {
            return Err(HttpError::BadRequest("a valid email address is required"));
        }
        validate_password(&request.password)?;
        if self
            .repository
            .get_user_from_email(&email)
            .map_err(internal)?
            .is_some()
        {
            return Err(HttpError::Conflict("a user with that email already exists"));
        }

        let hash = password::hash_password(&request.password).map_err(internal)?;
        let permission = if request.is_admin {
            Permission::admin()
        } else {
            request.permission
        };
        let user = User::new(&email, hash, request.is_admin, permission);
        self.repository.add_user(&user).map_err(internal)?;
        Ok(user.to_response())
    }

    /// Change an account's role, or whether it may sign in at all.
    ///
    /// Refuses to leave the CMS without an active administrator, so the last one cannot be
    /// demoted or disabled — by another administrator or by themselves.
    pub fn update_user(
        &self,
        id: &UserId,
        request: UpdateUserRequest,
    ) -> Result<UserResponse, HttpError> {
        let mut user = self.require_user(id)?;
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
            self.ensure_another_active_admin(id)?;
        }

        self.repository.update_user(id, &user).map_err(internal)?;
        Ok(user.to_response())
    }

    /// Delete an account, refusing to remove the last active administrator.
    pub fn delete_user(&self, id: &UserId) -> Result<(), HttpError> {
        let user = self.require_user(id)?;
        if user.is_admin && user.is_active {
            self.ensure_another_active_admin(id)?;
        }
        self.repository.delete_user(id).map_err(internal)
    }

    /// Set someone else's password (an administrator resetting an account).
    /// Reset someone else's password, ending every session they have.
    ///
    /// The caller keeps their own session: the version belongs to the account whose
    /// password changed, not to whoever changed it.
    pub fn set_password(&self, id: &UserId, password: &str) -> Result<(), HttpError> {
        let mut user = self.require_user(id)?;
        validate_password(password)?;
        user.password_hash = password::hash_password(password).map_err(internal)?;
        user.end_existing_sessions();
        self.repository.update_user(id, &user).map_err(internal)
    }

    /// Change your own password, proving you know the current one so that a stolen session
    /// is not enough to lock the owner out.
    ///
    /// This ends every existing session, the caller's included - that is the point, since
    /// a password change is how someone reacts to a leaked token. A token for the new
    /// generation comes back so the caller is not thrown out of the screen they are on.
    pub fn change_own_password(
        &self,
        id: &UserId,
        current: &str,
        new: &str,
    ) -> Result<PasswordChangedResponse, HttpError> {
        let mut user = self.require_user(id)?;
        // The current password is guessed the same way a login is, so it is counted the
        // same way too.
        if let Some(wait) = self.throttle.retry_after(&user.email, Instant::now()) {
            return Err(HttpError::TooManyRequests(wait));
        }
        if !password::verify_password(current, &user.password_hash) {
            self.throttle.record_failure(&user.email, Instant::now());
            return Err(HttpError::Forbidden("current password is incorrect"));
        }
        validate_password(new)?;
        self.throttle.record_success(&user.email);
        user.password_hash = password::hash_password(new).map_err(internal)?;
        user.end_existing_sessions();
        self.repository.update_user(id, &user).map_err(internal)?;

        let (token, expires_at) = self.issuer.issue(&user).map_err(internal)?;
        Ok(PasswordChangedResponse { token, expires_at })
    }

    fn require_user(&self, id: &UserId) -> Result<User, HttpError> {
        self.repository
            .get_user_from_id(id)
            .map_err(internal)?
            .ok_or_else(|| HttpError::NotFound(&format!("user '{id}' does not exist")))
    }

    /// At least one active administrator has to remain, or nobody could manage accounts or
    /// change the shape of the site again.
    fn ensure_another_active_admin(&self, excluding: &UserId) -> Result<(), HttpError> {
        let others = self
            .repository
            .get_all_users()
            .map_err(internal)?
            .into_iter()
            .filter(|(id, user)| id != excluding && user.is_admin && user.is_active)
            .count();
        if others == 0 {
            return Err(HttpError::Conflict(
                "the last administrator cannot be demoted, disabled or deleted",
            ));
        }
        Ok(())
    }

    pub fn list_users(&self) -> Result<Vec<UserResponse>, HttpError> {
        let mut users: Vec<User> = self
            .repository
            .get_all_users()
            .map_err(internal)?
            .into_iter()
            .map(|(_, user)| user)
            .collect();
        users.sort_by(|a, b| a.email.cmp(&b.email));
        Ok(users.iter().map(User::to_response).collect())
    }

    /// Create the first administrator if the user store is still empty.
    ///
    /// Refuses to start with an empty store and no credentials, because the alternative
    /// is exposing an unauthenticated CMS.
    pub fn bootstrap_admin(
        &self,
        email: Option<&str>,
        password: Option<&str>,
    ) -> Result<Option<UserResponse>, HttpError> {
        if self.has_any_user()? {
            return Ok(None);
        }
        let missing = || {
            HttpError::InternalServerError(
                "no users exist yet: set ADMIN_EMAIL and ADMIN_PASSWORD to create the initial administrator",
            )
        };
        let request = NewUserRequest {
            email: email.ok_or_else(missing)?.to_string(),
            password: password.ok_or_else(missing)?.to_string(),
            is_admin: true,
            permission: Permission::admin(),
        };
        self.create_user(request).map(Some)
    }
}

fn validate_password(password: &str) -> Result<(), HttpError> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(HttpError::BadRequest(&format!(
            "password must be at least {MIN_PASSWORD_LENGTH} characters"
        )));
    }
    Ok(())
}

fn internal<E: std::fmt::Display>(e: E) -> HttpError {
    HttpError::InternalServerError(&e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use super::*;
    use crate::repositories::user_repository::BoxError;

    #[derive(Default)]
    struct InMemoryUsers {
        users: RwLock<HashMap<String, User>>,
    }

    impl UserRepository for InMemoryUsers {
        fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
            Ok(self.users.read().unwrap().get(&user_id.to_string()).cloned())
        }
        fn get_user_from_email(&self, email: &str) -> Result<Option<User>, BoxError> {
            let email = normalize_email(email);
            Ok(self
                .users
                .read()
                .unwrap()
                .values()
                .find(|u| u.email == email)
                .cloned())
        }
        fn add_user(&self, user: &User) -> Result<UserId, BoxError> {
            self.users
                .write()
                .unwrap()
                .insert(user.id.to_string(), user.clone());
            Ok(user.id.clone())
        }
        fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError> {
            self.users
                .write()
                .unwrap()
                .insert(user_id.to_string(), user.clone());
            Ok(())
        }
        fn get_all_users(&self) -> Result<Vec<(UserId, User)>, BoxError> {
            Ok(self
                .users
                .read()
                .unwrap()
                .values()
                .map(|u| (u.id.clone(), u.clone()))
                .collect())
        }
        fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError> {
            self.users.write().unwrap().remove(&user_id.to_string());
            Ok(())
        }
    }

    fn service() -> (AuthService<InMemoryUsers>, Arc<InMemoryUsers>) {
        let repo = Arc::new(InMemoryUsers::default());
        (AuthService::new(repo.clone(), TokenIssuer::new(b"test-secret", 1)), repo)
    }

    fn new_user(email: &str, password: &str, is_admin: bool) -> NewUserRequest {
        NewUserRequest {
            email: email.to_string(),
            password: password.to_string(),
            is_admin,
            permission: Permission::default(),
        }
    }

    #[test]
    fn login_succeeds_with_correct_credentials() {
        let (auth, _) = service();
        auth.create_user(new_user("Alice@Example.com", "supersecret", true))
            .unwrap();

        let response = auth.login("alice@example.com", "supersecret").unwrap();
        assert_eq!(response.user.email, "alice@example.com");
        assert!(response.user.is_admin);
        assert!(response.expires_at > chrono::Utc::now());

        // The issued token resolves back to the same user.
        let user = auth.user_from_token(&response.token).unwrap();
        assert_eq!(user.email, "alice@example.com");
    }

    /// Guessing a password is not free: after a few failures the account waits - even when
    /// the password is finally right - while other addresses carry on as before.
    #[test]
    fn repeated_sign_in_failures_are_refused_for_a_while() {
        let (auth, _) = service();
        auth.create_user(new_user("a@example.com", "supersecret", false))
            .unwrap();

        // Two failures and then a success: the count starts over, so the next four are
        // ordinary rejections rather than a lock.
        assert!(auth.login("a@example.com", "wrong").is_err());
        assert!(auth.login("a@example.com", "wrong").is_err());
        assert!(auth.login("a@example.com", "supersecret").is_ok());
        for attempt in 1..=4 {
            assert_eq!(
                auth.login("a@example.com", "wrong").unwrap_err().status_code,
                401,
                "{attempt} 回目はまだ普通の拒否"
            );
        }

        // The fifth failure trips the limit, and the right password now waits too.
        assert!(auth.login("a@example.com", "wrong").is_err());
        let locked = auth.login("a@example.com", "supersecret").unwrap_err();
        assert_eq!(locked.status_code, 429);
        assert!(locked.message.contains("try again"), "{}", locked.message);

        // An address that does not exist is counted the same way, so the limiter itself
        // cannot be used to find out which accounts exist.
        for _ in 0..5 {
            assert_eq!(
                auth.login("ghost@example.com", "wrong").unwrap_err().status_code,
                401
            );
        }
        assert_eq!(
            auth.login("ghost@example.com", "wrong").unwrap_err().status_code,
            429
        );

        // And a different address is still just wrong.
        assert_eq!(
            auth.login("b@example.com", "wrong").unwrap_err().status_code,
            401
        );
    }

    /// Changing your own password ends every session, including the one that asked for the
    /// change - and hands that caller a token for the new generation so they stay signed in.
    #[test]
    fn changing_your_own_password_ends_the_sessions_that_came_before() {
        let (auth, _) = service();
        let created = auth
            .create_user(new_user("a@example.com", "old-password", false))
            .unwrap();

        let stolen = auth.login("a@example.com", "old-password").unwrap().token;
        assert!(auth.user_from_token(&stolen).is_ok());

        let changed = auth
            .change_own_password(&created.id, "old-password", "new-password")
            .unwrap();

        // The token that existed before the change is dead...
        assert_eq!(auth.user_from_token(&stolen).unwrap_err().status_code, 401);
        // ...the caller's replacement works...
        assert!(auth.user_from_token(&changed.token).is_ok());
        assert!(changed.expires_at > chrono::Utc::now());
        // ...and the credentials are the new ones.
        assert!(auth.login("a@example.com", "old-password").is_err());
        assert!(auth.login("a@example.com", "new-password").is_ok());
    }

    /// An administrator resetting someone else's password ends that account's sessions and
    /// leaves the administrator's own session alone.
    #[test]
    fn resetting_another_accounts_password_only_ends_that_accounts_sessions() {
        let (auth, _) = service();
        auth.create_user(new_user("admin@example.com", "admin-password", true))
            .unwrap();
        let target = auth
            .create_user(new_user("editor@example.com", "editor-password", false))
            .unwrap();

        let admin_token = auth.login("admin@example.com", "admin-password").unwrap().token;
        let editor_token = auth
            .login("editor@example.com", "editor-password")
            .unwrap()
            .token;

        auth.set_password(&target.id, "reset-password").unwrap();

        assert_eq!(auth.user_from_token(&editor_token).unwrap_err().status_code, 401);
        assert!(auth.user_from_token(&admin_token).is_ok(), "自分のセッションは残る");
    }

    #[test]
    fn login_failures_are_indistinguishable() {
        let (auth, _) = service();
        auth.create_user(new_user("a@example.com", "supersecret", false))
            .unwrap();

        let unknown = auth.login("nobody@example.com", "supersecret").unwrap_err();
        let wrong = auth.login("a@example.com", "wrong-password").unwrap_err();
        assert_eq!(unknown.status_code, 401);
        assert_eq!(wrong.status_code, 401);
        assert_eq!(unknown.message, wrong.message);
    }

    #[test]
    fn rejects_short_passwords_and_bad_emails() {
        let (auth, _) = service();
        assert_eq!(
            auth.create_user(new_user("a@example.com", "short", false))
                .unwrap_err()
                .status_code,
            400
        );
        assert_eq!(
            auth.create_user(new_user("not-an-email", "supersecret", false))
                .unwrap_err()
                .status_code,
            400
        );
    }

    #[test]
    fn rejects_duplicate_email_regardless_of_case() {
        let (auth, _) = service();
        auth.create_user(new_user("a@example.com", "supersecret", false))
            .unwrap();
        let err = auth.create_user(new_user("A@EXAMPLE.COM", "supersecret", false)).unwrap_err();
        assert_eq!(err.status_code, 409);
    }

    #[test]
    fn token_for_a_deleted_user_is_rejected() {
        let (auth, repo) = service();
        let created = auth.create_user(new_user("a@example.com", "supersecret", false)).unwrap();
        let token = auth.login("a@example.com", "supersecret").unwrap().token;

        repo.delete_user(&created.id).unwrap();
        assert_eq!(auth.user_from_token(&token).unwrap_err().status_code, 401);
    }

    #[test]
    fn disabled_account_cannot_log_in_and_its_token_is_rejected() {
        let (auth, repo) = service();
        let created = auth.create_user(new_user("a@example.com", "supersecret", false)).unwrap();
        let token = auth.login("a@example.com", "supersecret").unwrap().token;

        {
            let mut users = repo.users.write().unwrap();
            let user = users.get_mut(&created.id.to_string()).unwrap();
            user.is_active = false;
        }

        assert_eq!(auth.login("a@example.com", "supersecret").unwrap_err().status_code, 403);
        assert_eq!(auth.user_from_token(&token).unwrap_err().status_code, 403);
    }

    #[test]
    fn the_last_administrator_cannot_lock_the_cms_out() {
        let (auth, _) = service();
        let admin = auth
            .create_user(NewUserRequest {
                email: "admin@example.com".to_string(),
                password: "supersecret".to_string(),
                is_admin: true,
                permission: Permission::admin(),
            })
            .unwrap();
        let editor = auth.create_user(new_user("editor@example.com", "supersecret", false)).unwrap();

        // The only administrator cannot stop being one, stop being active, or be deleted.
        assert_eq!(
            auth.update_user(&admin.id, UpdateUserRequest { is_admin: Some(false), ..Default::default() })
                .unwrap_err()
                .status_code,
            409
        );
        assert_eq!(
            auth.update_user(&admin.id, UpdateUserRequest { is_active: Some(false), ..Default::default() })
                .unwrap_err()
                .status_code,
            409
        );
        assert_eq!(auth.delete_user(&admin.id).unwrap_err().status_code, 409);

        // With a second administrator the demotion is allowed...
        let second = auth
            .create_user(NewUserRequest {
                email: "second@example.com".to_string(),
                password: "supersecret".to_string(),
                is_admin: true,
                permission: Permission::admin(),
            })
            .unwrap();
        assert!(!auth
            .update_user(&admin.id, UpdateUserRequest { is_admin: Some(false), ..Default::default() })
            .unwrap()
            .is_admin);

        // ...and then the remaining administrator is the protected one.
        assert_eq!(auth.delete_user(&second.id).unwrap_err().status_code, 409);
        // The demoted account and a plain editor can be removed.
        auth.delete_user(&admin.id).unwrap();
        auth.delete_user(&editor.id).unwrap();
        assert_eq!(auth.list_users().unwrap().len(), 1);
    }

    #[test]
    fn roles_and_passwords_can_be_changed() {
        let (auth, _) = service();
        let viewer = auth.create_user(new_user("viewer@example.com", "supersecret", false)).unwrap();

        // A role change is exactly what the new permission says.
        let updated = auth
            .update_user(
                &viewer.id,
                UpdateUserRequest { permission: Some(Permission::editor()), ..Default::default() },
            )
            .unwrap();
        assert!(updated.permission.can_edit);
        assert!(!updated.permission.can_publish);
        assert!(updated.is_active);

        // Disabling keeps the account but stops it signing in.
        auth.update_user(&viewer.id, UpdateUserRequest { is_active: Some(false), ..Default::default() })
            .unwrap();
        assert_eq!(
            auth.login("viewer@example.com", "supersecret").unwrap_err().status_code,
            403
        );
        auth.update_user(&viewer.id, UpdateUserRequest { is_active: Some(true), ..Default::default() })
            .unwrap();

        // Your own password needs the current one.
        assert_eq!(
            auth.change_own_password(&viewer.id, "wrong", "newsupersecret").unwrap_err().status_code,
            403
        );
        auth.change_own_password(&viewer.id, "supersecret", "newsupersecret").unwrap();
        assert!(auth.login("viewer@example.com", "newsupersecret").is_ok());

        // An administrator reset does not, but the minimum length still applies.
        assert_eq!(auth.set_password(&viewer.id, "short").unwrap_err().status_code, 400);
        auth.set_password(&viewer.id, "resetpassword").unwrap();
        assert!(auth.login("viewer@example.com", "resetpassword").is_ok());

        // A change to an account that does not exist is a 404.
        let missing = UserId::from("00000000-0000-0000-0000-000000000000");
        assert_eq!(auth.set_password(&missing, "supersecret").unwrap_err().status_code, 404);
    }

    #[test]
    fn bootstrap_requires_credentials_only_when_the_store_is_empty() {
        let (auth, _) = service();
        // Empty store and no credentials -> refuse.
        assert_eq!(auth.bootstrap_admin(None, None).unwrap_err().status_code, 500);

        let created = auth
            .bootstrap_admin(Some("admin@example.com"), Some("supersecret"))
            .unwrap()
            .expect("admin should have been created");
        assert!(created.is_admin);

        // Once seeded, bootstrap is a no-op even without credentials.
        assert!(auth.bootstrap_admin(None, None).unwrap().is_none());
    }
}
