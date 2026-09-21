use crate::repository::relations;
use crate::repository::{
    COLLECTION_SCHEMA_STORE, COLLECTION_SETTINGS_STORE, DRAFT_STORE, METADATA_STORE, RkvRepository,
    UNIQUE_STORE, collection_draft_prefix, collection_item_draft_key, collection_item_metadata_key,
    collection_metadata_prefix, unique_key,
};
use rkv::{StoreOptions, Value};
use sl_cms_core::models::collection::{
    CollectionItem, CollectionItemId, CollectionName, CollectionSchema,
};
use sl_cms_core::models::item_status::{ItemDates, ItemMetadata};
use sl_cms_core::models::owner::ItemOwner;
use sl_cms_core::models::schema::SchemaSettings;
use sl_cms_core::models::values::referenced_items;
use sl_cms_core::repositories::collection_repository::{
    ApplyStatusError, CollectionRepository, Reservation, UniqueValue, canonical_draft,
};
use sl_cms_core::repositories::relation_repository::{RelationIndexChanges, Written};
use std::error::Error;

impl RkvRepository {
    /// What one write changes in the relation index for a collection item.
    ///
    /// The index is the union of both copies: the working copy is what an editor is holding and the
    /// published record is what the site serves, and either is a reason to keep what it points at.
    /// `written` says which copy this is and what it will hold; the other one is read here, inside
    /// the caller's lock, so the two cannot drift apart.
    #[allow(clippy::too_many_arguments)]
    fn collection_index_changes(
        &self,
        relation_store: &relations::RelationStore,
        reader: &rkv::Reader<rkv::backend::SafeModeRoTransaction<'_>>,
        item_store: &rkv::SingleStore<rkv::backend::SafeModeDatabase>,
        draft_store: &rkv::SingleStore<rkv::backend::SafeModeDatabase>,
        collection_name: &CollectionName,
        item_id: u64,
        written: Written<'_, CollectionItem>,
    ) -> Result<RelationIndexChanges, Box<dyn Error + Send + Sync + 'static>> {
        let owner = ItemOwner::collection_item(collection_name.as_str(), item_id);
        let read_draft = |reader| -> Result<Option<CollectionItem>, Box<dyn Error + Send + Sync>> {
            let key = collection_item_draft_key(collection_name.as_str(), item_id);
            match draft_store.get(reader, key.as_bytes())? {
                Some(Value::Str(stored)) => Ok(Some(serde_json::from_str(&stored)?)),
                _ => Ok(None),
            }
        };
        let read_item = |reader| -> Result<Option<CollectionItem>, Box<dyn Error + Send + Sync>> {
            match item_store.get(reader, &item_id.to_le_bytes())? {
                Some(Value::Str(stored)) => Ok(Some(serde_json::from_str(&stored)?)),
                _ => Ok(None),
            }
        };
        let published = match written {
            Written::Published(copy) => copy.cloned(),
            Written::Draft(_) => read_item(reader)?,
        };
        let draft = match written {
            Written::Published(_) => read_draft(reader)?,
            Written::Draft(copy) => copy.cloned(),
        };
        let mut now = published.as_ref().map(referenced_items).unwrap_or_default();
        if let Some(draft) = &draft {
            now.extend(referenced_items(draft));
        }
        now.sort();
        now.dedup();
        let current = relations::entries_of(relation_store, reader, &owner)?;
        Ok(RelationIndexChanges::between(&current, &now))
    }
}

