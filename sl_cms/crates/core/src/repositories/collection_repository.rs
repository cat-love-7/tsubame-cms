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

/// Why a status change was refused in a way the caller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyStatusError {
    /// The working copy is no longer the one the caller read: a save landed in between, and
    /// promoting the older content would delete what that save wrote.
    ///
    /// The caller's answer is to read the item again and decide afresh - which is why this is
    /// separate from a storage failure, where retrying the same thing is what a caller cannot do.
    DraftChanged,
}

impl std::fmt::Display for ApplyStatusError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyStatusError::DraftChanged => write!(
                formatter,
                "the working copy changed after it was read"
            ),
        }
    }
}

impl std::error::Error for ApplyStatusError {}

/// A stable rendering of a working copy: what "still the same working copy" is decided by.
///
/// The values are held in a `HashMap`, so the same content can serialise in different orders and
/// comparing two renderings would compare hash seeds. Sorting every object's keys on the way makes
/// the rendering depend on the content alone, which is what the working copy is *stored* as and
/// what a promotion compares against it.
///
/// Serialising these values cannot fail (the keys are strings), and the fallback is only here so a
/// request path never panics; an empty rendering would only ever compare equal to another empty
/// one.
pub fn canonical_draft(draft: &CollectionItem) -> String {
    let value = serde_json::to_value(draft).unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&sorted(value)).unwrap_or_default()
}

/// The same value with every object's keys in order, at every depth.
fn sorted(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(entries) => {
            let mut keys: Vec<String> = entries.keys().cloned().collect();
            keys.sort();
            let mut ordered = serde_json::Map::new();
            for key in keys {
                let entry = entries.get(&key).cloned().unwrap_or(serde_json::Value::Null);
                ordered.insert(key, sorted(entry));
            }
            serde_json::Value::Object(ordered)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sorted).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::field::{FieldValue, FieldValueMap};
    use std::collections::HashMap;

    fn item(entries: &[(&str, &str)]) -> CollectionItem {
        FieldValueMap(
            entries
                .iter()
                .map(|(key, value)| (key.to_string(), FieldValue::Text(value.to_string())))
                .collect::<HashMap<_, _>>(),
            std::marker::PhantomData,
        )
    }

    /// The values live in a `HashMap`, so the same content renders in any order; the marker is
    /// what a publish compares, and comparing hash order would refuse publishes at random.
    #[test]
    fn the_same_working_copy_renders_the_same_way_whatever_order_it_was_built_in() {
        assert_eq!(
            canonical_draft(&item(&[("title", "kept"), ("subtitle", "also")])),
            canonical_draft(&item(&[("subtitle", "also"), ("title", "kept")]))
        );
    }

    #[test]
    fn a_changed_working_copy_renders_differently() {
        let before = item(&[("title", "before"), ("count", "1")]);
        assert_ne!(canonical_draft(&before), canonical_draft(&item(&[("title", "after"), ("count", "1")])));
        assert_ne!(canonical_draft(&before), canonical_draft(&item(&[("title", "before"), ("count", "2")])));
        // A field that disappeared is a change as much as one whose value did.
        assert_ne!(canonical_draft(&before), canonical_draft(&item(&[("title", "before")])));
    }

    #[test]
    fn nested_values_are_ordered_too() {
        // A composite inside an array, built in a different key order each time.
        let nested = |label: &str, other_first: bool| {
            let inner = |pairs: [(&str, &str); 2]| {
                FieldValueMap(
                    pairs
                        .iter()
                        .map(|(key, value)| (key.to_string(), FieldValue::Text(value.to_string())))
                        .collect::<HashMap<_, _>>(),
                    std::marker::PhantomData,
                )
            };
            let pairs = if other_first {
                [("label", label), ("count", "1")]
            } else {
                [("count", "1"), ("label", label)]
            };
            let mut item = item(&[("title", "kept")]);
            item.0.insert(
                "parts".to_string(),
                FieldValue::CompositeField(Some(crate::models::field::CompositeFieldValue {
                    id: "comp".into(),
                    values: inner(pairs),
                })),
            );
            item
        };
        assert_eq!(canonical_draft(&nested("one", true)), canonical_draft(&nested("one", false)));
        assert_ne!(canonical_draft(&nested("one", true)), canonical_draft(&nested("two", true)));
    }
}

/// What claiming a unique value did.
///
/// "It is ours now" and "it was ours already" are different answers, because only the first is
/// something a caller can give back: a schema save that fails has to release what *it* claimed,
/// and releasing a value the item already held would take the value away from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation {
    /// Nobody held it, and the item does now.
    Claimed,
    /// The item held it already: nothing about the index changed.
    AlreadyHeld,
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
    /// Record that the item changed, **without touching anything else on its record**.
    ///
    /// A save and a publish write the same record, and the save reads it before it writes. Writing
    /// back the whole record from that read undoes a publish that happened in between - the site
    /// goes back to the state before it, publication time and all. This is the write a save makes:
    /// the read and the write are one step, so nothing between them can be lost.
    fn touch_item_metadata(&self, collection_name: &CollectionName, item_id: &CollectionItemId, now: chrono::DateTime<chrono::Utc>) -> impl Future<Output = Result<(), BoxError>> + Send;
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
    ///
    /// The promotion is **conditional on the working copy still being `draft`**, compared by
    /// [`canonical_draft`] (which is what a save stores it as). A save that lands between the
    /// caller's read and this call would otherwise be deleted by the promotion: the editor's
    /// newest work would disappear while the screen said it had been published. An implementation
    /// that finds a different rendering answers [`ApplyStatusError::DraftChanged`] and changes
    /// nothing.
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
