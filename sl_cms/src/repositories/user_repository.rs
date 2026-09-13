use std::error::Error;

use crate::models::{collection::CollectionName, single_page::SinglePageName, user::{Permission, User, UserId}};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

pub trait UserRepository:Send + Sync + 'static {
    fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError>;
    fn get_user_from_email(&self, email: &str) -> Result<Option<User>, BoxError>;
    fn add_user(&self, user: &User) -> Result<UserId, BoxError>;
    fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError>;
    fn get_all_users(&self) -> Result<Vec<(UserId, User)>, BoxError>;
    fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError>;
    fn get_user_permissions(&self, user_id: &UserId) -> Result<Option<Permission>, BoxError>;
    fn get_collection_permissions(&self, user_id: &UserId, collection_name: &CollectionName) -> Result<Option<Permission>, BoxError>;
    fn get_single_page_permissions(&self, user_id: &UserId, page_name: &SinglePageName) -> Result<Option<Permission>, BoxError>;
    fn get_image_permissions(&self, user_id: &UserId) -> Result<Option<Permission>, BoxError>;
}
