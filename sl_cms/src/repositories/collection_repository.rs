use std::error::Error;

use crate::models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

pub trait CollectionRepository:Send + Sync + 'static {
    fn get_collection_schema(&self, collection_name: &CollectionName) -> Result<Option<CollectionSchema>, BoxError>;
    fn list_collection_names(&self) -> Result<Vec<CollectionName>, BoxError>;
    fn add_collection_schema(&self, collection_name: &CollectionName, schema: &CollectionSchema) -> Result<(), BoxError>;
    fn delete_collection(&self, collection_name: &CollectionName) -> Result<(), BoxError>;
    fn list_collection_items(&self, collection_name: &CollectionName) -> Result<Vec<(CollectionItemId, CollectionItem)>, BoxError>;
    fn get_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<Option<CollectionItem>, BoxError>;
    fn add_collection_item(&self, collection_name: &CollectionName, item_data: &CollectionItem) -> Result<u64, BoxError>;
    fn update_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId, item_data: &CollectionItem) -> Result<(), BoxError>;
    fn delete_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<(), BoxError>;
}
