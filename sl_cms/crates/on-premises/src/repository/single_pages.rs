use crate::repository::relations;
use crate::repository::{
    DRAFT_STORE, METADATA_STORE, RkvRepository, SINGLE_PAGE_ITEM_STORE, SINGLE_PAGE_SCHEMA_STORE,
    page_draft_key, page_metadata_key,
};
use rkv::{StoreOptions, Value};
use sl_cms_core::models::item_status::{ItemDates, ItemMetadata};
use sl_cms_core::models::owner::ItemOwner;
use sl_cms_core::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};
use sl_cms_core::models::values::referenced_items;
use sl_cms_core::repositories::collection_repository::{ApplyStatusError, canonical_draft};
use sl_cms_core::repositories::relation_repository::{RelationIndexChanges, Written};
use sl_cms_core::repositories::single_page_repository::SinglePageRepository;
use std::error::Error;

impl RkvRepository {
    /// What one write changes in the relation index for a page.
    ///
    /// The same rule as a collection item (see `CollectionRepository`'s helper): the index is the
    /// union of the page's published copy and its working copy. `written` says which copy this is
    /// and what it will hold; the other one is read here, inside the caller's lock.
    fn page_index_changes(
        &self,
        relation_store: &relations::RelationStore,
        reader: &rkv::Reader<rkv::backend::SafeModeRoTransaction<'_>>,
        item_store: &rkv::SingleStore<rkv::backend::SafeModeDatabase>,
        draft_store: &rkv::SingleStore<rkv::backend::SafeModeDatabase>,
        page_name: &SinglePageName,
        written: Written<'_, SinglePageItem>,
    ) -> Result<RelationIndexChanges, Box<dyn Error + Send + Sync + 'static>> {
        let owner = ItemOwner::single_page(page_name.as_str());
        let read_draft = |reader| -> Result<Option<SinglePageItem>, Box<dyn Error + Send + Sync>> {
            let key = page_draft_key(page_name.as_str());
            match draft_store.get(reader, key.as_bytes())? {
                Some(Value::Str(stored)) => Ok(Some(serde_json::from_str(&stored)?)),
                _ => Ok(None),
            }
        };
        let read_item = |reader| -> Result<Option<SinglePageItem>, Box<dyn Error + Send + Sync>> {
            match item_store.get(reader, page_name.as_bytes())? {
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

impl SinglePageRepository for RkvRepository {
    async fn get_single_page_schema(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageSchema>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(SINGLE_PAGE_SCHEMA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, page_name.as_bytes())? {
            Some(Value::Str(s)) => {
                let schema: SinglePageSchema = serde_json::from_str(&s)?;
                Ok(Some(schema))
            }
            _ => Ok(None),
        }
    }
    async fn list_all_page_names(
        &self,
    ) -> Result<Vec<SinglePageName>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(SINGLE_PAGE_SCHEMA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let mut pages = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(_s))) = result {
                pages.push(str::from_utf8(&key)?.into());
            }
        }
        Ok(pages)
    }
    async fn add_single_page_schema(
        &self,
        page_name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(SINGLE_PAGE_SCHEMA_STORE, StoreOptions::create())?;
        let schema_str = serde_json::to_string(schema)?;
        let mut writer = env.write()?;
        store.put(&mut writer, page_name.as_bytes(), &Value::Str(&schema_str))?;
        writer.commit()?;
        Ok(())
    }
    async fn delete_single_page(
        &self,
        page_name: &SinglePageName,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(SINGLE_PAGE_SCHEMA_STORE, StoreOptions::create())?;
        let item_store = env.open_single(SINGLE_PAGE_ITEM_STORE, StoreOptions::create())?;
        // Opened before the transactions: LMDB rejects a database handle created after
        // the transaction that uses it began.
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        // Opened before the transactions, for the reason given in the page's own delete.
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        let reader = env.read()?;
        store.delete(&mut writer, page_name.as_bytes())?;
        relations::forget(
            &relation_store,
            &reader,
            &mut writer,
            &ItemOwner::single_page(page_name.as_str()),
        )?;
        // A page that was never published has no published copy, and rkv reports deleting
        // an absent key as an error, which would turn DELETE into a 500.
        if item_store.get(&reader, page_name.as_bytes())?.is_some() {
            item_store.delete(&mut writer, &page_name.as_bytes())?;
        }
        // Single pages have no id counter (no item-creation path exists), so there is
        // deliberately nothing to remove from `counter_store` here. Deleting an absent
        // key would fail with "key/value pair not found" and surface as a 500.
        //
        // The metadata record may legitimately not exist (never published).
        let metadata_key = page_metadata_key(page_name.as_str());
        if metadata_store
            .get(&reader, metadata_key.as_bytes())?
            .is_some()
        {
            metadata_store.delete(&mut writer, metadata_key.as_bytes())?;
        }
        // The unpublished working copy goes with the page.
        let draft_key = page_draft_key(page_name.as_str());
        if draft_store.get(&reader, draft_key.as_bytes())?.is_some() {
            draft_store.delete(&mut writer, draft_key.as_bytes())?;
        }
        writer.commit()?;
        Ok(())
    }

