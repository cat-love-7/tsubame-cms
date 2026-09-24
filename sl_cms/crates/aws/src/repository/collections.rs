//! Collections and their items: schemas, published copies, working copies and metadata.
//!
//! Split out of the module file because the collection side is the largest part of the adapter -
//! it is where the shared helpers (`read`, `write`, `transact`) were first written, and they had
//! grown around a whole trait implementation.

use super::*;
use crate::repository::relations;
use sl_cms_core::models::owner::ItemOwner;
use sl_cms_core::models::schema::SchemaSettings;
use sl_cms_core::models::values::referenced_items;
use sl_cms_core::repositories::relation_repository::RelationIndexChanges;
use sl_cms_core::repositories::relation_repository::Written;

impl AwsRepository {
    /// What one write changes in the relation index for a collection item.
    ///
    /// The index is the union of both copies: the working copy is what an editor is holding and the
    /// published record is what the site serves, and either is a reason to keep what it points at.
    async fn collection_index_changes(
        inner: &Inner,
        collection_name: &CollectionName,
        item_id: u64,
        written: Written<'_, CollectionItem>,
    ) -> Result<RelationIndexChanges, BoxError> {
        let owner = ItemOwner::collection_item(collection_name.as_str(), item_id);
        let partition = key::collection(collection_name);
        let read_copy = |data: Option<String>| -> Result<Option<CollectionItem>, BoxError> {
            match data {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        };
        let published = match written {
            Written::Published(copy) => copy.cloned(),
            Written::Draft(_) => read_copy(read(inner, &partition, &key::item(item_id)).await?)?,
        };
        let draft = match written {
            Written::Published(_) => {
                read_copy(read(inner, &partition, &key::draft(item_id)).await?)?
            }
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

    /// The writes that record `changes` in the index, with the room the caller has left.
    fn index_writes(
        inner: &Inner,
        owner: &ItemOwner,
        changes: &RelationIndexChanges,
        record_writes: usize,
    ) -> Result<Vec<aws_sdk_dynamodb::types::TransactWriteItem>, BoxError> {
        relations::index_writes(
            inner,
            owner,
            changes,
            relations::MAX_TRANSACT_WRITES - record_writes,
        )
    }
}

impl CollectionRepository for AwsRepository {
    async fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Option<CollectionSchema>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        match read(&inner, &key::collection(&name), key::SCHEMA).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn list_collection_names(&self) -> Result<Vec<CollectionName>, BoxError> {
        let inner = self.inner.clone();
        let records = list(&inner, key::COLLECTION_INDEX, "").await?;
        records
            .into_iter()
            .map(|(sk, _)| Ok(CollectionName::from(sk.as_str())))
            .collect()
    }

    async fn get_collection_settings(
        &self,
        collection_name: &CollectionName,
    ) -> Result<SchemaSettings, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        match read(&inner, &key::collection(&name), key::SETTINGS).await? {
            Some(data) => Ok(AwsRepository::decode(&data)?),
            // No record: the collection was never given settings, which is the default.
            None => Ok(SchemaSettings::default()),
        }
    }

    async fn set_collection_settings(
        &self,
        collection_name: &CollectionName,
        settings: &SchemaSettings,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(settings)?;
        write(&inner, &key::collection(&name), key::SETTINGS, &data).await
    }

    async fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(schema)?;
        // The schema and the entry that lists it go together: a collection whose entry is missing
        // is invisible (nothing lists it, and nothing would ever put the entry back).
        let outcome = transact(
            &inner,
            vec![
                put_in_transaction(&inner, &key::collection(&name), key::SCHEMA, &data)?,
                put_in_transaction(&inner, key::COLLECTION_INDEX, name.as_str(), name.as_str())?,
            ],
        )
        .await?;
        match outcome {
            TransactOutcome::Applied => Ok(()),
            TransactOutcome::Refused { write } => Err(format!(
                "saving the schema for {name} was refused by write {write}, which has no condition"
            )
            .into()),
        }
    }

    async fn delete_collection(&self, collection_name: &CollectionName) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        for (sk, _) in list(&inner, &partition, "").await? {
            // An item that is going takes its index entries with it, in both directions. One write
            // each rather than a transaction: deleting a collection is already many round trips,
            // and an entry left behind only ever names content that is gone.
            if let Some(id) = sk
                .strip_prefix("item#")
                .and_then(|id| id.parse::<u64>().ok())
            {
                let owner = ItemOwner::collection_item(name.as_str(), id);
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
            }
            remove(&inner, &partition, &sk).await?;
        }
        remove(&inner, key::COLLECTION_INDEX, name.as_str()).await
    }

