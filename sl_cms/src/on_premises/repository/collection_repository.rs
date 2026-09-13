use crate::models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema};
use crate::on_premises::repository::Repository;
use crate::repositories::collection_repository::CollectionRepository;
use rkv::{StoreOptions, Value};
use std::error::Error;

impl CollectionRepository for Repository {
    fn get_collection_schema(&self, collection_name: &CollectionName) -> Result<Option<CollectionSchema>, Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("collection_schema", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, collection_name.as_bytes())? {
            Some(Value::Str(s)) => {
                let schema: CollectionSchema = serde_json::from_str(&s)?;
                Ok(Some(schema))
            },
            _ => Ok(None),
        }
    }
    fn list_collection_names(&self) -> Result<Vec<CollectionName>, Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("collection_schema", StoreOptions::create())?;
        let reader = env.read()?;
        let mut collections = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(_s))) = result {
                collections.push(str::from_utf8(&key)?.into());
            }
        }
        Ok(collections)
    }
    fn add_collection_schema(&self, collection_name: &CollectionName, schema: &CollectionSchema) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e|e.to_string())?;
        let store = env.open_single("collection_schema", StoreOptions::create())?;
        let schema_str = serde_json::to_string(schema)?;
        let mut writer = env.write()?;
        store.put(&mut writer, collection_name.as_bytes(), &Value::Str(&schema_str))?;
        writer.commit()?;
        Ok(())
    }
    fn delete_collection(&self, collection_name: &CollectionName) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("collection_schema", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, collection_name.as_bytes())?;
        let item_store = env.open_single(format!("collection_{}", collection_name).as_str(), StoreOptions::create())?;
        let reader = env.read()?;
        for result in item_store.iter_start(&reader)? {
            if let Ok((key, Value::Str(_s))) = result {
                item_store.delete(&mut writer, &key)?;
            }
        }
        // The per-collection item counter only exists once an item has been added, and
        // rkv reports deleting an absent key as an error ("key/value pair not found"),
        // which would turn DELETE on an empty collection into a 500.
        if self.counter_store.get(&reader, collection_name.as_bytes())?.is_some() {
            self.counter_store.delete(&mut writer, collection_name.as_bytes())?;
        }
        writer.commit()?;
        Ok(())
    }
    fn list_collection_items(&self, collection_name: &CollectionName) -> Result<Vec<(CollectionItemId, CollectionItem)>, Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store = env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
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
    fn get_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<Option<CollectionItem>,Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store = env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let reader = env.read()?;
        let key_bytes = item_id.to_le_bytes();
        match collection_store.get(&reader, &key_bytes)? {
            Some(Value::Str(s)) => {
                let item: CollectionItem = serde_json::from_str(&s)?;
                Ok(Some(item))
            },
            _ => Ok(None),
        }
    }
    
    fn add_collection_item(&self, collection_name: &CollectionName, item_data: &CollectionItem) -> Result<u64,Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store = env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        // Acquire the write transaction BEFORE the read snapshot used to read the
        // counter. Two concurrent writers would otherwise both read the same counter
        // value and then write the same item id, silently overwriting each other.
        let mut writer = env.write()?;
        let reader = env.read()?;

        let current_id = match self.counter_store.get(&reader, collection_name.as_bytes())? {
            Some(Value::U64(id)) => id,
            _ => 0,
        };
        let new_id = current_id + 1;
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, &new_id.to_le_bytes(), &Value::Str(&item_str))?;
        self.counter_store.put(&mut writer, collection_name.as_bytes(), &Value::U64(new_id))?;
        writer.commit()?;
        Ok(new_id)
    }
    
    fn update_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId, item_data: &CollectionItem) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store = env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let mut writer = env.write()?;
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, &item_id.to_le_bytes(), &Value::Str(&item_str))?;
        writer.commit()?;
        Ok(())
    }
    
    fn delete_collection_item(&self, collection_name: &CollectionName, item_id: &CollectionItemId) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store_name = format!("collection_{}", collection_name);
        let collection_store = env.open_single(Some(store_name.as_str()), StoreOptions::create())?;
        let mut writer = env.write()?;
        collection_store.delete(&mut writer, &item_id.to_le_bytes())?;
        writer.commit()?;
        Ok(())
    }
}