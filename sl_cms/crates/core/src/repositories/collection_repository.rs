use std::error::Error;

use std::future::Future;

use crate::models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema};
use crate::models::item_status::ItemMetadata;

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// One value a schema declares unique, as the index compares it.
///
/// `value` is whatever the collection compares, so the caller trims it and drops the empty
/// ones: an optional unique field left blank must not collide with another blank one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueValue {
    pub field: String,
    pub value: String,
}

/// What claiming a unique value did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation {
    /// The item holds it now: nobody did, or it already did.
    Held,
    /// Another item holds it, so the write that wanted it cannot go ahead.
    Taken { owner: CollectionItemId },
}

/// The `offset`/`limit` half of the windowed list methods, shared by the default
/// implementations so every backend cuts a list the same way.
pub(crate) fn cut<T>(items: Vec<T>, offset: usize, limit: Option<usize>) -> Vec<T> {
    match limit {
        Some(limit) => items.into_iter().skip(offset).take(limit).collect(),
        None => items.into_iter().skip(offset).collect(),
    }
}

pub trait CollectionRepository:Send + Sync + 'static {
    fn get_collection_schema(&self, collection_name: &CollectionName) -> impl Future<Output = Result<Option<CollectionSchema>, BoxError>> + Send;
    fn list_collection_names(&self) -> impl Future<Output = Result<Vec<CollectionName>, BoxError>> + Send;
    fn add_collection_schema(&self, collection_name: &CollectionName, schema: &CollectionSchema) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn delete_collection(&self, collection_name: &CollectionName) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn list_collection_items(&self, collection_name: &CollectionName) -> impl Future<Output = Result<Vec<(CollectionItemId, CollectionItem)>, BoxError>> + Send;
    fn get_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> impl Future<Output = Result<Option<CollectionItem>, BoxError>> + Send;
    fn add_collection_item(&self, collection_name: &CollectionName, item_data: &CollectionItem) -> impl Future<Output = Result<u64, BoxError>> + Send;
    fn update_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId, item_data: &CollectionItem) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn delete_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> impl Future<Output = Result<(), BoxError>> + Send;

    // The working copy an editor saves into. The item store is what the delivery API
    // serves, so as long as a save lands here the live site cannot change by accident.
    // Absent means "no unpublished changes".
    fn get_collection_item_draft(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> impl Future<Output = Result<Option<CollectionItem>, BoxError>> + Send;
    fn set_collection_item_draft(&self, collection_name: &CollectionName, item_id: &CollectionItemId, item_data: &CollectionItem) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn delete_collection_item_draft(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Every working copy of a collection, so a list can show them without a read each.
    fn list_collection_item_drafts(&self, collection_name: &CollectionName) -> impl Future<Output = Result<Vec<(CollectionItemId, CollectionItem)>, BoxError>> + Send;

    // Draft/published metadata, kept out of the item's values so a schema field may be
    // named `status` without colliding. Absent metadata means "draft".
    fn get_item_metadata(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> impl Future<Output = Result<Option<ItemMetadata>, BoxError>> + Send;
    fn set_item_metadata(&self, collection_name: &CollectionName, item_id: &CollectionItemId, metadata: &ItemMetadata) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn list_item_metadata(&self, collection_name: &CollectionName) -> impl Future<Output = Result<Vec<(CollectionItemId, ItemMetadata)>, BoxError>> + Send;

    /// One page of a collection's items, in id order, plus how many items there are.
    ///
    /// The default reads the whole list and cuts it, which is correct anywhere. A backend that
    /// can read a window — DynamoDB's `Limit`/`ExclusiveStartKey`, an LMDB iterator that skips
    /// — overrides it, so serving one page of a large collection stops costing the whole
    /// collection. `limit` of `None` means "to the end", which is what an unpaginated caller
    /// asks for.
    fn list_collection_items_page(
        &self,
        collection_name: &CollectionName,
        offset: usize,
        limit: Option<usize>,
    ) -> impl Future<Output = Result<(Vec<(CollectionItemId, CollectionItem)>, usize), BoxError>> + Send
    {
        // The default is to read the list and cut it, which is correct anywhere; an adapter
        // that can read a window overrides this. `async move` because the body borrows `self`.
        async move {
            let all = self.list_collection_items(collection_name).await?;
            let total = all.len();
            Ok((cut(all, offset, limit), total))
        }
    }

    /// One page of the *published* items, in id order, each with its metadata, plus how many
    /// published items there are.
    ///
    /// This is the delivery API's list. It is separate from
    /// [`Self::list_collection_items_page`] because the set being paged is the published one:
    /// the delivery API may only ever see published content, and its pages have to be pages
    /// *of that set*, not of the items with some of them filtered out afterwards.
    fn list_published_items_page(
        &self,
        collection_name: &CollectionName,
        offset: usize,
        limit: Option<usize>,
    ) -> impl Future<
        Output = Result<(Vec<(CollectionItemId, CollectionItem, ItemMetadata)>, usize), BoxError>,
    > + Send {
        async move {
            let metadata: std::collections::HashMap<CollectionItemId, ItemMetadata> = self
                .list_item_metadata(collection_name)
                .await?
                .into_iter()
                .collect();
            let mut published: Vec<(CollectionItemId, CollectionItem, ItemMetadata)> = Vec::new();
            for (id, item) in self.list_collection_items(collection_name).await? {
                if let Some(metadata) = metadata.get(&id) {
                    if metadata.is_published() {
                        published.push((id.clone(), item, metadata.clone()));
                    }
                }
            }
            published.sort_by_key(|(id, _, _)| **id);
            let total = published.len();
            Ok((cut(published, offset, limit), total))
        }
    }

    /// Publish or unpublish one item as a single step.
    ///
    /// Publishing touches three records — the published copy, the working copy that replaces
    /// it, and the status — and a reader that catches it half-applied sees a CMS that
    /// contradicts itself: content without the status that says it is live, or a status
    /// naming content that is still the old one. So this is one operation, not three calls
    /// that happen to be made together. `draft` is the working copy to promote, or `None`
    /// when there is nothing pending and only the status changes.
    fn apply_item_status(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        draft: Option<&CollectionItem>,
        metadata: &ItemMetadata,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// Claim a unique value for `item_id`, or report who already holds it.
    ///
    /// A point read and a conditional write, never a scan: this is the same shape as the
    /// username reservation, and it is what makes two saves of one value settle into one
    /// winner rather than both succeeding. Claiming a value the item already holds succeeds,
    /// so re-saving an item is not a conflict with itself.
    fn reserve_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> impl Future<Output = Result<Reservation, BoxError>> + Send;

    /// The item holding a unique value, if any.
    ///
    /// The same point read the reservation uses, so a lookup by slug is not a scan either.
    fn find_unique_value(
        &self,
        collection_name: &CollectionName,
        unique: &UniqueValue,
    ) -> impl Future<Output = Result<Option<CollectionItemId>, BoxError>> + Send;

    /// Give a value up, but only while `item_id` still holds it.
    ///
    /// Another item may have claimed it since (a reservation is released after the write that
    /// stopped holding it), and deleting that claim would hand the value to whoever asks next.
    fn release_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}