    async fn list_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<
        Vec<(
            CollectionItemId,
            sl_cms_core::models::collection::CollectionItem,
        )>,
        BoxError,
    > {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        let mut items = Vec::new();
        for (sk, data) in list(&inner, &partition, "item#").await? {
            let id = sk.trim_start_matches("item#").parse::<u64>()?;
            items.push((
                CollectionItemId::from_u64(id),
                AwsRepository::decode(&data)?,
            ));
        }
        Ok(items)
    }

    async fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<sl_cms_core::models::collection::CollectionItem>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        match read(&inner, &key::collection(&name), &key::item(id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn add_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &sl_cms_core::models::collection::CollectionItem,
    ) -> Result<u64, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(item_data)?;
        let partition = key::collection(&name);
        let id = next_id(&inner, &partition).await?;
        // The item and the index entries for what it references land together: a reference the
        // index does not know about is one a delete would not refuse. A new id has no entries yet
        // (ids are never reused), so nothing has to be read first.
        let owner = ItemOwner::collection_item(name.as_str(), id);
        let changes = RelationIndexChanges::between(&[], &referenced_items(item_data));
        let mut writes = vec![put_in_transaction(
            &inner,
            &partition,
            &key::item(id),
            &data,
        )?];
        writes.extend(Self::index_writes(&inner, &owner, &changes, 1)?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "creating an item")?;
        Ok(id)
    }

