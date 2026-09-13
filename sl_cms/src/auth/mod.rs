//! Authentication: credential verification, token issuing, and the user lifecycle.

pub mod password;
pub mod token;

use std::sync::Arc;

use crate::models::error::HttpError;
use crate::models::user::{
    is_plausible_email, normalize_email, LoginResponse, NewUserRequest, Permission, User, UserId,
    UserResponse,
};
use crate::repositories::user_repository::UserRepository;
use token::TokenIssuer;

/// Shortest password accepted when creating a user.
pub const MIN_PASSWORD_LENGTH: usize = 8;

/// Message returned for both "unknown email" and "wrong password" so that the endpoint
/// cannot be used to enumerate accounts.
const BAD_CREDENTIALS: &str = "invalid email or password";

pub struct AuthService<R: UserRepository> {
    repository: Arc<R>,
    issuer: TokenIssuer,
}

impl<R: UserRepository> AuthService<R> {
    pub fn new(repository: Arc<R>, issuer: TokenIssuer) -> Self {
        AuthService { repository, issuer }
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
        let user = self
            .repository
            .get_user_from_email(&email)
            .map_err(internal)?;

        let Some(mut user) = user else {
            return Err(HttpError::Unauthorized(BAD_CREDENTIALS));
        };

        if !password::verify_password(password, &user.password_hash) {
            return Err(HttpError::Unauthorized(BAD_CREDENTIALS));
        }
        if !user.is_active {
            return Err(HttpError::Forbidden("account is disabled"));
        }

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
        let user_id = self.issuer.verify_subject(token).map_err(|e| {
            // Do not echo the verifier's reason to the client.
            tracing::debug!("rejected token: {e}");
            HttpError::Unauthorized("invalid or expired token")
        })?;

        let user = self
            .repository
            .get_user_from_id(&user_id)
            .map_err(internal)?
            .ok_or_else(|| HttpError::Unauthorized("invalid or expired token"))?;

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
        if request.password.len() < MIN_PASSWORD_LENGTH {
            return Err(HttpError::BadRequest(&format!(
                "password must be at least {MIN_PASSWORD_LENGTH} characters"
            )));
        }
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

    /// Look up a user by id (used by the `/auth/me` handler).
    pub fn find_user(&self, id: &UserId) -> Result<Option<User>, HttpError> {
        self.repository.get_user_from_id(id).map_err(internal)
    }
}

fn internal<E: std::fmt::Display>(e: E) -> HttpError {
    HttpError::InternalServerError(&e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use super::*;
    use crate::models::collection::CollectionName;
    use crate::models::single_page::SinglePageName;
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
        fn get_user_permissions(&self, user_id: &UserId) -> Result<Option<Permission>, BoxError> {
            Ok(self.get_user_from_id(user_id)?.map(|u| u.permission))
        }
        fn get_collection_permissions(
            &self,
            _user_id: &UserId,
            _collection_name: &CollectionName,
        ) -> Result<Option<Permission>, BoxError> {
            Ok(None)
        }
        fn get_single_page_permissions(
            &self,
            _user_id: &UserId,
            _page_name: &SinglePageName,
        ) -> Result<Option<Permission>, BoxError> {
            Ok(None)
        }
        fn get_image_permissions(&self, _user_id: &UserId) -> Result<Option<Permission>, BoxError> {
            Ok(None)
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
