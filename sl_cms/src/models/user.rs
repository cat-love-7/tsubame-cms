use crate::models::identity::StringId;

pub type UserId = StringId<User>;

pub struct User {
    pub username: String,
    pub is_active: bool,
    pub last_login: Option<chrono::DateTime<chrono::Utc>>,
}

impl User {
    pub fn new(username: &str) -> Self {
        User {
            username: username.to_string(),
            is_active: true,
            last_login: None,
        }
    }
}
pub struct Permission {
    pub can_publish: bool,
    pub can_edit: bool,
    pub can_view: bool,
}