    async fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &sl_cms_core::models::collection::CollectionItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(item_data)?;
        let partition = key::collection(&name);
        let owner = ItemOwner::collection_item(name.as_str(), id);
        let changes =
            Self::collection_index_changes(&inner, &name, id, Written::Published(Some(item_data)))
                .await?;
        let mut writes = vec![put_in_transaction(
            &inner,
            &partition,
            &key::item(id),
            &data,
        )?];
        writes.extend(Self::index_writes(&inner, &owner, &changes, 1)?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "updating an item")
    }

    async fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let partition = key::collection(&name);
        // The published copy, the working copy and the metadata go together: removing one and
        // leaving the others shows up as a half-deleted item (a draft with nothing published, or a
        // status naming content that is gone). One transaction, so it is all of them or none.
        let owner = ItemOwner::collection_item(name.as_str(), id);
        let mut writes = vec![
            delete_in_transaction(&inner, &partition, &key::item(id))?,
            delete_in_transaction(&inner, &partition, &key::draft(id))?,
            delete_in_transaction(&inner, &partition, &key::metadata(id))?,
        ];
        // Both copies go, so the index entries for what they referenced go with them, in the same
        // transaction: nothing may be left pointing at a piece of content that is gone.
        writes.extend(
            relations::forget_writes(
                &inner,
                &owner,
                relations::MAX_TRANSACT_WRITES - writes.len(),
            )
            .await?,
        );
        let outcome = transact(&inner, writes).await?;
        match outcome {
            TransactOutcome::Applied => Ok(()),
            TransactOutcome::Refused { write } => Err(format!(
                "deleting item {id} was refused by write {write}, which has no condition"
            )
            .into()),
        }
    }

    async fn get_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<sl_cms_core::models::collection::CollectionItem>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        match read(&inner, &key::collection(&name), &key::draft(id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn set_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &sl_cms_core::models::collection::CollectionItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        // The canonical rendering, so a promotion can compare the record with what the publisher
        // read (see `apply_item_status`).
        let data = canonical_draft(item_data);
        let partition = key::collection(&name);
        let owner = ItemOwner::collection_item(name.as_str(), id);
        // The published record is the copy this save does not touch, and the index holds what both
        // copies reference.
        let changes =
            Self::collection_index_changes(&inner, &name, id, Written::Draft(Some(item_data)))
                .await?;
        let mut writes = vec![put_in_transaction(
            &inner,
            &partition,
            &key::draft(id),
            &data,
        )?];
        writes.extend(Self::index_writes(&inner, &owner, &changes, 1)?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "saving an item")
    }

    async fn delete_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let partition = key::collection(&name);
        // The draft goes, so what it referenced stops being indexed - the published record (which
        // this write does not touch) is what the index keeps.
        let owner = ItemOwner::collection_item(name.as_str(), id);
        let changes =
            Self::collection_index_changes(&inner, &name, id, Written::Draft(None)).await?;
        let mut writes = vec![delete_in_transaction(&inner, &partition, &key::draft(id))?];
        writes.extend(Self::index_writes(&inner, &owner, &changes, 1)?);
        let outcome = transact(&inner, writes).await?;
        relations::expect_applied(outcome, "discarding a working copy")
    }

    async fn list_collection_item_drafts(
        &self,
        collection_name: &CollectionName,
    ) -> Result<
        Vec<(
            CollectionItemId,
            sl_cms_core::models::collection::CollectionItem,
        )>,
        BoxError,
    > {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        let mut drafts = Vec::new();
        for (sk, data) in list(&inner, &partition, "draft#").await? {
            let id = sk.trim_start_matches("draft#").parse::<u64>()?;
            drafts.push((
                CollectionItemId::from_u64(id),
                AwsRepository::decode(&data)?,
            ));
        }
        Ok(drafts)
    }

    async fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<ItemMetadata>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        match read(&inner, &key::collection(&name), &key::metadata(id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn set_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(metadata)?;
        write(&inner, &key::collection(&name), &key::metadata(id), &data).await
    }

    async fn touch_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let partition = key::collection(&name);
        let sort = key::metadata(id);
        // Only `updated_at` changes, and the rest of what is on the record is whatever it is by
        // the time this lands - a publish that happened in between is not undone by a save.
        let changed = change_record::<ItemMetadata>(&inner, &partition, &sort, |metadata| {
            *metadata = metadata.touched(now)
        })
        .await?;
        if changed.is_none() {
            // No record yet: the touch is what creates it, exactly as it would have been written
            // before (nothing else about the item was recorded either).
            let fresh = AwsRepository::encode(&ItemMetadata::default().touched(now))?;
            write(&inner, &partition, &sort, &fresh).await?;
        }
        Ok(())
    }

    async fn set_item_dates(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        dates: &ItemDates,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let partition = key::collection(&name);
        let sort = key::metadata(id);
        // The patch is applied to the record as it is by the time this lands, for the same reason
        // a touch is: a publish that happened in between keeps its publication time.
        let changed = change_record::<ItemMetadata>(&inner, &partition, &sort, |metadata| {
            *metadata = metadata.with_dates(dates)
        })
        .await?;
        if changed.is_none() {
            // Nothing recorded yet, so there is nothing to leave alone.
            let fresh = AwsRepository::encode(&ItemMetadata::default().with_dates(dates))?;
            write(&inner, &partition, &sort, &fresh).await?;
        }
        Ok(())
    }

    async fn list_published_items_page(
        &self,
        collection_name: &CollectionName,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<(Vec<(CollectionItemId, CollectionItem, ItemMetadata)>, usize), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        // Walk the statuses, not the content. A metadata record is a few hundred bytes and
        // the item behind it is not, so a page of a 10k-item collection reads 10k small
        // records and the content of one page — instead of everything.
        let mut total = 0usize;
        let mut window = Vec::new();
        let mut start_key = None;
        loop {
            let mut request = inner
                .client
                .query()
                .table_name(&inner.table)
                .key_condition_expression("#pk = :pk AND begins_with(#sk, :prefix)")
                .expression_attribute_names("#pk", "pk")
                .expression_attribute_names("#sk", "sk")
                .expression_attribute_values(":pk", AttributeValue::S(partition.clone()))
                .expression_attribute_values(":prefix", AttributeValue::S("meta#".to_string()))
                .consistent_read(true);
            if let Some(key) = start_key.take() {
                request = request.set_exclusive_start_key(Some(key));
            }
            let answer = request
                .send()
                .await
                .map_err(|e| format!("dynamodb query failed: {}", describe(&e)))?;
            for record in answer.items() {
                let sk = record
                    .get("sk")
                    .and_then(|value| value.as_s().ok())
                    .cloned()
                    .unwrap_or_default();
                let data = record
                    .get("data")
                    .and_then(|value| value.as_s().ok())
                    .cloned()
                    .unwrap_or_default();
                let metadata: ItemMetadata = AwsRepository::decode(&data)?;
                if !metadata.is_published() {
                    continue;
                }
                total += 1;
                if total <= offset || window.len() >= limit.unwrap_or(usize::MAX) {
                    // Past the window: still counted, but its content is not read.
                    continue;
                }
                let id = sk.trim_start_matches("meta#").parse::<u64>()?;
                match read(&inner, &partition, &key::item(id)).await? {
                    Some(item) => window.push((
                        CollectionItemId::from_u64(id),
                        AwsRepository::decode(&item)?,
                        metadata,
                    )),
                    // A status with no content behind it: counting it would make `total` a
                    // number the pages cannot add up to.
                    None => total -= 1,
                }
            }
            start_key = answer.last_evaluated_key().cloned();
            if start_key.is_none() {
                break;
            }
        }
        Ok((window, total))
    }

    async fn reserve_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> Result<Reservation, BoxError> {
        let inner = self.inner.clone();
        let collection = collection_name.clone();
        let unique = unique.clone();
        let id = *item_id;
        let partition = key::unique(collection.as_str(), &unique.field);
        // One write, and what it means depends on what was there: creating the entry is a claim
        // this caller can give back, while finding it already ours is not. The write is conditional
        // on the entry being *absent*, so a success is unambiguous, and a refusal is followed by a
        // read that says whether it is ours, somebody else's, or gone.
        //
        // Gone is possible - the holder released it between the write and the read - and is not a
        // refusal at all, so the write is tried again rather than reporting a claim that was never
        // made. (A loop rather than a recursion: the window is a round trip wide, and after a
        // handful of attempts something is wrong a caller should hear about.)
        for _ in 0..RESERVE_ATTEMPTS {
            let answer = inner
                .client
                .put_item()
                .table_name(&inner.table)
                .item("pk", AttributeValue::S(partition.clone()))
                .item("sk", AttributeValue::S(unique.value.clone()))
                .item("data", AttributeValue::S(id.to_string()))
                .condition_expression("attribute_not_exists(pk)")
                .send()
                .await;
            match answer {
                Ok(_) => return Ok(Reservation::Claimed),
                Err(e)
                    if e.as_service_error().and_then(|e| e.code())
                        == Some("ConditionalCheckFailedException") =>
                {
                    match read(&inner, &partition, &unique.value).await? {
                        // Ours already: not a conflict with itself, and nothing to give back.
                        Some(owner) if owner == id.to_string() => {
                            return Ok(Reservation::AlreadyHeld);
                        }
                        // Say who holds it, so the refusal can name the item rather than only the
                        // value.
                        Some(owner) => {
                            return Ok(Reservation::Taken {
                                owner: CollectionItemId::from_u64(owner.parse()?),
                            });
                        }
                        None => continue,
                    }
                }
                Err(e) => return Err(format!("dynamodb put_item failed: {}", describe(&e)).into()),
            }
        }
        Err(format!(
            "the unique index for {} kept changing under {} after {RESERVE_ATTEMPTS} attempts",
            unique.field, unique.value
        )
        .into())
    }

    async fn list_unique_values(
        &self,
        collection_name: &CollectionName,
        field: &str,
    ) -> Result<Vec<(CollectionItemId, UniqueValue)>, BoxError> {
        let inner = self.inner.clone();
        let partition = key::unique(collection_name.as_str(), field);
        let mut held = Vec::new();
        for (value, owner) in list(&inner, &partition, "").await? {
            held.push((
                CollectionItemId::from_u64(owner.parse()?),
                UniqueValue {
                    field: field.to_string(),
                    value,
                },
            ));
        }
        Ok(held)
    }

    async fn find_unique_value(
        &self,
        collection_name: &CollectionName,
        unique: &UniqueValue,
    ) -> Result<Option<CollectionItemId>, BoxError> {
        let inner = self.inner.clone();
        let partition = key::unique(collection_name.as_str(), &unique.field);
        match read(&inner, &partition, &unique.value).await? {
            Some(owner) => Ok(Some(CollectionItemId::from_u64(owner.parse()?))),
            None => Ok(None),
        }
    }

    async fn release_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let partition = key::unique(collection_name.as_str(), &unique.field);
        // Only while it is still ours: the value may have been claimed by another item since
        // this one stopped holding it, and deleting that claim would hand it to a third.
        let answer = inner
            .client
            .delete_item()
            .table_name(&inner.table)
            .key("pk", AttributeValue::S(partition))
            .key("sk", AttributeValue::S(unique.value.clone()))
            .condition_expression("#data = :id")
            .expression_attribute_names("#data", "data")
            .expression_attribute_values(":id", AttributeValue::S(item_id.to_string()))
            .send()
            .await;
        match answer {
            Ok(_) => Ok(()),
            // Not ours any more, or already gone: nothing to release.
            Err(e)
                if e.as_service_error().and_then(|e| e.code())
                    == Some("ConditionalCheckFailedException") =>
            {
                Ok(())
            }
            Err(e) => Err(format!("dynamodb delete_item failed: {}", describe(&e)).into()),
        }
    }

    async fn apply_item_status(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        draft: Option<&CollectionItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = *item_id;
        let draft = draft.cloned();
        let data = AwsRepository::encode(metadata)?;
        let partition = key::collection(&name);
        let metadata_key = key::metadata(*id);
        let Some(draft) = draft else {
            // Nothing pending: only the status changes, and one write is already atomic.
            return write(&inner, &partition, &metadata_key, &data).await;
        };
        let draft_data = AwsRepository::encode(&draft)?;
        // One transaction. A published copy that exists while its working copy is still
        // there, or a status naming content that has not landed, is a CMS that disagrees
        // with itself — the delivery API reads the copy and the admin list reads the
        // status. DynamoDB's transaction costs two writes per item, which is the price of
        // never showing that state.
        // The draft is stored in its canonical rendering, which is what makes this condition a
        // content comparison rather than a byte comparison: a save that landed after the caller
        // read the working copy changed that rendering, and publishing the older content would
        // delete the newer save.
        let expected = canonical_draft(&draft);
        // Publishing promotes the working copy, so what it references is what the index holds
        // afterwards - the draft is gone, and the published record it replaces is not read.
        let owner = ItemOwner::collection_item(name.as_str(), *id);
        let changes =
            Self::collection_index_changes(&inner, &name, *id, Written::Published(Some(&draft)))
                .await?;
        let mut writes = vec![
            put_in_transaction(&inner, &partition, &key::item(*id), &draft_data)?,
            delete_draft_in_transaction(&inner, &partition, &key::draft(*id), &expected)?,
            put_in_transaction(&inner, &partition, &metadata_key, &data)?,
        ];
        // Appended, so the positions the refusals below name do not move.
        writes.extend(Self::index_writes(&inner, &owner, &changes, 3)?);
        let outcome = transact(&inner, writes).await?;
        match outcome {
            TransactOutcome::Applied => Ok(()),
            // Position 1 is the draft delete above.
            TransactOutcome::Refused { write: 1 } => Err(Box::new(ApplyStatusError::DraftChanged)),
            TransactOutcome::Refused { write } => Err(format!(
                "publishing item {id} was refused by write {write}, which has no condition"
            )
            .into()),
        }
    }

    async fn list_item_metadata(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        let mut metadata = Vec::new();
        for (sk, data) in list(&inner, &partition, "meta#").await? {
            let id = sk.trim_start_matches("meta#").parse::<u64>()?;
            metadata.push((
                CollectionItemId::from_u64(id),
                AwsRepository::decode(&data)?,
            ));
        }
        Ok(metadata)
    }
}

/// How many times a unique reservation retries the conditional write before giving up.
///
/// Each retry means the value was released between a refused write and the read that followed it,
/// which is a round trip wide; more than a handful in a row is a sign of something else.
const RESERVE_ATTEMPTS: usize = 5;
