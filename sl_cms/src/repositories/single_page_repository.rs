use std::error::Error;

use crate::models::item_status::ItemMetadata;
use crate::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

pub trait SinglePageRepository:Send + Sync + 'static {
    fn get_single_page_schema(&self, page_name: &SinglePageName) -> Result<Option<SinglePageSchema>, BoxError>;
    fn list_all_page_names(&self) -> Result<Vec<SinglePageName>, BoxError>;
    fn add_single_page_schema(&self, page_name: &SinglePageName, schema: &SinglePageSchema) -> Result<(), BoxError>;
    fn delete_single_page(&self, page_name: &SinglePageName) -> Result<(), BoxError>;
    fn get_single_page_item(&self, page_name: &SinglePageName) -> Result<Option<SinglePageItem>, BoxError>;
    fn update_single_page_item(&self, page_name: &SinglePageName, item_data: &SinglePageItem) -> Result<(), BoxError>;

    // The working copy, as for collection items. Absent means "no unpublished changes".
    fn get_single_page_item_draft(&self, page_name: &SinglePageName) -> Result<Option<SinglePageItem>, BoxError>;
    fn set_single_page_item_draft(&self, page_name: &SinglePageName, item_data: &SinglePageItem) -> Result<(), BoxError>;
    fn delete_single_page_item_draft(&self, page_name: &SinglePageName) -> Result<(), BoxError>;

    // Draft/published metadata. Absent metadata means "draft".
    fn get_page_metadata(&self, page_name: &SinglePageName) -> Result<Option<ItemMetadata>, BoxError>;
    fn set_page_metadata(&self, page_name: &SinglePageName, metadata: &ItemMetadata) -> Result<(), BoxError>;
    /// Publish or unpublish a page as a single step; see
    /// [`CollectionRepository::apply_item_status`](crate::repositories::collection_repository::CollectionRepository::apply_item_status)
    /// for why this is one call and not three.
    fn apply_page_status(&self, page_name: &SinglePageName, draft: Option<&SinglePageItem>, metadata: &ItemMetadata) -> Result<(), BoxError>;
}
