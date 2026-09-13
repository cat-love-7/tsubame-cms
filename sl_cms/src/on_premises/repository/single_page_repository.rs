use crate::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};
use crate::on_premises::repository::Repository;
use crate::repositories::single_page_repository::SinglePageRepository;
use rkv::{StoreOptions, Value};
use std::error::Error;


impl SinglePageRepository for Repository {
    fn get_single_page_schema(&self, page_name: &SinglePageName) -> Result<Option<SinglePageSchema>, Box<dyn Error + Send + Sync + 'static>> {
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
    fn list_all_page_names(&self) -> Result<Vec<SinglePageName>, Box<dyn Error + Send + Sync + 'static>> {
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
    fn add_single_page_schema(&self, page_name: &SinglePageName, schema: &SinglePageSchema) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e|e.to_string())?;
        let store = env.open_single("single_page_schema", StoreOptions::create())?;
        let schema_str = serde_json::to_string(schema)?;
        let mut writer = env.write()?;
        store.put(&mut writer, page_name.as_bytes(), &Value::Str(&schema_str))?;
        writer.commit()?;
        Ok(())
    }
    fn delete_single_page(&self, page_name: &SinglePageName) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("single_page_schema", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, page_name.as_bytes())?;
        let item_store = env.open_single("single_page_item", StoreOptions::create())?;
        item_store.delete(&mut writer, &page_name.as_bytes())?;
        self.counter_store.delete(&mut writer, page_name.as_bytes())?;
        writer.commit()?;
        Ok(())
    }
    fn get_single_page_item(&self, page_name: &SinglePageName) -> Result<Option<SinglePageItem>,Box<dyn Error + Send + Sync + 'static>> {
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
    fn update_single_page_item(&self, page_name: &SinglePageName, item_data: &SinglePageItem) -> Result<(),Box<dyn Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let collection_store = env.open_single("single_page_item", StoreOptions::create())?;
        let mut writer = env.write()?;
        let item_str = serde_json::to_string(item_data)?;
        collection_store.put(&mut writer, page_name.as_bytes(), &Value::Str(&item_str))?;
        writer.commit()?;
        Ok(())
    }
}