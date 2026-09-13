use std::error::Error;

use crate::models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema};
use crate::models::item_status::ItemMetadata;

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

    // The working copy an editor saves into. The item store is what the delivery API
    // serves, so as long as a save lands here the live site cannot change by accident.
    // Absent means "no unpublished changes".
    fn get_collection_item_draft(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<Option<CollectionItem>, BoxError>;
    fn set_collection_item_draft(&self, collection_name: &CollectionName, item_id: &CollectionItemId, item_data: &CollectionItem) -> Result<(), BoxError>;
    fn delete_collection_item_draft(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<(), BoxError>;
    /// Every working copy of a collection, so a list can show them without a read each.
    fn list_collection_item_drafts(&self, collection_name: &CollectionName) -> Result<Vec<(CollectionItemId, CollectionItem)>, BoxError>;

    // Draft/published metadata, kept out of the item's values so a schema field may be
    // named `status` without colliding. Absent metadata means "draft".
    fn get_item_metadata(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<Option<ItemMetadata>, BoxError>;
    fn set_item_metadata(&self, collection_name: &CollectionName, item_id: &CollectionItemId, metadata: &ItemMetadata) -> Result<(), BoxError>;
    fn list_item_metadata(&self, collection_name: &CollectionName) -> Result<Vec<(CollectionItemId, ItemMetadata)>, BoxError>;
}
