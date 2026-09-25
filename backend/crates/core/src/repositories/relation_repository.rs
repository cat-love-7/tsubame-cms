use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use crate::models::owner::ItemOwner;
use crate::repositories::collection_repository::BoxError;

/// Boxed rather than a bare `async fn` so the trait stays object safe (`Arc<dyn RelationRepository>`
/// is what the services hold); the same shape as `webhook::NotifyFuture`.
pub type RelationReferencesFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ItemOwner>, BoxError>> + Send + 'a>>;

/// Boxed for the same reason (see [`RelationReferencesFuture`]).
pub type DetachFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ItemOwner>, BoxError>> + Send + 'a>>;

/// The relation index: which content references which content.
///
/// The index is **written with the content itself**. A save, a publish, a delete and a collection
/// delete each write the entries they change in the same transaction as the record they change
/// (the item write methods of
/// [`CollectionRepository`](crate::repositories::collection_repository::CollectionRepository) and
/// [`SinglePageRepository`](crate::repositories::single_page_repository::SinglePageRepository)),
/// because what asks the index - "may this be deleted?" - cannot afford to be told about a
/// reference that is not there any more, or miss one that is. The image index is the opposite
/// trade: it is written after the record, and a drift there only ever hides a user.
///
/// Both copies of a piece of content hold entries: the working copy is what an editor is in the
/// middle of and the published copy is what the site serves, and either is a reason to keep what
/// it points at. The index is a *set* of references, so the same target named by two fields is one
/// entry.
///
/// This trait is the reading half; nothing that writes needs it.
pub trait RelationRepository: Send + Sync + 'static {
    /// The content that references `target`, in a stable order.
    fn get_relation_references(&self, target: &ItemOwner) -> RelationReferencesFuture<'_>;

    /// Remove every reference to `target`, in both copies of every referrer, and answer with what
    /// was detached.
    ///
    /// See [`detach_references`] for the rule; an adapter supplies its storage, not the order the
    /// records are rewritten in.
    fn detach_references(&self, target: &ItemOwner) -> DetachFuture<'_>;
}

/// Remove every reference to `target` from the content that holds one.
///
/// This is `?detach=true`: the content on the other side is rewritten without the reference, in
/// both copies (the site serves one and the editor holds the other, and the index counts both), and
/// each referrer goes through the ordinary write methods - so the index follows it without anything
/// having to remember to do that here. Answers with what was detached.
///
/// A referrer that is gone by the time it is reached is skipped rather than reported: the caller is
/// about to delete `target` either way, and content that no longer exists holds nothing.
pub async fn detach_references<R>(
    repository: &R,
    target: &ItemOwner,
) -> Result<Vec<ItemOwner>, BoxError>
where
    R: RelationRepository
        + crate::repositories::collection_repository::CollectionRepository
        + crate::repositories::single_page_repository::SinglePageRepository,
{
    use crate::models::collection::{CollectionItemId, CollectionName};
    use crate::models::single_page::SinglePageName;

    let referrers = repository.get_relation_references(target).await?;
    let now = chrono::Utc::now();
    for referrer in &referrers {
        match referrer.kind {
            crate::models::owner::ItemOwnerKind::CollectionItem => {
                let Some(item_id) = referrer.item else {
                    continue;
                };
                let name = CollectionName::from(referrer.name.as_str());
                let id = CollectionItemId::from_u64(item_id);
                // The published copy first, then the working one: both have to stop naming the
                // target, or the index (which counts both) keeps the entry alive.
                let published = repository.get_collection_item(&name, &id).await?;
                let draft = repository.get_collection_item_draft(&name, &id).await?;
                if let Some(published) = published {
                    let stripped = published.without_reference(target);
                    repository
                        .update_collection_item(&name, &id, &stripped)
                        .await?;
                }
                if let Some(draft) = draft {
                    let stripped = draft.without_reference(target);
                    repository
                        .set_collection_item_draft(&name, &id, &stripped)
                        .await?;
                }
                // The content changed, so it says so - what a save records, without touching the
                // publication state. A build watching `updated_at` has to see this, and so does
                // anyone reading the delivery API's idea of when the content last changed.
                repository.touch_item_metadata(&name, &id, now).await?;
            }
            crate::models::owner::ItemOwnerKind::SinglePage => {
                let name = SinglePageName::from(referrer.name.as_str());
                let published = repository.get_single_page_item(&name).await?;
                let draft = repository.get_single_page_item_draft(&name).await?;
                if let Some(published) = published {
                    let stripped = published.without_reference(target);
                    repository.update_single_page_item(&name, &stripped).await?;
                }
                if let Some(draft) = draft {
                    let stripped = draft.without_reference(target);
                    repository
                        .set_single_page_item_draft(&name, &stripped)
                        .await?;
                }
                repository.touch_page_metadata(&name, now).await?;
            }
        }
    }
    Ok(referrers)
}

