use sl_cms_core::models::item_status::ItemMetadata;
use sl_cms_core::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};
use crate::repository::{page_draft_key, page_metadata_key, Repository, DRAFT_STORE, METADATA_STORE};
use sl_cms_core::repositories::single_page_repository::SinglePageRepository;
use rkv::{StoreOptions, Value};
use std::error::Error;


impl SinglePageRepository for Repository {
    async fn get_single_page_schema(&self, page_name: &SinglePageName) -> Result<Option<SinglePageSchema>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("single_page_schema", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, page_name.as_bytes())? {
            Some(Value::Str(s)) => {
                let schema: SinglePageSchema = serde_json::from_str(&s)?;
                Ok(Some(schema))
            },
            _ => Ok(None),
        }
    }
    async fn list_all_page_names(&self) -> Result<Vec<SinglePageName>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("single_page_schema", StoreOptions::create())?;
        let reader = env.read()?;
        let mut pages = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(_s))) = result {
                pages.push(str::from_utf8(&key)?.into());
            }
        }
        Ok(pages)
    }
    async fn add_single_page_schema(&self, page_name: &SinglePageName, schema: &SinglePageSchema) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e|e.to_string())?;
        let store = env.open_single("single_page_schema", StoreOptions::create())?;
        let schema_str = serde_json::to_string(schema)?;
        let mut writer = env.write()?;
        store.put(&mut writer, page_name.as_bytes(), &Value::Str(&schema_str))?;
        writer.commit()?;
        Ok(())
    }
    async fn delete_single_page(&self, page_name: &SinglePageName) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("single_page_schema", StoreOptions::create())?;
        let item_store = env.open_single("single_page_item", StoreOptions::create())?;
        // Opened before the transactions: LMDB rejects a database handle created after
        // the transaction that uses it began.
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        let reader = env.read()?;
        store.delete(&mut writer, page_name.as_bytes())?;
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
        if metadata_store.get(&reader, metadata_key.as_bytes())?.is_some() {
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
        let item_store = env.open_single("single_page_item", StoreOptions::create())?;
        // Opened before the transaction begins (see `delete_single_page`).
        let metadata_store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let draft_store = env.open_single(DRAFT_STORE, StoreOptions::create())?;

        let metadata_key = page_metadata_key(page_name.as_str());
        // One write transaction, as for collection items.
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
        writer.commit()?;
        Ok(())
    }

    async fn get_page_metadata(&self, page_name: &SinglePageName) -> Result<Option<ItemMetadata>, Box<dyn Error + Send + Sync + 'static>> {
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

    async fn set_page_metadata(&self, page_name: &SinglePageName, metadata: &ItemMetadata) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(METADATA_STORE, StoreOptions::create())?;
        let key = page_metadata_key(page_name.as_str());
        let mut writer = env.write()?;
        store.put(&mut writer, key.as_bytes(), &Value::Str(&serde_json::to_string(metadata)?))?;
        writer.commit()?;
        Ok(())
    }
    async fn get_single_page_item(&self, page_name: &SinglePageName) -> Result<Option<SinglePageItem>,Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let collection_store = env.open_single("single_page_item", StoreOptions::create())?;
        let reader = env.read()?;
        match collection_store.get(&reader, page_name.as_bytes())? {
            Some(Value::Str(s)) => {
                let item: SinglePageItem = serde_json::from_str(&s)?;
                Ok(Some(item))
            },
            _ => Ok(None),
        }
    }
    async fn update_single_page_item(&self, page_name: &SinglePageName, item_data: &SinglePageItem) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let collection_store = env.open_single("single_page_item", StoreOptions::create())?;
        let mut writer = env.write()?;
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, page_name.as_bytes(), &Value::Str(&item_str))?;
        writer.commit()?;
        Ok(())
    }

    async fn get_single_page_item_draft(&self, page_name: &SinglePageName) -> Result<Option<SinglePageItem>, Box<dyn Error + Send + Sync + 'static>> {
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

    async fn set_single_page_item_draft(&self, page_name: &SinglePageName, item_data: &SinglePageItem) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let key = page_draft_key(page_name.as_str());
        let mut writer = env.write()?;
        store.put(&mut writer, key.as_bytes(), &Value::Str(&serde_json::to_string(item_data)?))?;
        writer.commit()?;
        Ok(())
    }

    async fn delete_single_page_item_draft(&self, page_name: &SinglePageName) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(DRAFT_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let key = page_draft_key(page_name.as_str());
        if store.get(&reader, key.as_bytes())?.is_some() {
            let mut writer = env.write()?;
            store.delete(&mut writer, key.as_bytes())?;
            writer.commit()?;
        }
        Ok(())
    }
}
