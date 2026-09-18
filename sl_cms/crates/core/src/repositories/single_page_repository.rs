use std::error::Error;

use std::future::Future;

use crate::models::item_status::ItemMetadata;
use crate::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

pub trait SinglePageRepository: Send + Sync + 'static {
    fn get_single_page_schema(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<Option<SinglePageSchema>, BoxError>> + Send;
    fn list_all_page_names(
        &self,
    ) -> impl Future<Output = Result<Vec<SinglePageName>, BoxError>> + Send;
    fn add_single_page_schema(
        &self,
        page_name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn delete_single_page(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn get_single_page_item(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<Option<SinglePageItem>, BoxError>> + Send;
    fn update_single_page_item(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    // The working copy, as for collection items. Absent means "no unpublished changes".
    fn get_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<Option<SinglePageItem>, BoxError>> + Send;
    fn set_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn delete_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    // Draft/published metadata. Absent metadata means "draft".
    fn get_page_metadata(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<Option<ItemMetadata>, BoxError>> + Send;
    fn set_page_metadata(
        &self,
        page_name: &SinglePageName,
        metadata: &ItemMetadata,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Record that the page changed, without touching anything else on its record (see
    /// [`CollectionRepository::touch_item_metadata`](crate::repositories::collection_repository::CollectionRepository::touch_item_metadata)).
    fn touch_page_metadata(
        &self,
        page_name: &SinglePageName,
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Publish or unpublish a page as a single step; see
    /// [`CollectionRepository::apply_item_status`](crate::repositories::collection_repository::CollectionRepository::apply_item_status)
    /// for why this is one call and not three.
    fn apply_page_status(
        &self,
        page_name: &SinglePageName,
        draft: Option<&SinglePageItem>,
        metadata: &ItemMetadata,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}
