//! Single pages: a schema, a published copy, an unpublished working copy and metadata.
//!
//! A page has no items and no ids, so unlike a collection it needs no counter and no secondary
//! index of item ids: the page name *is* the key, and everything about the page lives in one
//! partition (`pk = page#<name>`), which is also what makes deleting a page one query and a
//! handful of deletes.

use super::*;
use crate::repository::relations;
use sl_cms_core::models::owner::ItemOwner;
use sl_cms_core::models::values::referenced_items;
use sl_cms_core::repositories::relation_repository::RelationIndexChanges;
use sl_cms_core::repositories::relation_repository::Written;

impl AwsRepository {
    /// What one write changes in the relation index for a page.
    ///
    /// The same rule as a collection item (see `CollectionRepository`'s helper): the index is the
    /// union of the page's published copy and its working copy, and the copy this write does not
    /// touch is read from the table before the transaction.
    async fn page_index_changes(
        inner: &Inner,
        page_name: &SinglePageName,
        written: Written<'_, SinglePageItem>,
    ) -> Result<RelationIndexChanges, BoxError> {
        let owner = ItemOwner::single_page(page_name.as_str());
        let partition = key::page(page_name);
        let read_copy = |data: Option<String>| -> Result<Option<SinglePageItem>, BoxError> {
            match data {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        };
        let published = match written {
            Written::Published(copy) => copy.cloned(),
            Written::Draft(_) => read_copy(read(inner, &partition, key::ITEM).await?)?,
        };
        let draft = match written {
            Written::Published(_) => read_copy(read(inner, &partition, key::DRAFT).await?)?,
            Written::Draft(copy) => copy.cloned(),
        };
        let mut now = published.as_ref().map(referenced_items).unwrap_or_default();
        if let Some(draft) = &draft {
            now.extend(referenced_items(draft));
        }
        now.sort();
        now.dedup();
        let current = relations::entries_of(inner, &owner).await?;
        Ok(RelationIndexChanges::between(&current, &now))
    }
}
use sl_cms_core::models::item_status::{ItemDates, ItemMetadata};
use sl_cms_core::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};
use sl_cms_core::repositories::single_page_repository::SinglePageRepository;

impl SinglePageRepository for AwsRepository {
    async fn get_single_page_schema(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageSchema>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        match read(&inner, &key::page(&name), key::SCHEMA).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn list_all_page_names(&self) -> Result<Vec<SinglePageName>, BoxError> {
        let inner = self.inner.clone();
        let records = list(&inner, key::PAGE_INDEX, "").await?;
        records
            .into_iter()
            .map(|(sk, _)| Ok(SinglePageName::from(sk.as_str())))
            .collect()
    }

    async fn add_single_page_schema(
        &self,
        page_name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(schema)?;
        write(&inner, &key::page(&name), key::SCHEMA, &data).await?;
        // The index entry is what `list_all_page_names` queries; it is written after the
        // schema so a name is never listed before the page exists.
        write(&inner, key::PAGE_INDEX, name.as_str(), name.as_str()).await
    }

    async fn delete_single_page(&self, page_name: &SinglePageName) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let partition = key::page(&name);
        // Schema, published copy, working copy and metadata, in one pass. Any of them may
        // be absent (a page that was never published, or never edited), and deleting an
        // absent key is not an error.
        for (sk, _) in list(&inner, &partition, "").await? {
            remove(&inner, &partition, &sk).await?;
        }
        // Both copies go, so what the page referenced stops being indexed against it (one write
        // each: a page delete is already many round trips).
        let owner = ItemOwner::single_page(name.as_str());
        for target in relations::entries_of(&inner, &owner).await? {
            remove(
                &inner,
                &relations::owner_partition(&owner),
                &format!("{}{}", relations::REL, target.storage_key()),
            )
            .await?;
            remove(
                &inner,
                &relations::target_partition(&target),
                &format!("{}{}", relations::REF, owner.storage_key()),
            )
            .await?;
        }
        remove(&inner, key::PAGE_INDEX, name.as_str()).await
    }

    async fn get_single_page_item(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        match read(&inner, &key::page(&name), key::ITEM).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn update_single_page_item(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(item_data)?;
        let partition = key::page(&name);
        let owner = ItemOwner::single_page(name.as_str());
        let changes =
            Self::page_index_changes(&inner, &name, Written::Published(Some(item_data))).await?;
        let mut writes = vec![put_in_transaction(&inner, &partition, key::ITEM, &data)?];
        writes.extend(relations::index_writes(
            &inner,
            &owner,
            &changes,
            relations::MAX_TRANSACT_WRITES - 1,
        )?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "saving a page")
    }

    async fn get_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        match read(&inner, &key::page(&name), key::DRAFT).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn set_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        // Stored canonically, so a promotion can compare it with what the publisher read (see
        // `apply_page_status`).
        let data = canonical_draft(item_data);
        let partition = key::page(&name);
        let owner = ItemOwner::single_page(name.as_str());
        let changes =
            Self::page_index_changes(&inner, &name, Written::Draft(Some(item_data))).await?;
        let mut writes = vec![put_in_transaction(&inner, &partition, key::DRAFT, &data)?];
        writes.extend(relations::index_writes(
            &inner,
            &owner,
            &changes,
            relations::MAX_TRANSACT_WRITES - 1,
        )?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "saving a page")
    }

