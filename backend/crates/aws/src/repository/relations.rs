//! The relation index, shared by the collection and the page records.
//!
//! It is one index either way, and it lives in the same table as the content:
//!
//! * the partition `refs#<owner>` with sort keys `rel#<target>` answers "what does this content
//!   reference" (the image index's forward direction shares that partition, under `image#`), and
//! * the partition `rel#<target>` with sort keys `ref#<owner>` answers "what references this
//!   content".
//!
//! Every write that changes a record's references puts its index entries in the **same
//! transaction** as the record (see the item write methods of `collections` and `single_pages`),
//! because what asks the index - "may this be deleted?" - must not be told about a reference that
//! is gone, or miss one that is there. DynamoDB allows a hundred writes in one transaction, so a
//! save that would change more references than that is refused loudly rather than half-indexed;
//! see [`MAX_TRANSACT_WRITES`].

use super::{Inner, delete_in_transaction, list, put_in_transaction};
use tsubame_core::models::owner::ItemOwner;
use tsubame_core::repositories::collection_repository::BoxError;
use tsubame_core::repositories::relation_repository::{
    DetachFuture, RelationIndexChanges, RelationReferencesFuture, RelationRepository,
};

use crate::repository::{AwsRepository, TransactOutcome};

/// DynamoDB's ceiling on the writes in one transaction.
pub(crate) const MAX_TRANSACT_WRITES: usize = 100;

/// The forward direction's prefix inside an owner's own partition. The image index uses
/// `refs#<owner>` as well, with `image#` sort keys, so the two indexes share a partition.
pub(crate) const REL: &str = "rel#";
/// The inverse direction's records: `rel#<target>` / `ref#<owner>`.
pub(crate) const REF: &str = "ref#";

pub(crate) fn owner_partition(owner: &ItemOwner) -> String {
    format!("refs#{}", owner.storage_key())
}

pub(crate) fn target_partition(target: &ItemOwner) -> String {
    format!("rel#{}", target.storage_key())
}

/// The targets `owner` references, as the table has them.
pub(crate) async fn entries_of(
    inner: &Inner,
    owner: &ItemOwner,
) -> Result<Vec<ItemOwner>, BoxError> {
    let mut found: Vec<ItemOwner> = list(inner, &owner_partition(owner), REL)
        .await?
        .into_iter()
        .filter_map(|(sk, _)| ItemOwner::from_storage_key(sk.trim_start_matches(REL)))
        .collect();
    found.sort();
    Ok(found)
}

/// The content that references `target`.
pub(crate) async fn referring(
    inner: &Inner,
    target: &ItemOwner,
) -> Result<Vec<ItemOwner>, BoxError> {
    let mut found: Vec<ItemOwner> = list(inner, &target_partition(target), REF)
        .await?
        .into_iter()
        .filter_map(|(sk, _)| ItemOwner::from_storage_key(sk.trim_start_matches(REF)))
        .collect();
    found.sort();
    Ok(found)
}

/// The index writes for a change, as transaction items.
///
/// `room` is how many writes the caller's transaction has left, record writes included: a change
/// that does not fit is an error rather than a partly indexed save.
pub(crate) fn index_writes(
    inner: &Inner,
    owner: &ItemOwner,
    changes: &RelationIndexChanges,
    room: usize,
) -> Result<Vec<aws_sdk_dynamodb::types::TransactWriteItem>, BoxError> {
    // Two entries per reference, one in each direction.
    let needed = changes.reference_count() * 2;
    if needed > room {
        return Err(format!(
            "this save changes {} references, which needs {needed} index writes and the \
             transaction has room for {room}: split it into smaller saves",
            changes.reference_count()
        )
        .into());
    }
    let value = owner.storage_key();
    let mut writes = Vec::with_capacity(needed);
    for target in &changes.added {
        writes.push(put_in_transaction(
            inner,
            &owner_partition(owner),
            &format!("{REL}{}", target.storage_key()),
            &value,
        )?);
        writes.push(put_in_transaction(
            inner,
            &target_partition(target),
            &format!("{REF}{}", owner.storage_key()),
            &value,
        )?);
    }
    for target in &changes.removed {
        writes.push(delete_in_transaction(
            inner,
            &owner_partition(owner),
            &format!("{REL}{}", target.storage_key()),
        )?);
        writes.push(delete_in_transaction(
            inner,
            &target_partition(target),
            &format!("{REF}{}", owner.storage_key()),
        )?);
    }
    Ok(writes)
}

/// Drop every entry `owner` holds, as transaction items.
pub(crate) async fn forget_writes(
    inner: &Inner,
    owner: &ItemOwner,
    room: usize,
) -> Result<Vec<aws_sdk_dynamodb::types::TransactWriteItem>, BoxError> {
    let changes = RelationIndexChanges::between(&entries_of(inner, owner).await?, &[]);
    index_writes(inner, owner, &changes, room)
}

/// Read the index that the other half of the adapter writes.
impl RelationRepository for AwsRepository {
    fn get_relation_references(&self, target: &ItemOwner) -> RelationReferencesFuture<'_> {
        // The future borrows the repository, not the caller's target: a target is a name and an
        // id, so a copy is cheaper than making every caller hold one alive across the await.
        let target = target.clone();
        Box::pin(async move {
            let inner = self.inner.clone();
            referring(&inner, &target).await
        })
    }

    fn detach_references(&self, target: &ItemOwner) -> DetachFuture<'_> {
        let target = target.clone();
        Box::pin(async move {
            // The rule is the same whichever table holds the referrers, so it lives in the core;
            // this adapter supplies the storage the writes go to.
            tsubame_core::repositories::relation_repository::detach_references(self, &target).await
        })
    }
}

/// Apply a transaction whose writes are index entries and record writes together.
///
/// Every caller here has an unconditional transaction, so a refusal is a programming error rather
/// than something to report to an editor; the message says which write it was.
pub(crate) fn expect_applied(outcome: TransactOutcome, what: &str) -> Result<(), BoxError> {
    match outcome {
        TransactOutcome::Applied => Ok(()),
        TransactOutcome::Refused { write } => {
            Err(format!("{what} was refused by write {write}, which has no condition").into())
        }
    }
}