impl CollectionRepository for RkvRepository {
    async fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Option<CollectionSchema>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(COLLECTION_SCHEMA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, collection_name.as_bytes())? {
            Some(Value::Str(s)) => {
                let schema: CollectionSchema = serde_json::from_str(&s)?;
                Ok(Some(schema))
            }
            _ => Ok(None),
        }
    }
    async fn list_collection_names(
        &self,
    ) -> Result<Vec<CollectionName>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(COLLECTION_SCHEMA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let mut collections = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(_s))) = result {
                collections.push(str::from_utf8(&key)?.into());
            }
        }
        Ok(collections)
    }
    async fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(COLLECTION_SCHEMA_STORE, StoreOptions::create())?;
        let schema_str = serde_json::to_string(schema)?;
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            collection_name.as_bytes(),
            &Value::Str(&schema_str),
        )?;
        writer.commit()?;
        Ok(())
    }
    async fn get_collection_settings(
        &self,
        collection_name: &CollectionName,
    ) -> Result<SchemaSettings, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(COLLECTION_SETTINGS_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, collection_name.as_bytes())? {
            Some(Value::Str(stored)) => Ok(serde_json::from_str(&stored)?),
            // A collection that was never given any settings answers the default, which is what
            // every collection did before settings existed.
            _ => Ok(SchemaSettings::default()),
        }
    }
    async fn set_collection_settings(
        &self,
        collection_name: &CollectionName,
        settings: &SchemaSettings,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(COLLECTION_SETTINGS_STORE, StoreOptions::create())?;
        let stored = serde_json::to_string(settings)?;
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            collection_name.as_bytes(),
            &Value::Str(&stored),
        )?;
        writer.commit()?;
        Ok(())
    }
    async fn delete_collection(
        &self,
        collection_name: &CollectionName,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(COLLECTION_SCHEMA_STORE, StoreOptions::create())?;
        let item_store = env.open_single(
            format!("collection_{}", collection_name).as_str(),
            StoreOptions::create(),
        )?;
        // Open (and, on first use, create) the metadata store *before* starting any
        // transaction. LMDB rejects a database handle that was created after the
        // transaction using it began, so opening it later would make DELETE fail.
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let settings_store = env.open_single(COLLECTION_SETTINGS_STORE, StoreOptions::create())?;
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, collection_name.as_bytes())?;
        let reader = env.read()?;
        // Settings go with the definition they belong to - and, like the counter below, only if
        // there are any: rkv reports deleting an absent key as an error.
        if settings_store
            .get(&reader, collection_name.as_bytes())?
            .is_some()
        {
            settings_store.delete(&mut writer, collection_name.as_bytes())?;
        }
        for result in item_store.iter_start(&reader)? {
            if let Ok((key, Value::Str(_s))) = result {
                item_store.delete(&mut writer, &key)?;
                // An item that is gone holds nothing, so what it referenced has to stop being
                // indexed against it - in both directions, in this same transaction.
                let id = *CollectionItemId::from_le_bytes(key.try_into()?);
                relations::forget(
                    &relation_store,
                    &reader,
                    &mut writer,
                    &ItemOwner::collection_item(collection_name.as_str(), id),
                )?;
            }
        }
        // The per-collection item counter only exists once an item has been added, and
        // rkv reports deleting an absent key as an error ("key/value pair not found"),
        // which would turn DELETE on an empty collection into a 500.
        if self
            .counter_store
            .get(&reader, collection_name.as_bytes())?
            .is_some()
        {
            self.counter_store
                .delete(&mut writer, collection_name.as_bytes())?;
        }
        // Drop the items' draft/published metadata too, so a collection recreated under
        // the same name cannot inherit stale statuses.
        let prefix = collection_metadata_prefix(collection_name.as_str());
        let mut stale = Vec::new();
        for result in metadata_store.iter_from(&reader, prefix.as_bytes())? {
            if let Ok((key, Value::Str(_))) = result {
                if !key.starts_with(prefix.as_bytes()) {
                    break;
                }
                stale.push(key.to_vec());
            }
        }
        for key in stale {
            metadata_store.delete(&mut writer, &key)?;
        }
        // The unpublished working copies go with them.
        let draft_prefix = collection_draft_prefix(collection_name.as_str());
        let mut stale_drafts = Vec::new();
        for result in draft_store.iter_from(&reader, draft_prefix.as_bytes())? {
            if let Ok((key, Value::Str(_))) = result {
                if !key.starts_with(draft_prefix.as_bytes()) {
                    break;
                }
                stale_drafts.push(key.to_vec());
            }
        }
        for key in stale_drafts {
            draft_store.delete(&mut writer, &key)?;
        }
        writer.commit()?;
        Ok(())
    }
    async fn list_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItem)>, Box<dyn Error + Send + Sync + 'static>>
    {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let reader = env.read()?;
        let mut items = Vec::new();
        for result in collection_store.iter_start(&reader)? {
            if let Ok((key, Value::Str(s))) = result {
                let item_id = CollectionItemId::from_le_bytes(key.try_into()?);
                let item: CollectionItem = serde_json::from_str(&s)?;
                items.push((item_id, item));
            }
        }
        Ok(items)
    }
    async fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<CollectionItem>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let reader = env.read()?;
        let key_bytes = item_id.to_le_bytes();
        match collection_store.get(&reader, &key_bytes)? {
            Some(Value::Str(s)) => {
                let item: CollectionItem = serde_json::from_str(&s)?;
                Ok(Some(item))
            }
            _ => Ok(None),
        }
    }

    async fn add_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &CollectionItem,
    ) -> Result<u64, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        // Opened before the transaction begins: LMDB rejects a handle created after it.
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        // Acquire the write transaction BEFORE the read snapshot used to read the
        // counter. Two concurrent writers would otherwise both read the same counter
        // value and then write the same item id, silently overwriting each other.
        let mut writer = env.write()?;
        let reader = env.read()?;

        let current_id = match self
            .counter_store
            .get(&reader, collection_name.as_bytes())?
        {
            Some(Value::U64(id)) => id,
            _ => 0,
        };
        let new_id = current_id + 1;
        // The index entries for the new item, in the same transaction as the item: what asks the
        // index (a delete) must never see a reference the item does not hold, or miss one it does.
        // A new id has no entries yet - ids are never reused - so nothing has to be read first.
        let owner = ItemOwner::collection_item(collection_name.as_str(), new_id);
        let changes = RelationIndexChanges::between(&[], &referenced_items(item_data));
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, &new_id.to_le_bytes(), &Value::Str(&item_str))?;
        relations::apply(&relation_store, &mut writer, &owner, &changes)?;
        self.counter_store
            .put(&mut writer, collection_name.as_bytes(), &Value::U64(new_id))?;
        writer.commit()?;
        Ok(new_id)
    }

    async fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &CollectionItem,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        // Opened before the transaction begins (see `add_collection_item`).
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        let reader = env.read()?;
        let changes = self.collection_index_changes(
            &relation_store,
            &reader,
            &collection_store,
            &draft_store,
            collection_name,
            **item_id,
            Written::Published(Some(item_data)),
        )?;
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, &item_id.to_le_bytes(), &Value::Str(&item_str))?;
        relations::apply(
            &relation_store,
            &mut writer,
            &ItemOwner::collection_item(collection_name.as_str(), **item_id),
            &changes,
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        // Opened before the transactions, for the reason given in `delete_collection`.
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        // Opened before the transactions (see `delete_collection`).
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        // Writer first, then the read snapshot (see `add_collection_item`).
        let mut writer = env.write()?;
        let reader = env.read()?;
        collection_store.delete(&mut writer, &item_id.to_le_bytes())?;
        // Both copies go, so the index entries go with them.
        relations::forget(
            &relation_store,
            &reader,
            &mut writer,
            &ItemOwner::collection_item(collection_name.as_str(), **item_id),
        )?;

        // Most items are never published, so a missing metadata record is normal.
        let key = collection_item_metadata_key(collection_name.as_str(), **item_id);
        if metadata_store.get(&reader, key.as_bytes())?.is_some() {
            metadata_store.delete(&mut writer, key.as_bytes())?;
        }
        // The working copy goes with the item, whether or not it was ever published.
        let draft_key = collection_item_draft_key(collection_name.as_str(), **item_id);
        if draft_store.get(&reader, draft_key.as_bytes())?.is_some() {
            draft_store.delete(&mut writer, draft_key.as_bytes())?;
        }

        writer.commit()?;
        Ok(())
    }

    async fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<ItemMetadata>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = collection_item_metadata_key(collection_name.as_str(), **item_id);
        match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(s)) => Ok(Some(serde_json::from_str(&s)?)),
            _ => Ok(None),
        }
    }

    async fn set_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        metadata: &ItemMetadata,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = collection_item_metadata_key(collection_name.as_str(), **item_id);
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            key.as_bytes(),
            &Value::Str(&serde_json::to_string(metadata)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn list_published_items_page(
        &self,
        collection_name: &CollectionName,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<
        (Vec<(CollectionItemId, CollectionItem, ItemMetadata)>, usize),
        Box<dyn Error + Send + Sync + 'static>,
    > {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let reader = env.read()?;
        let prefix = collection_metadata_prefix(collection_name.as_str());

        // Walk the statuses, not the content: a metadata record is a few bytes and the item
        // behind it is not, so a page of a large collection no longer reads the collection.
        // The status is also the only thing that knows an item is published — the published
        // copy of an unpublished item is still there.
        let mut total = 0usize;
        let mut window = Vec::new();
        for result in metadata_store.iter_from(&reader, prefix.as_bytes())? {
            let Ok((key, Value::Str(s))) = result else {
                continue;
            };
            let key = str::from_utf8(&key)?;
            let Some(id) = key.strip_prefix(&prefix) else {
                break;
            };
            let Ok(id) = id.parse::<u64>() else {
                continue;
            };
            let metadata: ItemMetadata = serde_json::from_str(&s)?;
            if !metadata.is_published() {
                continue;
            }
            total += 1;
            if total > offset && window.len() < limit.unwrap_or(usize::MAX) {
                let item_id = CollectionItemId::from_u64(id);
                match collection_store.get(&reader, &item_id.to_le_bytes())? {
                    Some(Value::Str(item)) => {
                        window.push((item_id, serde_json::from_str(&item)?, metadata))
                    }
                    // A status with no content behind it: the item is gone, and counting it
                    // would make `total` a number the pages cannot add up to.
                    _ => total -= 1,
                }
            }
        }
        Ok((window, total))
    }

    async fn apply_item_status(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        draft: Option<&CollectionItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store =
            env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        // Opened before the transaction begins: LMDB rejects a handle created after it.
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        // Opened before the transaction begins (see `add_collection_item`).
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;

        let metadata_key = collection_item_metadata_key(collection_name.as_str(), **item_id);
        // The promotion is conditional on the working copy still being the one the caller read: a
        // save that landed in between is newer than the content about to be published, and deleting
        // it would throw that work away. The storage lock makes this read and the write below one
        // step, so nothing can slip in between them either.
        if let Some(draft) = draft {
            let reader = env.read()?;
            let draft_key = collection_item_draft_key(collection_name.as_str(), **item_id);
            let stored = match draft_store.get(&reader, draft_key.as_bytes())? {
                Some(Value::Str(stored)) => stored,
                // Nothing there: the working copy this caller read is gone (another publish has
                // promoted it, or the item was deleted), which is the same answer as a changed one.
                _ => return Err(Box::new(ApplyStatusError::DraftChanged)),
            };
            // Readable in either shape: records written before the canonical rendering existed were
            // saved in hash order, and comparing their content is what matters.
            let stored: CollectionItem = serde_json::from_str(&stored)?;
            if canonical_draft(&stored) != canonical_draft(draft) {
                return Err(Box::new(ApplyStatusError::DraftChanged));
            }
        }
        // What the index has to hold afterwards: a publish promotes the working copy, so what it
        // references is what the item references now, and there is no working copy left to add to
        // it. Unpublishing writes the status only - both copies stay where they are - so nothing
        // about the index changes.
        let changes = match draft {
            Some(draft) => {
                let reader = env.read()?;
                let current = relations::entries_of(
                    &relation_store,
                    &reader,
                    &ItemOwner::collection_item(collection_name.as_str(), **item_id),
                )?;
                RelationIndexChanges::between(&current, &referenced_items(draft))
            }
            None => RelationIndexChanges::default(),
        };
        // One write transaction: the published copy, the working copy it replaces, the status and
        // the index land together, so no reader can see a half-applied publish.
        let mut writer = env.write()?;
        if let Some(draft) = draft {
            collection_store.put(
                &mut writer,
                &item_id.to_le_bytes(),
                &Value::Str(&serde_json::to_string(draft)?),
            )?;
            draft_store.delete(
                &mut writer,
                collection_item_draft_key(collection_name.as_str(), **item_id).as_bytes(),
            )?;
        }
        metadata_store.put(
            &mut writer,
            metadata_key.as_bytes(),
            &Value::Str(&serde_json::to_string(metadata)?),
        )?;
        relations::apply(
            &relation_store,
            &mut writer,
            &ItemOwner::collection_item(collection_name.as_str(), **item_id),
            &changes,
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn touch_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        // Read and write under the one lock: a save must not write back the publication state it
        // read before a publish that landed in between.
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = collection_item_metadata_key(collection_name.as_str(), **item_id);
        let reader = env.read()?;
        let metadata = match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<ItemMetadata>(&s)?,
            _ => ItemMetadata::default(),
        }
        .touched(now);
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            key.as_bytes(),
            &Value::Str(&serde_json::to_string(&metadata)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn set_item_dates(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        dates: &ItemDates,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        // The same one lock as a touch, for the same reason: what this writes is a patch, and the
        // record it is applied to must not be one a publish has since replaced.
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = collection_item_metadata_key(collection_name.as_str(), **item_id);
        let reader = env.read()?;
        let metadata = match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<ItemMetadata>(&s)?,
            _ => ItemMetadata::default(),
        }
        .with_dates(dates);
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            key.as_bytes(),
            &Value::Str(&serde_json::to_string(&metadata)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn list_item_metadata(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let prefix = collection_metadata_prefix(collection_name.as_str());

        let mut items = Vec::new();
        // Keys are sorted, so everything with this prefix is contiguous.
        for result in store.iter_from(&reader, prefix.as_bytes())? {
            if let Ok((key, Value::Str(s))) = result {
                let key = str::from_utf8(&key)?;
                let Some(id) = key.strip_prefix(&prefix) else {
                    break;
                };
                let Ok(id) = id.parse::<u64>() else {
                    continue;
                };
                items.push((CollectionItemId::from_u64(id), serde_json::from_str(&s)?));
            }
        }
        Ok(items)
    }

    async fn get_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<CollectionItem>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = collection_item_draft_key(collection_name.as_str(), **item_id);
        match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(s)) => Ok(Some(serde_json::from_str(&s)?)),
            _ => Ok(None),
        }
    }

    async fn list_collection_item_drafts(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItem)>, Box<dyn Error + Send + Sync + 'static>>
    {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let prefix = collection_draft_prefix(collection_name.as_str());

        let mut items = Vec::new();
        // Keys are sorted, so everything with this prefix is contiguous.
        for result in store.iter_from(&reader, prefix.as_bytes())? {
            if let Ok((key, Value::Str(s))) = result {
                let key = str::from_utf8(&key)?;
                let Some(id) = key.strip_prefix(&prefix) else {
                    break;
                };
                let Ok(id) = id.parse::<u64>() else {
                    continue;
                };
                items.push((CollectionItemId::from_u64(id), serde_json::from_str(&s)?));
            }
        }
        Ok(items)
    }

    async fn set_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &CollectionItem,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let key = collection_item_draft_key(collection_name.as_str(), **item_id);
        // Opened before the transaction begins: LMDB rejects a handle created after it.
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let item_store = env.open_single(
            format!("collection_{collection_name}").as_str(),
            StoreOptions::create(),
        )?;
        let mut writer = env.write()?;
        let reader = env.read()?;
        let changes = self.collection_index_changes(
            &relation_store,
            &reader,
            &item_store,
            &store,
            collection_name,
            **item_id,
            Written::Draft(Some(item_data)),
        )?;
        // Stored in the canonical rendering, so a promotion can compare it with what the publisher
        // read without the two renderings differing only in the order a `HashMap` was walked.
        store.put(
            &mut writer,
            key.as_bytes(),
            &Value::Str(&canonical_draft(item_data)),
        )?;
        relations::apply(
            &relation_store,
            &mut writer,
            &ItemOwner::collection_item(collection_name.as_str(), **item_id),
            &changes,
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn reserve_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> Result<Reservation, Box<dyn Error + Send + Sync + 'static>> {
        // The storage lock is what makes check-then-write one step here: this adapter has a
        // single writer, so no other save can slip between the read and the write.
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(UNIQUE_STORE, StoreOptions::create())?;
        let key = unique_key(collection_name.as_str(), unique);
        let reader = env.read()?;
        match store.get(&reader, key.as_bytes())? {
            // Already ours: a save that keeps its value is not a conflict with itself, and there is
            // nothing for the caller to give back if its own write then fails.
            Some(Value::Str(owner)) if owner == item_id.to_string() => Ok(Reservation::AlreadyHeld),
            Some(Value::Str(owner)) => Ok(Reservation::Taken {
                owner: CollectionItemId::from_u64(owner.parse()?),
            }),
            _ => {
                let mut writer = env.write()?;
                store.put(
                    &mut writer,
                    key.as_bytes(),
                    &Value::Str(&item_id.to_string()),
                )?;
                writer.commit()?;
                Ok(Reservation::Claimed)
            }
        }
    }

    async fn list_unique_values(
        &self,
        collection_name: &CollectionName,
        field: &str,
    ) -> Result<Vec<(CollectionItemId, UniqueValue)>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(UNIQUE_STORE, StoreOptions::create())?;
        let prefix = format!("unique:{}\u{1f}{}\u{1f}", collection_name.as_str(), field);
        let reader = env.read()?;
        let mut held = Vec::new();
        for result in store.iter_from(&reader, prefix.as_bytes())? {
            let Ok((key, Value::Str(owner))) = result else {
                continue;
            };
            let key = str::from_utf8(&key)?;
            let Some(value) = key.strip_prefix(&prefix) else {
                break;
            };
            held.push((
                CollectionItemId::from_u64(owner.parse()?),
                UniqueValue {
                    field: field.to_string(),
                    value: value.to_string(),
                },
            ));
        }
        Ok(held)
    }

    async fn find_unique_value(
        &self,
        collection_name: &CollectionName,
        unique: &UniqueValue,
    ) -> Result<Option<CollectionItemId>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(UNIQUE_STORE, StoreOptions::create())?;
        let key = unique_key(collection_name.as_str(), unique);
        let reader = env.read()?;
        match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(owner)) => Ok(Some(CollectionItemId::from_u64(owner.parse()?))),
            _ => Ok(None),
        }
    }

    async fn release_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(UNIQUE_STORE, StoreOptions::create())?;
        let key = unique_key(collection_name.as_str(), unique);
        let reader = env.read()?;
        // Only if it is still ours: the value may have been claimed by another item since this
        // one stopped holding it.
        if store.get(&reader, key.as_bytes())? == Some(Value::Str(&item_id.to_string())) {
            let mut writer = env.write()?;
            store.delete(&mut writer, key.as_bytes())?;
            writer.commit()?;
        }
        Ok(())
    }

    async fn delete_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = collection_item_draft_key(collection_name.as_str(), **item_id);
        // rkv reports deleting an absent key as an error, and most items have no draft.
        if store.get(&reader, key.as_bytes())?.is_some() {
            let mut writer = env.write()?;
            store.delete(&mut writer, key.as_bytes())?;
            writer.commit()?;
        }
        Ok(())
    }
}
