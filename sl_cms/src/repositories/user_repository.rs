use std::error::Error;

use crate::models::user::{User, UserId};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

pub trait UserRepository:Send + Sync + 'static {
    fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError>;
    fn get_user_from_email(&self, email: &str) -> Result<Option<User>, BoxError>;
    fn add_user(&self, user: &User) -> Result<UserId, BoxError>;
    fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError>;
    fn get_all_users(&self) -> Result<Vec<(UserId, User)>, BoxError>;
    fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError>;
}
