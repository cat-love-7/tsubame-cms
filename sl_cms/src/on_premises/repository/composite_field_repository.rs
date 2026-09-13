use crate::models::field::CompositeFieldSchema;
use crate::models::schema::CompositeFieldId;
use crate::on_premises::repository::Repository;
use crate::repositories::composite_field_repository::{CompositeFieldRepository};
use rkv::{StoreOptions, Value};
use std::collections::HashMap;
use std::error::Error;

impl CompositeFieldRepository for Repository {
    fn list_composite_field_schemas(&self) -> Result<HashMap<CompositeFieldId,CompositeFieldSchema>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("composite_field_schema", StoreOptions::create())?;
        let reader = env.read()?;
        let mut schemas = HashMap::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(s))) = result {
                let id = str::from_utf8(&key)?.into();
                let schema: CompositeFieldSchema = serde_json::from_str(&s)?;
                schemas.insert(id, schema);
            }
        }
        Ok(schemas)
    }
    fn get_composite_field_schema(&self, id: &CompositeFieldId) -> Result<Option<CompositeFieldSchema>, Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("composite_field_schema", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, id.as_bytes())? {
            Some(Value::Str(s)) => {
                let schema: CompositeFieldSchema = serde_json::from_str(&s)?;
                Ok(Some(schema))
            },
            _ => Ok(None),
        }
    }
    fn add_composite_field_schema(&self,id: &CompositeFieldId, schema: &CompositeFieldSchema) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e|e.to_string())?;
        let store = env.open_single("composite_field_schema", StoreOptions::create())?;
        let schema_str = serde_json::to_string(schema)?;
        let mut writer = env.write()?;
        store.put(&mut writer, id.as_bytes(), &Value::Str(&schema_str))?;
        writer.commit()?;
        Ok(())
    }
    fn delete_composite_field_schema(&self, id: &CompositeFieldId) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("composite_field_schema", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, id.as_bytes())?;
        writer.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::field::{FieldSchema, FieldType, TextFieldOptions};

    use super::*;
    use std::cell::RefCell;
    use std::{fs, vec};
    use std::sync::atomic::AtomicU32;
    use std::sync::{Arc};
    use rkv::{Manager, Rkv};
    use rkv::backend::{SafeMode, SafeModeEnvironment};
    
    thread_local! {
        static THREAD_ID: RefCell<u32> = RefCell::new(0);
    }

    fn setup_repository() -> Repository {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let id = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        THREAD_ID.with(|p| {
            let mut id_cell = p.borrow_mut();
            *id_cell = id;
        });

        let strpath = format!("./data/on_premises/test_rkv_composite_{}",id);
        let path = std::path::Path::new(&strpath);

        fs::create_dir_all(path).unwrap();
        let mut manager = Manager::<SafeModeEnvironment>::singleton().write().unwrap();
        let created_arc = manager.get_or_create(path, Rkv::new::<SafeMode>).unwrap();
        Repository::new(Arc::clone(&created_arc), path.join("images"))
    }
    fn teardown_repository() {
        let id = THREAD_ID.with(|p| {
            p.borrow().clone()
        });
        let strpath = format!("./data/on_premises/test_rkv_composite_{}",id);
        let path = std::path::Path::new(&strpath);
        if path.exists() {
            fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn test_add_and_get_composite_field_schema() {
        let repository = setup_repository();
        let schema = vec![];
        let id = "test_schema".into();
        repository.add_composite_field_schema(&id, &schema).unwrap();
        let retrieved_schema = repository.get_composite_field_schema(&id).unwrap();
        assert!(retrieved_schema.is_some());
        assert_eq!(retrieved_schema.unwrap(), schema);
        teardown_repository();
    }
    #[test]
    fn test_list_composite_field_schemas() {
        let repository = setup_repository();
        let id1 = "schema1".into();
        let schema1 = vec![FieldSchema {
            name: "field1".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            }
        ];
        let id2 = "schema2".into();
        let schema2 = vec![FieldSchema {
            name: "field2".to_string(),
            field_type: FieldType::Number,
            required: false,
            width: 12,
            height: 1,
        }];
        repository.add_composite_field_schema(&id1, &schema1).unwrap();
        repository.add_composite_field_schema(&id2, &schema2).unwrap();
        let schemas = repository.list_composite_field_schemas().unwrap();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.contains_key(&"schema1".into()));
        assert!(schemas.contains_key(&"schema2".into()));
        assert_eq!(schemas.get(&"schema1".into()).unwrap(), &schema1);
        assert_eq!(schemas.get(&"schema2".into()).unwrap(), &schema2);
        teardown_repository();
    }
    #[test]
    fn test_get_nonexistent_composite_field_schema() {
        let repository = setup_repository();
        let retrieved_schema = repository.get_composite_field_schema(&"nonexistent".into()).unwrap();
        assert!(retrieved_schema.is_none());
        teardown_repository();
    }
    #[test]
    fn test_get_anomaly() {
        let repository = setup_repository();
        let id = "test_schema".into();
        let schema = vec![];
        repository.add_composite_field_schema(&id, &schema).unwrap();
        // Manually corrupt the data
        {
            let env = repository.rkv.read().unwrap();
            let store = env.open_single("composite_field_schema", StoreOptions::create()).unwrap();
            let mut writer = env.write().unwrap();
            store.put(&mut writer, b"test_schema", &Value::Str("invalid_json")).unwrap();
            writer.commit().unwrap();
        }

        let result = repository.get_composite_field_schema(&"test_schema".into());
        assert!(result.is_err());

        let list_result = repository.list_composite_field_schemas();
        assert!(list_result.is_err());
        teardown_repository();
    }
    #[test]
    fn test_delete_composite_field_schema() {
        let repository = setup_repository();
        let id = "to_be_deleted".into();
        let schema = vec![];
        repository.add_composite_field_schema(&id, &schema).unwrap();
        let retrieved_schema = repository.get_composite_field_schema(&id).unwrap();
        assert!(retrieved_schema.is_some());
        repository.delete_composite_field_schema(&id).unwrap();
        let retrieved_schema_after_deletion = repository.get_composite_field_schema(&id).unwrap();
        assert!(retrieved_schema_after_deletion.is_none());
        teardown_repository();
    }
}