    async fn apply_page_status(
        &self,
        page_name: &SinglePageName,
        draft: Option<&SinglePageItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let item_store = env.open_single(SINGLE_PAGE_ITEM_STORE, StoreOptions::create())?;
        // Opened before the transaction begins (see `delete_single_page`).
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;

        let metadata_key = page_metadata_key(page_name.as_str());
        // One write transaction, as for collection items, and the promotion is conditional on the
        // working copy still being the one the caller read: a save that landed in between is newer
        // than the content about to be published, and deleting it would throw that work away.
        if let Some(draft) = draft {
            let reader = env.read()?;
            let draft_key = page_draft_key(page_name.as_str());
            let stored = match draft_store.get(&reader, draft_key.as_bytes())? {
                Some(Value::Str(stored)) => stored,
                _ => return Err(Box::new(ApplyStatusError::DraftChanged)),
            };
            let stored: SinglePageItem = serde_json::from_str(&stored)?;
            if canonical_draft(&stored) != canonical_draft(draft) {
                return Err(Box::new(ApplyStatusError::DraftChanged));
            }
        }
        // What the index has to hold afterwards: a publish promotes the working copy, and there is
        // no working copy left to add to it. Unpublishing writes the status only, so nothing about
        // the index changes.
        let changes = match draft {
            Some(draft) => {
                let reader = env.read()?;
                self.page_index_changes(
                    &relation_store,
                    &reader,
                    &item_store,
                    &draft_store,
                    page_name,
                    Written::Published(Some(draft)),
                )?
            }
            None => RelationIndexChanges::default(),
        };
        let mut writer = env.write()?;
        if let Some(draft) = draft {
            item_store.put(
                &mut writer,
                page_name.as_bytes(),
                &Value::Str(&serde_json::to_string(draft)?),
            )?;
            draft_store.delete(&mut writer, page_draft_key(page_name.as_str()).as_bytes())?;
        }
        metadata_store.put(
            &mut writer,
            metadata_key.as_bytes(),
            &Value::Str(&serde_json::to_string(metadata)?),
        )?;
        relations::apply(
            &relation_store,
            &mut writer,
            &ItemOwner::single_page(page_name.as_str()),
            &changes,
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn get_page_metadata(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<ItemMetadata>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = page_metadata_key(page_name.as_str());
        match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(s)) => Ok(Some(serde_json::from_str(&s)?)),
            _ => Ok(None),
        }
    }

    async fn set_page_metadata(
        &self,
        page_name: &SinglePageName,
        metadata: &ItemMetadata,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = page_metadata_key(page_name.as_str());
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            key.as_bytes(),
            &Value::Str(&serde_json::to_string(metadata)?),
        )?;
        writer.commit()?;
        Ok(())
    }
    async fn touch_page_metadata(
        &self,
        page_name: &SinglePageName,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        // See `CollectionRepository::touch_item_metadata`.
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = page_metadata_key(page_name.as_str());
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

    async fn set_page_dates(
        &self,
        page_name: &SinglePageName,
        dates: &ItemDates,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        // See `CollectionRepository::set_item_dates`.
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = page_metadata_key(page_name.as_str());
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

    async fn get_single_page_item(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let collection_store = env.open_single(SINGLE_PAGE_ITEM_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        match collection_store.get(&reader, page_name.as_bytes())? {
            Some(Value::Str(s)) => {
                let item: SinglePageItem = serde_json::from_str(&s)?;
                Ok(Some(item))
            }
            _ => Ok(None),
        }
    }
    async fn update_single_page_item(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let collection_store = env.open_single(SINGLE_PAGE_ITEM_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        // Opened before the transaction begins (see `delete_single_page`).
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        let reader = env.read()?;
        let changes = self.page_index_changes(
            &relation_store,
            &reader,
            &collection_store,
            &draft_store,
            page_name,
            Written::Published(Some(item_data)),
        )?;
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, page_name.as_bytes(), &Value::Str(&item_str))?;
        relations::apply(
            &relation_store,
            &mut writer,
            &ItemOwner::single_page(page_name.as_str()),
            &changes,
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn get_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = page_draft_key(page_name.as_str());
        match store.get(&reader, key.as_bytes())? {
            Some(Value::Str(s)) => Ok(Some(serde_json::from_str(&s)?)),
            _ => Ok(None),
        }
    }

    async fn set_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let key = page_draft_key(page_name.as_str());
        // Opened before the transaction begins (see `delete_single_page`).
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let item_store = env.open_single(SINGLE_PAGE_ITEM_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        let reader = env.read()?;
        let changes = self.page_index_changes(
            &relation_store,
            &reader,
            &item_store,
            &store,
            page_name,
            Written::Draft(Some(item_data)),
        )?;
        // Stored in the canonical rendering, so a promotion can compare it with what the publisher
        // read (see `apply_page_status`).
        store.put(
            &mut writer,
            key.as_bytes(),
            &Value::Str(&canonical_draft(item_data)),
        )?;
        relations::apply(
            &relation_store,
            &mut writer,
            &ItemOwner::single_page(page_name.as_str()),
            &changes,
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn delete_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        // Opened before the transaction begins: LMDB rejects a handle created after it.
        let relation_store =
            env.open_single(relations::RELATION_REFS_STORE, StoreOptions::create())?;
        let item_store = env.open_single(SINGLE_PAGE_ITEM_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = page_draft_key(page_name.as_str());
        if store.get(&reader, key.as_bytes())?.is_some() {
            let changes = self.page_index_changes(
                &relation_store,
                &reader,
                &item_store,
                &store,
                page_name,
                Written::Draft(None),
            )?;
            let mut writer = env.write()?;
            store.delete(&mut writer, key.as_bytes())?;
            relations::apply(
                &relation_store,
                &mut writer,
                &ItemOwner::single_page(page_name.as_str()),
                &changes,
            )?;
            writer.commit()?;
        }
        Ok(())
    }
}
