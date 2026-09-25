use std::error::Error;

use std::future::Future;

use crate::models::item_status::{ItemDates, ItemMetadata};
use crate::models::schema::SchemaSettings;
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

    /// What a page is told about itself; [`SchemaSettings::default`] for a page that has never been
    /// given any, as for a collection.
    fn get_single_page_settings(
        &self,
        page_name: &SinglePageName,
    ) -> impl Future<Output = Result<SchemaSettings, BoxError>> + Send;

    /// Replace a page's settings. The caller has already checked the page exists.
    fn set_single_page_settings(
        &self,
        page_name: &SinglePageName,
        settings: &SchemaSettings,
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
    /// Write the dates a caller stated, and nothing else about the record (see
    /// [`CollectionRepository::set_item_dates`](crate::repositories::collection_repository::CollectionRepository::set_item_dates)).
    fn set_page_dates(
        &self,
        page_name: &SinglePageName,
        dates: &ItemDates,
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
