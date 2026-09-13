use std::collections::HashMap;

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
    /// Bumped whenever the credentials change, so every token issued before that stops
    /// being accepted.
    ///
    /// A "reject tokens issued before X" timestamp would need the issuer and the verifier
    /// to agree on the clock; a counter only has to be read from the stored account.
    /// `serde(default)` keeps accounts written before this field readable: they start at
    /// version 0, which is what their outstanding tokens carry.
    #[serde(default)]
    pub token_version: u64,
    /// Per-collection overrides, keyed by collection name.
    ///
    /// The account-wide [`User::permission`] is what an account has everywhere; an entry here
    /// **replaces** it for that one collection, so a grant can both widen access (an editor
    /// for one collection) and narrow it (no access to a collection the account could
    /// otherwise edit). Absent means "the account-wide permission applies".
    #[serde(default)]
    pub collection_permissions: HashMap<String, Permission>,
    /// The same for single pages, keyed by page name.
    #[serde(default)]
    pub single_page_permissions: HashMap<String, Permission>,
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
            token_version: 0,
            collection_permissions: HashMap::new(),
            single_page_permissions: HashMap::new(),
        }
    }

    /// What this account may do with one collection.
    pub fn permission_for_collection(&self, name: &str) -> Permission {
        self.collection_permissions
            .get(name)
            .copied()
            .unwrap_or(self.permission)
    }

    /// What this account may do with one single page.
    pub fn permission_for_single_page(&self, name: &str) -> Permission {
        self.single_page_permissions
            .get(name)
            .copied()
            .unwrap_or(self.permission)
    }

    /// End every session that exists now, and start a new generation of tokens.
    pub fn end_existing_sessions(&mut self) {
        self.token_version = self.token_version.saturating_add(1);
    }

    /// Whether this user may perform write operations, judged against `permission`.
    ///
    /// Callers pass the permission that applies to what the request is about: the
    /// account-wide one, or a collection's own (see
    /// [`User::permission_for_collection`]).
    pub fn can_write(&self, permission: Permission) -> bool {
        self.is_admin || permission.can_edit
    }

    /// Whether this user may read content, drafts included, judged against `permission`.
    pub fn can_read(&self, permission: Permission) -> bool {
        self.is_admin || permission.can_view
    }

    /// Whether this user may change whether content is published.
    ///
    /// Deliberately separate from [`User::can_write`]: drafts are kept apart from the
    /// published copy, so editing an item does not touch the live site. That is what lets
    /// a CMS have editors who prepare content and publishers who release it.
    /// Whether this user may change what is published, judged against `permission`.
    pub fn can_publish(&self, permission: Permission) -> bool {
        self.is_admin || permission.can_publish
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
            collection_permissions: self.collection_permissions.clone(),
            single_page_permissions: self.single_page_permissions.clone(),
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
    /// Per-resource overrides, so the account screen can show them.
    pub collection_permissions: HashMap<String, Permission>,
    pub single_page_permissions: HashMap<String, Permission>,
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

/// What changing your *own* password answers with: a token for the new generation.
///
/// The change ends every session, including the caller's, so without this the person who
/// just changed their password would be signed out of the screen they did it from.
#[derive(serde::Serialize, Debug)]
pub struct PasswordChangedResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
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
    /// Replaces the collection overrides when present. The map is sent whole rather than
    /// merged: the account screen knows the full picture, and "remove this override" has to
    /// be expressible somehow.
    #[serde(default)]
    pub collection_permissions: Option<HashMap<String, Permission>>,
    #[serde(default)]
    pub single_page_permissions: Option<HashMap<String, Permission>>,
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
        assert!(!user.can_write(user.permission));
        assert!(user.is_active);
        // The response projection must not leak the hash.
        let json = serde_json::to_string(&user.to_response()).unwrap();
        assert!(!json.contains("hash"), "response leaked the password hash: {json}");
    }

    /// An account written before `token_version` existed still reads, at generation 0 -
    /// which is what the tokens outstanding for it carry, so nobody is signed out by the
    /// upgrade itself.
    #[test]
    fn an_account_from_before_token_versions_existed_is_still_readable() {
        let legacy: User = serde_json::from_str(
            r#"{"id":"u-1","email":"a@example.com","password_hash":"h","is_active":true,
                "is_admin":false,"permission":{"can_publish":false,"can_edit":false,"can_view":true},
                "created_at":"2024-01-01T00:00:00Z","last_login":null}"#,
        )
        .unwrap();
        assert_eq!(legacy.token_version, 0);
        // No resource overrides either: the account-wide permission applies everywhere.
        assert!(legacy.collection_permissions.is_empty());
        assert!(legacy.single_page_permissions.is_empty());

        let mut changed = legacy.clone();
        changed.end_existing_sessions();
        assert_eq!(changed.token_version, 1);
    }

    /// A per-resource entry replaces the account-wide permission for that one resource, so a
    /// grant can widen access and take it away.
    #[test]
    fn a_resource_override_replaces_the_account_wide_permission() {
        let mut user = User::new("a@b.co", "h".into(), false, Permission::viewer());
        // Everywhere: read only.
        assert!(user.can_read(user.permission));
        assert!(!user.can_write(user.permission));

        // In `blog`: editing, but still no publishing.
        user.collection_permissions
            .insert("blog".to_string(), Permission::editor());
        let blog = user.permission_for_collection("blog");
        assert!(user.can_write(blog));
        assert!(!user.can_publish(blog));

        // Another collection, and every single page, keep the account-wide permission.
        assert!(!user.can_write(user.permission_for_collection("news")));
        assert!(!user.can_write(user.permission_for_single_page("home")));

        // An override can take access away too.
        user.single_page_permissions.insert(
            "home".to_string(),
            Permission { can_view: false, can_edit: false, can_publish: false },
        );
        assert!(!user.can_read(user.permission_for_single_page("home")));
        assert!(user.can_read(user.permission_for_single_page("about")));
    }

    /// An administrator is an administrator everywhere: an override cannot lock one out of
    /// the resource they are supposed to be able to fix.
    #[test]
    fn an_administrator_is_not_bound_by_resource_overrides() {
        let mut admin = User::new("a@b.co", "h".into(), true, Permission::admin());
        admin.single_page_permissions.insert(
            "home".to_string(),
            Permission { can_view: false, can_edit: false, can_publish: false },
        );
        let denied = admin.permission_for_single_page("home");
        assert!(admin.can_read(denied) && admin.can_write(denied) && admin.can_publish(denied));
    }

    #[test]
    fn admin_and_editor_can_write() {
        let admin = User::new("a@b.co", "h".into(), true, Permission::admin());
        let editor = User::new("a@b.co", "h".into(), false, Permission::editor());
        assert!(admin.can_write(admin.permission));
        assert!(editor.can_write(editor.permission));
    }

    /// The three capabilities are independent: an editor writes drafts but cannot release
    /// them, and only a publisher (or an administrator) can.
    #[test]
    fn publishing_is_a_separate_capability_from_editing() {
        let editor = User::new("a@b.co", "h".into(), false, Permission::editor());
        assert!(editor.can_write(editor.permission));
        assert!(editor.can_read(editor.permission));
        assert!(!editor.can_publish(editor.permission));

        let publisher = User::new(
            "a@b.co",
            "h".into(),
            false,
            Permission { can_view: true, can_edit: true, can_publish: true },
        );
        assert!(publisher.can_publish(publisher.permission));

        // An administrator can do all three whatever the permission record says.
        let admin = User::new("a@b.co", "h".into(), true, Permission::viewer());
        assert!(admin.can_read(admin.permission) && admin.can_write(admin.permission) && admin.can_publish(admin.permission));
    }

    #[test]
    fn an_account_without_can_view_cannot_read() {
        let none = User::new(
            "a@b.co",
            "h".into(),
            false,
            Permission { can_view: false, can_edit: false, can_publish: false },
        );
        assert!(!none.can_read(none.permission));
        assert!(!none.can_write(none.permission));
        assert!(!none.can_publish(none.permission));
    }
}
