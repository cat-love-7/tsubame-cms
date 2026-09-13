use std::sync::Arc;

use crate::models::collection::{CollectionItem, CollectionItemId, CollectionItemResponse, CollectionName, CollectionSchema};
use crate::models::error::{HttpError, map_internal_error};
use crate::repositories::collection_repository::CollectionRepository;
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::ImageRepository;

pub struct CollectionService<CR: CollectionRepository, CFR: CompositeFieldRepository, IR: ImageRepository> {
    collection_repository: Arc<CR>,
    composite_field_repository: Arc<CFR>,
    image_repository: Arc<IR>,
}


impl<CR: CollectionRepository, CFR: CompositeFieldRepository, IR: ImageRepository> CollectionService<CR, CFR, IR> {
    pub fn new(collection_repository: Arc<CR>, composite_field_repository: Arc<CFR>, image_repository: Arc<IR>) -> Self {
        CollectionService {
            collection_repository,
            composite_field_repository,
            image_repository,
        }
    }
    pub fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<CollectionSchema, HttpError> {
        let collection = self.collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match collection {
            Some(schema) => Ok(schema),
            None => Err(HttpError::NotFound("Collection not found")),
        }
    }
    pub fn update_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        if self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::BadRequest(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        self.collection_repository
            .add_collection_schema(collection_name, schema)
            .map_err(map_internal_error)
    }
    pub fn get_all_collections(&self) -> Result<Vec<CollectionName>, HttpError> {
        self.collection_repository
            .list_collection_names()
            .map_err(map_internal_error)
    }
    pub fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        if self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?
            .is_some()
        {
            return Err(HttpError::BadRequest(&format!(
                "Collection with id '{}' already exists",
                collection_name
            )));
        }
        self.collection_repository
            .add_collection_schema(collection_name, schema)
            .map_err(map_internal_error)
    }
    pub fn delete_collection(&self, collection_name: &CollectionName) -> Result<(), HttpError> {
        if self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::BadRequest(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        self.collection_repository
            .delete_collection(collection_name)
            .map_err(map_internal_error)
    }
    pub fn get_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match schema {
            None => {
                Err(HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                )))
            }
            Some(schema) => {
                let items = self.collection_repository
                    .list_collection_items(collection_name)
                    .map_err(map_internal_error)?;

                let composite_schema_map = self
                    .composite_field_repository
                    .list_composite_field_schemas()
                    .map_err(map_internal_error)?;
                let mut ret = Vec::new();
                for (id, item) in items {
                    let formatted_item = item.format_to_schema(&composite_schema_map, &schema);
                    let response_item = formatted_item.to_response(self.image_repository.as_ref());
                    ret.push((id, response_item));
                }
                Ok(ret)
            },
        }
    }
    pub fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<CollectionItemResponse, HttpError> {
        let schema = self
                    .collection_repository
                    .get_collection_schema(collection_name)
                    .map_err(map_internal_error)?;
        if schema.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        let schema = schema.unwrap();
        let item = self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .map_err(map_internal_error)?;
        let composite_schema_map = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?;
        match item {
            Some(i) => {
                let formatted_item = i.format_to_schema(&composite_schema_map, &schema);
                let response_item = formatted_item.to_response(self.image_repository.as_ref());
                Ok(response_item)
            }
            None => Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id.to_string(), collection_name
            ))),
        }
    }
    pub fn create_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &CollectionItem,
    ) -> Result<u64, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match schema {
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                )));
            }
            Some(schema) => {
                let composite_schema_map = self
                    .composite_field_repository
                    .list_composite_field_schemas()
                    .map_err(map_internal_error)?;
                item_data.validate_to_schema(&composite_schema_map, &schema)
                    .map_err(|e| HttpError::BadRequest(&e.to_string()))?;
            }
        }
        let item_id = self
            .collection_repository
            .add_collection_item(collection_name, item_data)
            .map_err(map_internal_error)?;
        Ok(item_id)
    }
    pub fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        item_data: &CollectionItem,
    ) -> Result<(), HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match schema {
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                )));
            }
            Some(schema) => {
                if self
                    .collection_repository
                    .get_collection_item(collection_name, &item_id)
                    .map_err(|e| HttpError::InternalServerError(&e.to_string()))?
                    .is_none()
                {
                    return Err(HttpError::NotFound(&format!(
                        "Item with id '{}' not found in collection '{}'",
                        item_id.to_string(), collection_name
                    )));
                }
                item_data.validate_to_schema(
                    &self
                        .composite_field_repository
                        .list_composite_field_schemas()
                        .map_err(|e| HttpError::InternalServerError(&e.to_string()))?,
                    &schema,
                ).map_err(|e| HttpError::BadRequest(&e.to_string()))?;

                self.collection_repository
                    .update_collection_item(collection_name, &item_id, item_data)
                    .map_err(|e| HttpError::InternalServerError(&e.to_string()))?;
                Ok(())
            }
        }
    }

    pub fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<(), HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        if schema.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        if self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id.to_string(), collection_name
            )));
        }

        self.collection_repository
            .delete_collection_item(collection_name, &item_id)
            .map_err(map_internal_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use crate::models::collection::CollectionName;
    use crate::models::field::{CompositeFieldSchema, FieldValueMap, TextFieldOptions};
    use crate::models::field::{FieldSchema, FieldType, FieldValue};
    use crate::models::schema::CompositeFieldId;
    use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};
    use crate::repositories::image_repository::ImageRepository;

    use super::*;

    struct MockCollectionRepository {
        schemas: Arc<RwLock<HashMap<CollectionName, CollectionSchema>>>,
        items: Arc<RwLock<HashMap<CollectionName, HashMap<CollectionItemId, CollectionItem>>>>,
        item_counter: Arc<RwLock<u64>>,
    }
    impl CollectionRepository for MockCollectionRepository {
        fn get_collection_schema(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Option<CollectionSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(collection_name).cloned())
        }
        fn list_collection_names(
            &self,
        ) -> Result<Vec<CollectionName>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.schemas.read().unwrap().keys().cloned().collect())
        }
        fn add_collection_schema(
            &self,
            collection_name: &CollectionName,
            schema: &CollectionSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(collection_name.clone(), schema.clone());
            Ok(())
        }
        fn delete_collection(
            &self,
            collection_name: &CollectionName,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(collection_name);
            Ok(())
        }
        fn list_collection_items(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Vec<(CollectionItemId, CollectionItem)>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.items.read().unwrap().get(collection_name).map_or(vec![], |items_map| {
                items_map.iter().map(|(id, item)| (id.clone(), item.clone())).collect()
            }))
        }
        fn get_collection_item(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<Option<CollectionItem>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            if let Some(items_map) = self.items.read().unwrap().get(collection_name) {
                Ok(items_map.get(item_id).cloned())
            } else {
                Ok(None)
            }
        }
        fn add_collection_item(
            &self,
            collection_name: &CollectionName,
            item_data: &CollectionItem,
        ) -> Result<u64, Box<dyn std::error::Error + Send + Sync + 'static>> {
            let mut items_map = self.items.write().unwrap();
            let collection_items = items_map.entry(collection_name.clone()).or_insert_with(HashMap::new);
            let new_id = {
                let mut counter = self.item_counter.write().unwrap();
                *counter += 1;
                *counter
            };
            collection_items.insert(CollectionItemId::from_u64(new_id), item_data.clone());
            Ok(new_id)
        }
        fn update_collection_item(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            item_data: &CollectionItem,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            if let Some(items_map) = self.items.write().unwrap().get_mut(collection_name) {
                items_map.insert(item_id.clone(), item_data.clone());
            }
            Ok(())
        }
        fn delete_collection_item(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            if let Some(items_map) = self.items.write().unwrap().get_mut(collection_name) {
                items_map.remove(item_id);
            }
            Ok(())
        }
    }

    struct MockCompositeFieldRepository {
        schemas: Arc<RwLock<HashMap<CompositeFieldId, CompositeFieldSchema>>>,
    }
    impl CompositeFieldRepository for MockCompositeFieldRepository {
        fn list_composite_field_schemas(
            &self,
        ) -> Result<
            HashMap<CompositeFieldId, CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().clone())
        }
        fn get_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<
            Option<CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().get(id).cloned())
        }
        fn add_composite_field_schema(
            &self,
            id: &CompositeFieldId,
            schema: &CompositeFieldSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(id.clone(), schema.clone());
            Ok(())
        }
        fn delete_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(id);
            Ok(())
        }
    }

    struct MockImageRepository {}
    impl ImageRepository for MockImageRepository {
        fn get_image(&self, id: &ImageID) -> Result<Option<Image>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(Some(Image {
                original_filename: format!("image_{}.jpg", id),
                url: format!("/images/{}", id),
                uploaded_at: chrono::Utc::now(),
            }))
        }
        fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(vec![])
        }
        fn generate_image_upload_url(&self, _upload_info: &NewImageRequest) -> Result<NewImageInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(NewImageInfo {
                id: ImageID::from_u64(1),
                upload_url: "/upload/1".to_string(),
            })
        }
        fn delete_image(&self, _id: &ImageID) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
    }

    // Test helper functions
    fn create_test_service() -> CollectionService<MockCollectionRepository, MockCompositeFieldRepository, MockImageRepository> {
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository))
    }

    fn create_test_schema() -> CollectionSchema {
        vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            },
            FieldSchema {
                name: "count".to_string(),
                field_type: FieldType::Number,
                required: false,
                width: 12,
                height: 1,
            },
        ]
    }

    fn create_test_item(title: &str, count: f64) -> CollectionItem {
        FieldValueMap(HashMap::from([
            ("title".to_string(), FieldValue::Text(title.to_string())),
            ("count".to_string(), FieldValue::Number(Some(count))),
        ]), std::marker::PhantomData)
    }

    fn create_test_item_response(title: &str, count: f64) -> CollectionItemResponse {
        use crate::models::field::FieldValueResponse;
        HashMap::from([
            ("title".to_string(), FieldValueResponse::Text(title.to_string())),
            ("count".to_string(), FieldValueResponse::Number(Some(count))),
        ])
    }

    #[test]
    fn test_create_collection_service() {
        let service = create_test_service();
        assert!(service.get_all_collections().is_ok());
    }
    #[test]
    fn test_get_all_collections_empty() {
        let service = create_test_service();
        let collection_names = service.get_all_collections().unwrap();
        assert_eq!(collection_names.len(), 0);
    }

    #[test]
    fn test_get_collection_schema_not_found() {
        let service = create_test_service();
        let result = service.get_collection_schema(&"non_existent".into());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection not found"));
    }

    #[test]
    fn test_add_collection_schema_success() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];

        let result = service.add_collection_schema(&"test_collection".into(), &schema);
        assert!(result.is_ok());

        let collection_names = service.get_all_collections().unwrap();
        assert_eq!(collection_names, vec!["test_collection".into()]);

        let retrieved_schema = service.get_collection_schema(&"test_collection".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_update_collection_schema_success() {
        let service = create_test_service();
        let initial_schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        service.add_collection_schema(&"test_collection".into(), &initial_schema).unwrap();

        let updated_schema = vec![
            FieldSchema {
                name: "title2".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.update_collection_schema(&"test_collection".into(), &updated_schema);
        assert!(result.is_ok());

        let retrieved_schema = service.get_collection_schema(&"test_collection".into()).unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[test]
    fn test_add_collection_schema_already_exists() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        service.add_collection_schema(&"test_collection".into(), &schema).unwrap();

        let duplicate_schema = vec![
            FieldSchema {
                name: "other".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.add_collection_schema(&"test_collection".into(), &duplicate_schema);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Collection with id 'test_collection' already exists"));

        let retrieved_schema = service.get_collection_schema(&"test_collection".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_delete_collection_success() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        service.add_collection_schema(&"test_collection".into(), &schema).unwrap();

        let result = service.delete_collection(&"test_collection".into());
        assert!(result.is_ok());

        let collection_names = service.get_all_collections().unwrap();
        assert_eq!(collection_names.len(), 0);
    }

    #[test]
    fn test_update_missing_schema() {
        let service = create_test_service();
        let update_result = service.update_collection_schema(
            &"non_existent".into(),
            &vec![
                FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                }
            ],
        );
        assert!(update_result.is_err());
    }
    #[test]
    fn test_create_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.create_collection_item(
            &"test_composite".into(),
            &create_test_item("Test Title", 42.0),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn test_create_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.create_collection_item(&"non_existent".into(), &FieldValueMap(HashMap::new(), std::marker::PhantomData));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_create_collection_item_invalid_data() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.create_collection_item(
            &"test_composite".into(),
            &FieldValueMap(HashMap::from([("count".to_string(), FieldValue::Number(Some(42.0)))]), std::marker::PhantomData),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Field 'title' is missing"));
    }

    #[test]
    fn test_get_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Sample Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), create_test_item_response("Sample Title", 10.0));
    }

    #[test]
    fn test_get_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(999));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Item with id '999' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_get_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.get_collection_item(&"non_existent".into(), CollectionItemId::from_u64(1));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_update_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Original Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.update_collection_item(
            &"test_composite".into(),
            CollectionItemId::from_u64(1),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_ok());

        let item = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1)).unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[test]
    fn test_update_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.update_collection_item(
            &"test_composite".into(),
            CollectionItemId::from_u64(999),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Item with id '999' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_update_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.update_collection_item(
            &"non_existent".into(),
            CollectionItemId::from_u64(1),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_update_collection_item_invalid_data() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Original Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.update_collection_item(
            &"test_composite".into(),
            CollectionItemId::from_u64(1),
            &FieldValueMap(HashMap::from([("count".to_string(), FieldValue::Number(Some(100.0)))]), std::marker::PhantomData),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Field 'title' is missing"));
    }

    #[test]
    fn test_get_collection_items_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Sample Title", 10.0));
        collection_item.insert(CollectionItemId::from_u64(2), create_test_item("Another Title", 20.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(2)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.get_collection_items(&"test_composite".into());
        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.contains(&(CollectionItemId::from_u64(1), create_test_item_response("Sample Title", 10.0))));
        assert!(items.contains(&(CollectionItemId::from_u64(2), create_test_item_response("Another Title", 20.0))));
    }

    #[test]
    fn test_get_collection_items_missing_collection() {
        let service = create_test_service();
        let result = service.get_collection_items(&"non_existent".into());
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_delete_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Sample Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.delete_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1));
        assert!(result.is_ok());

        let get_result = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1));
        assert!(get_result.is_err());
        assert_eq!(get_result.err().unwrap(), HttpError::NotFound("Item with id '1' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_delete_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository));

        let result = service.delete_collection_item(&"test_composite".into(), CollectionItemId::from_u64(999));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Item with id '999' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_delete_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.delete_collection_item(&"non_existent".into(), CollectionItemId::from_u64(1));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }
}