    async fn delete_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let partition = key::page(&name);
        // The working copy goes, so what it referenced stops being indexed; the published copy,
        // which this write does not touch, is what the index keeps.
        let owner = ItemOwner::single_page(name.as_str());
        let changes = Self::page_index_changes(&inner, &name, Written::Draft(None)).await?;
        let mut writes = vec![delete_in_transaction(&inner, &partition, key::DRAFT)?];
        writes.extend(relations::index_writes(
            &inner,
            &owner,
            &changes,
            relations::MAX_TRANSACT_WRITES - 1,
        )?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "discarding a page's working copy")
    }

    async fn get_page_metadata(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<ItemMetadata>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        match read(&inner, &key::page(&name), key::META).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn set_page_metadata(
        &self,
        page_name: &SinglePageName,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(metadata)?;
        write(&inner, &key::page(&name), key::META, &data).await
    }

    async fn touch_page_metadata(
        &self,
        page_name: &SinglePageName,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let partition = key::page(&name);
        // See `CollectionRepository::touch_item_metadata`: a save must not write back the
        // publication state it read before it.
        let changed = change_record::<ItemMetadata>(&inner, &partition, key::META, |metadata| {
            *metadata = metadata.touched(now)
        })
        .await?;
        if changed.is_none() {
            let fresh = AwsRepository::encode(&ItemMetadata::default().touched(now))?;
            write(&inner, &partition, key::META, &fresh).await?;
        }
        Ok(())
    }

    async fn set_page_dates(
        &self,
        page_name: &SinglePageName,
        dates: &ItemDates,
    ) -> Result<(), BoxError> {
        // See `CollectionRepository::set_item_dates`.
        let inner = self.inner.clone();
        let name = page_name.clone();
        let partition = key::page(&name);
        let changed = change_record::<ItemMetadata>(&inner, &partition, key::META, |metadata| {
            *metadata = metadata.with_dates(dates)
        })
        .await?;
        if changed.is_none() {
            let fresh = AwsRepository::encode(&ItemMetadata::default().with_dates(dates))?;
            write(&inner, &partition, key::META, &fresh).await?;
        }
        Ok(())
    }

    async fn apply_page_status(
        &self,
        page_name: &SinglePageName,
        draft: Option<&SinglePageItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let draft = draft.cloned();
        let data = AwsRepository::encode(metadata)?;
        let partition = key::page(&name);
        let Some(draft) = draft else {
            // Nothing pending: only the status changes.
            return write(&inner, &partition, key::META, &data).await;
        };
        let draft_data = AwsRepository::encode(&draft)?;
        // One transaction, for the reason in `CollectionRepository::apply_item_status`, and the
        // delete is conditional on the working copy still being the one the caller read: a save
        // landing in between would otherwise be deleted by the promotion.
        let expected = canonical_draft(&draft);
        // Publishing promotes the working copy, so what it references is what the index holds
        // afterwards - the draft is gone, and the published record it replaces is not read.
        let owner = ItemOwner::single_page(name.as_str());
        let changes =
            Self::page_index_changes(&inner, &name, Written::Published(Some(&draft))).await?;
        let mut writes = vec![
            put_in_transaction(&inner, &partition, key::ITEM, &draft_data)?,
            delete_draft_in_transaction(&inner, &partition, key::DRAFT, &expected)?,
            put_in_transaction(&inner, &partition, key::META, &data)?,
        ];
        // Appended, so the positions the refusals below name do not move.
        writes.extend(relations::index_writes(
            &inner,
            &owner,
            &changes,
            relations::MAX_TRANSACT_WRITES - 3,
        )?);
        let outcome = transact(&inner, writes).await?;
        match outcome {
            TransactOutcome::Applied => Ok(()),
            // Position 1 is the draft delete above.
            TransactOutcome::Refused { write: 1 } => Err(Box::new(ApplyStatusError::DraftChanged)),
            TransactOutcome::Refused { write } => Err(format!(
                "publishing page {name} was refused by write {write}, which has no condition"
            )
            .into()),
        }
    }
}
