//! The relation index, shared by the collection and the page stores.
//!
//! It is one index either way: a collection item referencing a page and a page referencing a
//! collection item land in the same store, because what asks it - "may this be deleted?" - asks
//! about a piece of content, not about the store it is in.
//!
//! Both directions live in the one store, as for images:
//!
//! * `<owner>|rel|<target>` answers "what does this content reference", and
//! * `rel|<target>|<owner>` answers "what references this content".
//!
//! A target is named exactly the way an owner is (`models::owner::ItemOwner`), so the two halves
//! of a key cannot be confused: a collection called `home` and a page called `home` are different
//! strings.

use rkv::backend::{SafeModeDatabase, SafeModeRoTransaction, SafeModeRwTransaction};
use rkv::{Reader, StoreOptions, Value, Writer};

use sl_cms_core::models::owner::ItemOwner;
use sl_cms_core::repositories::collection_repository::BoxError;
use sl_cms_core::repositories::relation_repository::{
    DetachFuture, RelationIndexChanges, RelationReferencesFuture, RelationRepository,
};

use crate::repository::RkvRepository;

/// Store holding the relation index.
pub(crate) const RELATION_REFS_STORE: &str = "relation_refs";

pub(crate) type RelationStore = rkv::SingleStore<SafeModeDatabase>;

/// The entries one owner holds: `<owner>|rel|<target>`.
pub(crate) fn owner_prefix(owner: &ItemOwner) -> String {
    format!("{}|rel|", owner.storage_key())
}

/// The entries one target has: `rel|<target>|<owner>`.
pub(crate) fn target_prefix(target: &ItemOwner) -> String {
    format!("rel|{}|", target.storage_key())
}

fn owner_key(owner: &ItemOwner, target: &ItemOwner) -> String {
    format!("{}{}", owner_prefix(owner), target.storage_key())
}

fn target_key(target: &ItemOwner, owner: &ItemOwner) -> String {
    format!("{}{}", target_prefix(target), owner.storage_key())
}

/// The targets `owner` references, as the store has them.
pub(crate) fn entries_of(
    store: &RelationStore,
    reader: &Reader<SafeModeRoTransaction<'_>>,
    owner: &ItemOwner,
) -> Result<Vec<ItemOwner>, BoxError> {
    let prefix = owner_prefix(owner);
    let mut found = Vec::new();
    for result in store.iter_from(reader, prefix.as_bytes())? {
        let Ok((key, _)) = result else { continue };
        let key = std::str::from_utf8(&key)?;
        let Some(rest) = key.strip_prefix(&prefix) else {
            break;
        };
        if let Some(target) = ItemOwner::from_storage_key(rest) {
            found.push(target);
        }
    }
    found.sort();
    Ok(found)
}

/// Write what one save changed, in the caller's transaction.
///
/// Both directions are written for every entry: an index that answered one of them and not the
/// other would be worse than no index, because a delete would believe it.
pub(crate) fn apply(
    store: &RelationStore,
    writer: &mut Writer<SafeModeRwTransaction<'_>>,
    owner: &ItemOwner,
    changes: &RelationIndexChanges,
) -> Result<(), BoxError> {
    let owner_value = owner.storage_key();
    for target in &changes.added {
        store.put(
            writer,
            owner_key(owner, target).as_bytes(),
            &Value::Str(&owner_value),
        )?;
        store.put(
            writer,
            target_key(target, owner).as_bytes(),
            &Value::Str(&owner_value),
        )?;
    }
    for target in &changes.removed {
        store.delete(writer, owner_key(owner, target).as_bytes())?;
        store.delete(writer, target_key(target, owner).as_bytes())?;
    }
    Ok(())
}

/// Drop every entry `owner` holds, in the caller's transaction.
///
/// Deleting a piece of content is what this is for: the inverse entries have to go with the
/// forward ones, or the index would keep naming content that is gone.
pub(crate) fn forget(
    store: &RelationStore,
    reader: &Reader<SafeModeRoTransaction<'_>>,
    writer: &mut Writer<SafeModeRwTransaction<'_>>,
    owner: &ItemOwner,
) -> Result<(), BoxError> {
    let changes = RelationIndexChanges::between(&entries_of(store, reader, owner)?, &[]);
    apply(store, writer, owner, &changes)
}

/// The content that references `target`.
pub(crate) fn referring(
    store: &RelationStore,
    reader: &Reader<SafeModeRoTransaction<'_>>,
    target: &ItemOwner,
) -> Result<Vec<ItemOwner>, BoxError> {
    let prefix = target_prefix(target);
    let mut found = Vec::new();
    for result in store.iter_from(reader, prefix.as_bytes())? {
        let Ok((key, _)) = result else { continue };
        let key = std::str::from_utf8(&key)?;
        let Some(rest) = key.strip_prefix(&prefix) else {
            break;
        };
        if let Some(owner) = ItemOwner::from_storage_key(rest) {
            found.push(owner);
        }
    }
    found.sort();
    Ok(found)
}

impl RelationRepository for RkvRepository {
    fn get_relation_references(&self, target: &ItemOwner) -> RelationReferencesFuture<'_> {
        // The future borrows the repository, not the caller's target: a target is a name and an
        // id, so a copy is cheaper than making every caller hold one alive across the await.
        let target = target.clone();
        Box::pin(async move {
            let _guard = self.begin();
            let env = self.rkv.read().map_err(|e| e.to_string())?;
            let store = env.open_single(RELATION_REFS_STORE, StoreOptions::create())?;
            let reader = env.read()?;
            referring(&store, &reader, &target)
        })
    }

    fn detach_references(&self, target: &ItemOwner) -> DetachFuture<'_> {
        let target = target.clone();
        Box::pin(async move {
            // The rule is the same whichever store holds the referrers, so it lives in the core;
            // this adapter supplies the storage the writes go to.
            sl_cms_core::repositories::relation_repository::detach_references(self, &target).await
        })
    }
}
