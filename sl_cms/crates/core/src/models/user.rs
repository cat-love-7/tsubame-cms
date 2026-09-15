use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::models::identity::StringId;

pub type UserId = StringId<User>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    /// The sign-in identifier, and the only name the CMS needs: an account can be operated
    /// without an email address.
    ///
    /// Stored in its normalized form (trimmed, lower case) and unique across accounts.
    /// `serde(default)` plus [`User::adopt_legacy_identifier`] is what lets a record written
    /// when the identifier *was* an email address still be read.
    #[serde(default)]
    pub username: String,
    /// The identifier an identity provider knows this account by (Cognito's `sub`).
    ///
    /// `None` for an account this deployment authenticates itself, or for a record written
    /// before the deployment moved to a provider. It is what a provider's token is resolved
    /// through, and it is set the first time someone signs in (see
    /// `AuthService::user_from_token`).
    #[serde(default)]
    pub external_id: Option<String>,
    /// Contact address, if the operator recorded one. Never used to sign in, so it may be
    /// absent; a future Cognito deployment keeps it as an attribute, not an identity.
    #[serde(default)]
    pub email: Option<String>,
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
    /// A new account record.
    ///
    /// No password: where one is stored, it is stored by the deployment's adapter
    /// ([`LocalCredentials`](crate::repositories::local_credentials::LocalCredentials)), never
    /// in the record the CMS reasons about.
    pub fn new(username: &str, is_admin: bool, permission: Permission) -> Self {
        User {
            id: UserId::from(uuid::Uuid::new_v4().to_string().as_str()),
            username: normalize_username(username),
            external_id: None,
            email: None,
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

    /// Fill in `username` for a record written before the field existed, where the sign-in
    /// identifier was the email address.
    ///
    /// The storage adapters call this on the way out, so an existing installation can be
    /// upgraded without anyone being locked out.
    pub fn adopt_legacy_identifier(&mut self) {
        if self.username.trim().is_empty() {
            if let Some(email) = self.email.as_deref() {
                self.username = normalize_username(email);
            }
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

    /// Whether this user may edit content anywhere.
    ///
    /// The image library is shared: an image is not owned by a collection or a page, so there is
    /// no resource to judge against. Uploading one is therefore allowed to anyone who may edit
    /// *something* - an editor with a grant for one collection needs the images that collection
    /// uses - while changing or deleting an image that others may be using stays with the
    /// account-wide permission (see `require_auth` in `http`).
    pub fn can_edit_somewhere(&self) -> bool {
        self.is_admin
            || self.permission.can_edit
            || self.collection_permissions.values().any(|p| p.can_edit)
            || self.single_page_permissions.values().any(|p| p.can_edit)
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
            username: self.username.clone(),
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

/// Longest identifier accepted. Cognito allows 128, and staying inside its limit keeps a
/// later import from having to rename anybody.
pub const MAX_USERNAME_LEN: usize = 128;

/// Case- and whitespace-insensitive form used for storage and lookup, so `Alice` and `alice`
/// are the same account.
pub fn normalize_username(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

/// Whether `username` can be a sign-in identifier.
///
/// An address is *allowed* (`ops@example.com`), it is simply not required: the identifier is
/// whatever the operator calls the account. The character set is deliberately the one Cognito
/// accepts (`A-Z a-z 0-9 + = , . @ _ -`), so an account created here can be carried over to a
/// Cognito pool without being renamed.
pub fn is_plausible_username(username: &str) -> bool {
    let username = username.trim();
    !username.is_empty()
        && username.len() <= MAX_USERNAME_LEN
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+=,.@_-".contains(c))
}

/// Very small sanity check for the *optional* contact address — deliberately not a full
/// RFC 5322 validator.
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
    /// The sign-in identifier; this is what an account is called everywhere in the UI.
    pub username: String,
    /// Contact address, when one was recorded.
    pub email: Option<String>,
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
    pub username: String,
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
    pub username: String,
    /// An initial password, when the caller has one to choose.
    ///
    /// Left out, the account exists with no credential: it cannot sign in until an administrator
    /// issues a reset link and the person sets their own password. That is what the account screen
    /// does - an administrator choosing someone else's password is a habit worth not having - and
    /// the field stays for deployments that provision accounts from a script.
    #[serde(default)]
    pub password: Option<String>,
    /// Contact address, if the operator has one to record.
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub permission: Permission,
}

/// A new account where an identity provider owns the credential: the same fields as
/// [`NewUserRequest`] without a password, because there is none for the CMS to choose.
#[derive(serde::Deserialize, Debug)]
pub struct NewAccountRequest {
    pub username: String,
    /// Contact address, if the operator has one to record.
    #[serde(default)]
    pub email: Option<String>,
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

/// Completing an administrator-issued reset: the token from the link, and the password the
/// account's owner chose.
#[derive(serde::Deserialize, Debug)]
pub struct CompletePasswordResetRequest {
    pub token: String,
    pub new_password: String,
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
    fn normalizes_and_validates_a_username() {
        assert_eq!(normalize_username("  Ops.User "), "ops.user");
        // A name is enough; an address is allowed but not required.
        for username in ["ops", "ops.user", "ops@example.com", "team+editor", "a-b_c"] {
            assert!(is_plausible_username(username), "{username}");
        }
        for username in [
            "",
            "   ",
            "with space",
            "with:colon",
            "with/slash",
            "quote\"",
        ] {
            assert!(!is_plausible_username(username), "{username:?}");
        }
        assert!(!is_plausible_username(&"x".repeat(MAX_USERNAME_LEN + 1)));
        assert!(is_plausible_username(&"x".repeat(MAX_USERNAME_LEN)));

        // The contact address keeps its own, looser check.
        assert!(is_plausible_email("a@b.co"));
        assert!(!is_plausible_email(""));
        assert!(!is_plausible_email("no-at-sign"));
        assert!(!is_plausible_email("@example.com"));
        assert!(!is_plausible_email("a@b@c"));
        assert!(!is_plausible_email("with space@example.com"));
    }

    #[test]
    fn new_user_is_a_viewer_with_no_write_access() {
        let user = User::new("Ops.User", false, Permission::default());
        assert_eq!(user.username, "ops.user");
        assert!(user.email.is_none(), "メールアドレスは任意");
        assert!(!user.can_write(user.permission));
        assert!(user.is_active);
        // The response projection must not leak the hash.
        let json = serde_json::to_string(&user.to_response()).unwrap();
        assert!(!json.contains("hash"), "response leaked the password hash: {json}");
    }

    /// A record written when the identifier *was* the email address still signs in: the
    /// storage adapters fill `username` in from `email` on the way out.
    #[test]
    fn an_account_from_before_usernames_existed_falls_back_to_its_email() {
        let mut legacy: User = serde_json::from_str(
            r#"{"id":"u-1","email":"ops@example.com","is_active":true,
                "is_admin":false,"permission":{"can_publish":false,"can_edit":false,"can_view":true},
                "created_at":"2024-01-01T00:00:00Z","last_login":null}"#,
        )
        .unwrap();
        assert_eq!(legacy.username, "", "古いレコードには username が無い");
        assert_eq!(legacy.email.as_deref(), Some("ops@example.com"));

        legacy.adopt_legacy_identifier();
        assert_eq!(legacy.username, "ops@example.com", "メールを識別子として引き継ぐ");
        // The address stays as the contact address.
        assert_eq!(legacy.email.as_deref(), Some("ops@example.com"));
    }

    /// A record that already has a username keeps it, even if an address is recorded too.
    #[test]
    fn adopting_a_legacy_identifier_does_not_overwrite_a_username() {
        let mut user = User::new("ops", false, Permission::default());
        user.email = Some("ops@example.com".to_string());
        user.adopt_legacy_identifier();
        assert_eq!(user.username, "ops");
    }

    /// An account written before `token_version` existed still reads, at generation 0 -
    /// which is what the tokens outstanding for it carry, so nobody is signed out by the
    /// upgrade itself.
    #[test]
    fn an_account_from_before_token_versions_existed_is_still_readable() {
        let legacy: User = serde_json::from_str(
            r#"{"id":"u-1","email":"a@example.com","is_active":true,
                "is_admin":false,"permission":{"can_publish":false,"can_edit":false,"can_view":true},
                "created_at":"2024-01-01T00:00:00Z","last_login":null}"#,
        )
        .unwrap();
        assert_eq!(legacy.token_version, 0);
        // The identifier is empty here; the adapters fill it in from the address.
        assert_eq!(legacy.email.as_deref(), Some("a@example.com"));
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
        let mut user = User::new("a@b.co", false, Permission::viewer());
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
        let mut admin = User::new("a@b.co", true, Permission::admin());
        admin.single_page_permissions.insert(
            "home".to_string(),
            Permission { can_view: false, can_edit: false, can_publish: false },
        );
        let denied = admin.permission_for_single_page("home");
        assert!(admin.can_read(denied) && admin.can_write(denied) && admin.can_publish(denied));
    }

    #[test]
    fn admin_and_editor_can_write() {
        let admin = User::new("a@b.co", true, Permission::admin());
        let editor = User::new("a@b.co", false, Permission::editor());
        assert!(admin.can_write(admin.permission));
        assert!(editor.can_write(editor.permission));
    }

    /// The three capabilities are independent: an editor writes drafts but cannot release
    /// them, and only a publisher (or an administrator) can.
    #[test]
    fn publishing_is_a_separate_capability_from_editing() {
        let editor = User::new("a@b.co", false, Permission::editor());
        assert!(editor.can_write(editor.permission));
        assert!(editor.can_read(editor.permission));
        assert!(!editor.can_publish(editor.permission));

        let publisher = User::new(
            "a@b.co",
            false,
            Permission { can_view: true, can_edit: true, can_publish: true },
        );
        assert!(publisher.can_publish(publisher.permission));

        // An administrator can do all three whatever the permission record says.
        let admin = User::new("a@b.co", true, Permission::viewer());
        assert!(admin.can_read(admin.permission) && admin.can_write(admin.permission) && admin.can_publish(admin.permission));
    }

    #[test]
    fn an_account_without_can_view_cannot_read() {
        let none = User::new(
            "a@b.co",
            false,
            Permission { can_view: false, can_edit: false, can_publish: false },
        );
        assert!(!none.can_read(none.permission));
        assert!(!none.can_write(none.permission));
        assert!(!none.can_publish(none.permission));
    }
}
