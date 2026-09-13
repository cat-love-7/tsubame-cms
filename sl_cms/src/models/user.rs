use chrono::{DateTime, Utc};

use crate::models::identity::StringId;

pub type UserId = StringId<User>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub email: String,
    /// Argon2 PHC string. It has to be serialised (the record is stored as JSON), so
    /// never hand a `User` to a client — use [`User::to_response`].
    pub password_hash: String,
    pub is_active: bool,
    pub is_admin: bool,
    pub permission: Permission,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
}

impl User {
    pub fn new(email: &str, password_hash: String, is_admin: bool, permission: Permission) -> Self {
        User {
            id: UserId::from(uuid::Uuid::new_v4().to_string().as_str()),
            email: normalize_email(email),
            password_hash,
            is_active: true,
            is_admin,
            permission,
            created_at: Utc::now(),
            last_login: None,
        }
    }

    /// Whether this user may perform write operations.
    pub fn can_write(&self) -> bool {
        self.is_admin || self.permission.can_edit
    }

    /// Whether this user may read content, drafts included.
    pub fn can_read(&self) -> bool {
        self.is_admin || self.permission.can_view
    }

    /// Whether this user may change whether content is published.
    ///
    /// Deliberately separate from [`User::can_write`]: drafts are kept apart from the
    /// published copy, so editing an item does not touch the live site. That is what lets
    /// a CMS have editors who prepare content and publishers who release it.
    pub fn can_publish(&self) -> bool {
        self.is_admin || self.permission.can_publish
    }

    pub fn to_response(&self) -> UserResponse {
        UserResponse {
            id: self.id.clone(),
            email: self.email.clone(),
            is_admin: self.is_admin,
            is_active: self.is_active,
            permission: self.permission,
            created_at: self.created_at,
            last_login: self.last_login,
        }
    }
}

/// Case- and whitespace-insensitive form used for storage and lookup, so
/// `Alice@Example.com` and `alice@example.com` are the same account.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Very small sanity check — deliberately not a full RFC 5322 validator.
pub fn is_plausible_email(email: &str) -> bool {
    let email = email.trim();
    !email.is_empty()
        && !email.contains(char::is_whitespace)
        && email.matches('@').count() == 1
        && !email.starts_with('@')
        && !email.ends_with('@')
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    pub can_publish: bool,
    pub can_edit: bool,
    pub can_view: bool,
}

impl Permission {
    pub fn admin() -> Self {
        Permission { can_publish: true, can_edit: true, can_view: true }
    }
    pub fn editor() -> Self {
        Permission { can_publish: false, can_edit: true, can_view: true }
    }
    pub fn viewer() -> Self {
        Permission { can_publish: false, can_edit: false, can_view: true }
    }
}

impl Default for Permission {
    /// New accounts start as viewers rather than with no access at all.
    fn default() -> Self {
        Permission::viewer()
    }
}

/// Client-facing projection that never contains the password hash.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct UserResponse {
    pub id: UserId,
    pub email: String,
    pub is_admin: bool,
    /// Whether the account may sign in at all; a disabled account keeps its history.
    pub is_active: bool,
    pub permission: Permission,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
}

#[derive(serde::Deserialize, Debug)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(serde::Serialize, Debug)]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserResponse,
}

#[derive(serde::Deserialize, Debug)]
pub struct NewUserRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub permission: Permission,
}

/// A partial change to one account. Absent fields are left alone, so a client that only
/// knows about roles cannot accidentally reset the others.
#[derive(serde::Deserialize, Debug, Default)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub is_admin: Option<bool>,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub permission: Option<Permission>,
}

/// An administrator setting someone else's password.
#[derive(serde::Deserialize, Debug)]
pub struct ResetPasswordRequest {
    pub password: String,
}

/// Changing your own password: the current one is required, because a stolen session
/// should not be enough to lock the owner out.
#[derive(serde::Deserialize, Debug)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_validates_email() {
        assert_eq!(normalize_email("  Alice@Example.COM "), "alice@example.com");
        assert!(is_plausible_email("a@b.co"));
        assert!(!is_plausible_email(""));
        assert!(!is_plausible_email("no-at-sign"));
        assert!(!is_plausible_email("@example.com"));
        assert!(!is_plausible_email("a@b@c"));
        assert!(!is_plausible_email("with space@example.com"));
    }

    #[test]
    fn new_user_is_a_viewer_with_no_write_access() {
        let user = User::new("A@Example.com", "hash".to_string(), false, Permission::default());
        assert_eq!(user.email, "a@example.com");
        assert!(!user.can_write());
        assert!(user.is_active);
        // The response projection must not leak the hash.
        let json = serde_json::to_string(&user.to_response()).unwrap();
        assert!(!json.contains("hash"), "response leaked the password hash: {json}");
    }

    #[test]
    fn admin_and_editor_can_write() {
        let admin = User::new("a@b.co", "h".into(), true, Permission::admin());
        let editor = User::new("a@b.co", "h".into(), false, Permission::editor());
        assert!(admin.can_write());
        assert!(editor.can_write());
    }

    /// The three capabilities are independent: an editor writes drafts but cannot release
    /// them, and only a publisher (or an administrator) can.
    #[test]
    fn publishing_is_a_separate_capability_from_editing() {
        let editor = User::new("a@b.co", "h".into(), false, Permission::editor());
        assert!(editor.can_write());
        assert!(editor.can_read());
        assert!(!editor.can_publish());

        let publisher = User::new(
            "a@b.co",
            "h".into(),
            false,
            Permission { can_view: true, can_edit: true, can_publish: true },
        );
        assert!(publisher.can_publish());

        // An administrator can do all three whatever the permission record says.
        let admin = User::new("a@b.co", "h".into(), true, Permission::viewer());
        assert!(admin.can_read() && admin.can_write() && admin.can_publish());
    }

    #[test]
    fn an_account_without_can_view_cannot_read() {
        let none = User::new(
            "a@b.co",
            "h".into(),
            false,
            Permission { can_view: false, can_edit: false, can_publish: false },
        );
        assert!(!none.can_read());
        assert!(!none.can_write());
        assert!(!none.can_publish());
    }
}