/// Which copy of a piece of content a write is storing, and what it will hold.
///
/// A record has two copies - the published one the site is served and the working one an editor is
/// holding - and the index counts both, because either is a reason to keep what the content points
/// at. An adapter working out the change says which copy it is writing and what that copy will
/// hold; the other copy is the one the write does not touch, and it is read from storage.
pub enum Written<'a, T> {
    /// The published record (`None` when it is going away).
    Published(Option<&'a T>),
    /// The working copy (`None` when it is going away).
    Draft(Option<&'a T>),
}

/// What one write does to the index: the entries it adds and the entries it drops.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RelationIndexChanges {
    pub added: Vec<ItemOwner>,
    pub removed: Vec<ItemOwner>,
}

impl RelationIndexChanges {
    /// The change between the index as it stands and what the content holds now.
    ///
    /// Compared as sets, because a relation has no order and two fields naming the same target are
    /// the one entry.
    pub fn between(current: &[ItemOwner], now: &[ItemOwner]) -> Self {
        let current: BTreeSet<&ItemOwner> = current.iter().collect();
        let now: BTreeSet<&ItemOwner> = now.iter().collect();
        RelationIndexChanges {
            added: now
                .difference(&current)
                .map(|owner| (*owner).clone())
                .collect(),
            removed: current
                .difference(&now)
                .map(|owner| (*owner).clone())
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }

    /// How many references this changes, each of which costs an entry in each direction.
    ///
    /// A backend whose transaction has a ceiling (DynamoDB's hundred writes) uses this to refuse a
    /// write that could not be indexed atomically, rather than writing half of it.
    pub fn reference_count(&self) -> usize {
        self.added.len() + self.removed.len()
    }
}

/// An empty index, for tests that are not about references.
///
/// A deployment always reads the real one; this exists so a service test can be built without a
/// storage behind it, the way `StaticRelationTargets::none()` does for the schema save.
#[cfg(test)]
pub struct NoRelations;

#[cfg(test)]
impl RelationRepository for NoRelations {
    fn get_relation_references(&self, _target: &ItemOwner) -> RelationReferencesFuture<'_> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn detach_references(&self, _target: &ItemOwner) -> DetachFuture<'_> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, id: u64) -> ItemOwner {
        ItemOwner::collection_item(name, id)
    }

    #[test]
    fn the_change_is_the_difference_of_two_sets() {
        let current = vec![item("authors", 1), item("authors", 2)];
        let now = vec![item("authors", 2), item("authors", 3)];

        let changes = RelationIndexChanges::between(&current, &now);

        assert_eq!(changes.added, vec![item("authors", 3)]);
        assert_eq!(changes.removed, vec![item("authors", 1)]);
        assert_eq!(changes.reference_count(), 2);
        assert!(!changes.is_empty());
    }

    #[test]
    fn an_order_change_is_not_a_change() {
        let current = vec![item("authors", 2), item("authors", 1)];
        let now = vec![item("authors", 1), item("authors", 2)];

        assert!(RelationIndexChanges::between(&current, &now).is_empty());
    }

    #[test]
    fn a_page_and_a_collection_of_the_same_name_are_different_targets() {
        let current = vec![ItemOwner::single_page("home")];
        let now = vec![item("home", 1)];

        let changes = RelationIndexChanges::between(&current, &now);

        assert_eq!(changes.added, vec![item("home", 1)]);
        assert_eq!(changes.removed, vec![ItemOwner::single_page("home")]);
    }

    #[test]
    fn everything_goes_when_nothing_is_left() {
        let current = vec![item("authors", 1), ItemOwner::single_page("home")];
        let changes = RelationIndexChanges::between(&current, &[]);

        assert!(changes.added.is_empty());
        assert_eq!(changes.removed.len(), 2);
    }
}
