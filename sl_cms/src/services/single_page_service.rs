use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;

use crate::models::error::{HttpError, map_internal_error};
use crate::models::item_status::{ItemMetadata, ItemStatus};
use crate::models::schema::{validate_composite_references, validate_schema, CompositeFieldId};
use crate::models::single_page::{SinglePageItem, SinglePageItemResponse, SinglePageName, SinglePageSchema};
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::ImageRepository;
use crate::repositories::single_page_repository::SinglePageRepository;
use crate::webhook::{ContentEvent, Notifier};

pub struct SinglePageService<SR: SinglePageRepository, CFR: CompositeFieldRepository, IR: ImageRepository> {
    single_page_repository: Arc<SR>,
    composite_field_repository: Arc<CFR>,
    image_repository: Arc<IR>,
    /// Told about every publish/unpublish so a site build can be triggered.
    notifier: Arc<dyn Notifier>,
}


impl<SR: SinglePageRepository, CFR: CompositeFieldRepository, IR: ImageRepository> SinglePageService<SR, CFR, IR> {
    pub fn new(single_page_repository: Arc<SR>, composite_field_repository: Arc<CFR>, image_repository: Arc<IR>, notifier: Arc<dyn Notifier>) -> Self {
        SinglePageService {
            single_page_repository,
            composite_field_repository,
            image_repository,
            notifier,
        }
    }
    pub fn get_single_page_schema(
        &self,
        name: &SinglePageName,
    ) -> Result<SinglePageSchema, HttpError> {
        let single_page = self.single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?;
        match single_page {
            Some(schema) => Ok(schema),
            None => Err(HttpError::NotFound("Single page not found")),
        }
    }
    pub fn update_single_page_schema(
        &self,
        name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema)?;
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        self.single_page_repository
            .add_single_page_schema(name, schema)
            .map_err(map_internal_error)
    }
    pub fn get_all_page_names(&self) -> Result<Vec<SinglePageName>, HttpError> {
        self.single_page_repository
            .list_all_page_names()
            .map_err(map_internal_error)
    }

    /// Every composite a schema references must exist, otherwise the schema can be stored
    /// but never used to read or write values.
    fn ensure_composites_exist(&self, schema: &SinglePageSchema) -> Result<(), HttpError> {
        let available: HashSet<CompositeFieldId> = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?
            .into_keys()
            .collect();
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))
    }
    pub fn add_single_page_schema(
        &self,
        name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema)?;
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?
            .is_some()
        {
            return Err(HttpError::Conflict(&format!(
                "Single page with id '{}' already exists",
                name
            )));
        }
        self.single_page_repository
            .add_single_page_schema(name, schema)
            .map_err(map_internal_error)
    }
    pub fn delete_single_page(&self, name: &SinglePageName) -> Result<(), HttpError> {
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        self.single_page_repository
            .delete_single_page(name)
            .map_err(map_internal_error)
    }
    pub fn get_single_page_item(
        &self,
        name: &SinglePageName,
    ) -> Result<SinglePageItemResponse, HttpError> {
        let schema = self
            .single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?;
        match schema {
            None => {
                Err(HttpError::NotFound(&format!(
                    "Single page with id '{}' does not exist",
                    name
                )))
            }
            Some(schema) => {
                let item = self.single_page_repository
                    .get_single_page_item(name)
                    .map_err(map_internal_error)?;

                let composite_schema_map = self
                    .composite_field_repository
                    .list_composite_field_schemas()
                    .map_err(map_internal_error)?;
                let formatted_item = item.unwrap_or_default().format_to_schema(&composite_schema_map,
                    &schema,
                );
                let response_item = formatted_item.to_response(self.image_repository.as_ref());
                Ok(response_item)
            },
        }
    }
    pub fn update_single_page_item(
        &self,
        name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), HttpError> {
        let schema = self
            .single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?;
        match schema {
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Single page with id '{}' does not exist",
                    name
                )));
            }
            Some(schema) => {
                item_data.validate_to_schema(
                    &self
                        .composite_field_repository
                        .list_composite_field_schemas()
                        .map_err(map_internal_error)?,
                    &schema,
                ).map_err(|e| HttpError::BadRequest(&e.to_string()))?;

                self.single_page_repository
                    .update_single_page_item(name, item_data)
                    .map_err(map_internal_error)?;
                self.stamp_page(name)?;
                Ok(())
            }
        }
    }

    /// Record when the page's values were last saved.
    ///
    /// Saving must not disturb the published state: a published page stays published, and
    /// only `updated_at` moves.
    fn stamp_page(&self, name: &SinglePageName) -> Result<(), HttpError> {
        let metadata = self
            .single_page_repository
            .get_page_metadata(name)
            .map_err(map_internal_error)?
            .unwrap_or_default()
            .touched(Utc::now());

        self.single_page_repository
            .set_page_metadata(name, &metadata)
            .map_err(map_internal_error)
    }

    /// Update a single page's item from an **untagged** JSON body.
    ///
    /// Values arrive without type tags; the page schema is what gives them meaning.
    /// Types that do not match are rejected with 400 rather than coerced.
    pub fn update_single_page_item_from_json(
        &self,
        name: &SinglePageName,
        body: &serde_json::Value,
    ) -> Result<(), HttpError> {
        let item = self.parse_item(name, body)?;
        self.update_single_page_item(name, &item)
    }

    fn parse_item(
        &self,
        name: &SinglePageName,
        body: &serde_json::Value,
    ) -> Result<SinglePageItem, HttpError> {
        let schema = self
            .single_page_repository
            .get_single_page_schema(name)
            .map_err(map_internal_error)?
            .ok_or_else(|| {
                HttpError::NotFound(&format!(
                    "Single page with id '{}' does not exist",
                    name
                ))
            })?;
        let composite_schemas = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?;
        SinglePageItem::from_untyped(body, &composite_schemas, &schema)
            .map_err(|e| HttpError::BadRequest(&e))
    }

    // ---- draft / published ---------------------------------------------------

    /// Draft/published state of one page. Absent metadata means "draft".
    pub fn get_page_metadata(&self, name: &SinglePageName) -> Result<ItemMetadata, HttpError> {
        // Reuses the schema lookup so a missing page is a 404 rather than a default.
        self.get_single_page_schema(name)?;
        Ok(self
            .single_page_repository
            .get_page_metadata(name)
            .map_err(map_internal_error)?
            .unwrap_or_default())
    }

    pub fn set_page_status(
        &self,
        name: &SinglePageName,
        status: ItemStatus,
    ) -> Result<ItemMetadata, HttpError> {
        self.get_single_page_schema(name)?;
        // Built from the stored record so publishing keeps the content timestamps.
        let metadata = self
            .single_page_repository
            .get_page_metadata(name)
            .map_err(map_internal_error)?
            .unwrap_or_default()
            .with_status(status);
        self.single_page_repository
            .set_page_metadata(name, &metadata)
            .map_err(map_internal_error)?;
        // Only after the status is stored (see `CollectionService::set_item_status`).
        self.notifier
            .notify(ContentEvent::single_page(name, &metadata));
        Ok(metadata)
    }

    /// A page's content, but only if it is published, with the metadata the delivery API
    /// reports.
    pub fn get_published_page_item(
        &self,
        name: &SinglePageName,
    ) -> Result<(ItemMetadata, SinglePageItemResponse), HttpError> {
        let metadata = self.get_page_metadata(name)?;
        if !metadata.is_published() {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        Ok((metadata, self.get_single_page_item(name)?))
    }

    /// Pages visible to the public delivery API.
    pub fn list_published_page_names(&self) -> Result<Vec<SinglePageName>, HttpError> {
        let mut names = Vec::new();
        for name in self.get_all_page_names()? {
            let metadata = self
                .single_page_repository
                .get_page_metadata(&name)
                .map_err(map_internal_error)?;
            if metadata.map(|m| m.is_published()).unwrap_or(false) {
                names.push(name);
            }
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::marker::PhantomData;
    use std::sync::{Arc, RwLock};

    use crate::models::field::{CompositeFieldSchema, TextFieldOptions};
    use crate::models::field::{FieldSchema, FieldType, FieldValue, FieldValueMap, FieldValueResponse};
    use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};
    use crate::models::schema::CompositeFieldId;

    use super::*;
    use crate::webhook::NoopNotifier;

    struct MockSinglePageRepository {
        schemas: Arc<RwLock<HashMap<SinglePageName, SinglePageSchema>>>,
        items: Arc<RwLock<HashMap<SinglePageName, SinglePageItem>>>,
        page_metadata: Arc<RwLock<HashMap<SinglePageName, ItemMetadata>>>,
    }
    impl SinglePageRepository for MockSinglePageRepository {
        fn get_single_page_schema(
            &self,
            name: &SinglePageName,
        ) -> Result<Option<SinglePageSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(name).cloned())
        }
        fn list_all_page_names(&self) -> Result<Vec<SinglePageName>,Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.schemas.read().unwrap().keys().cloned().collect())
        }
        fn add_single_page_schema(
            &self,
            _single_page_name: &SinglePageName,
            _schema: &SinglePageSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(_single_page_name.clone(), _schema.clone());
            Ok(())
        }
        fn delete_single_page(
            &self,
            _single_page_name: &SinglePageName,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(_single_page_name);
            self.page_metadata.write().unwrap().remove(_single_page_name);
            Ok(())
        }
        fn get_single_page_item(
            &self,
            single_page_name: &SinglePageName
        ) -> Result<Option<SinglePageItem>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            if let Some(item) = self.items.read().unwrap().get(single_page_name) {
                Ok(Some(item.clone()))
            } else {
                Ok(None)
            }
        }
        fn update_single_page_item(
            &self,
            single_page_name: &SinglePageName,
            item_data: &SinglePageItem,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.items.write().unwrap().insert(single_page_name.clone(), item_data.clone());
            Ok(())
        }
        fn get_page_metadata(
            &self,
            page_name: &SinglePageName,
        ) -> Result<Option<ItemMetadata>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.page_metadata.read().unwrap().get(page_name).cloned())
        }
        fn set_page_metadata(
            &self,
            page_name: &SinglePageName,
            metadata: &ItemMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.page_metadata.write().unwrap().insert(page_name.clone(), metadata.clone());
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
        fn take_upload_key(&self, _key: &str) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(None)
        }
        fn read_image_bytes(&self, _file_name: &str) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(None)
        }
        fn write_image_bytes(&self, _file_name: &str, _data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
    }

    // Test helper functions
    fn create_test_service() -> SinglePageService<MockSinglePageRepository, MockCompositeFieldRepository, MockImageRepository> {
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier))
    }

    /// Records the events a service emits (see the collection service tests).
    #[derive(Default)]
    struct RecordingNotifier {
        events: std::sync::Mutex<Vec<ContentEvent>>,
    }

    impl Notifier for RecordingNotifier {
        fn notify(&self, event: ContentEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn a_status_change_notifies_once_and_an_unknown_page_notifies_nothing() {
        let notifier = Arc::new(RecordingNotifier::default());
        let mut schemas = HashMap::new();
        schemas.insert("home".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            notifier.clone(),
        );

        service
            .set_page_status(&"home".into(), ItemStatus::Published)
            .unwrap();
        {
            let events = notifier.events.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), "single_page.published");
            assert_eq!(events[0].page.as_ref().unwrap().as_str(), "home");
        }

        // An unknown page is a 404 and must not look like a change.
        assert!(service
            .set_page_status(&"missing".into(), ItemStatus::Published)
            .is_err());
        assert_eq!(notifier.events.lock().unwrap().len(), 1);
    }

    fn create_test_schema() -> SinglePageSchema {
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

    fn create_test_item(title: &str, count: f64) -> SinglePageItem {
        FieldValueMap(HashMap::from([
            ("title".to_string(), FieldValue::Text(title.to_string())),
            ("count".to_string(), FieldValue::Number(Some(count))),
        ]), PhantomData)
    }

    /// `get_single_page_item` returns the API response form, not the stored item form.
    fn create_test_item_response(title: &str, count: f64) -> SinglePageItemResponse {
        HashMap::from([
            ("title".to_string(), FieldValueResponse::Text(title.to_string())),
            ("count".to_string(), FieldValueResponse::Number(Some(count))),
        ])
    }

    #[test]
    fn test_create_single_page_service() {
        let service = create_test_service();
        assert!(service.get_all_page_names().is_ok());
    }
    #[test]
    fn test_get_all_pages_empty() {
        let service = create_test_service();
        let page_names = service.get_all_page_names().unwrap();
        assert_eq!(page_names.len(), 0);
    }

    #[test]
    fn test_get_single_page_schema_not_found() {
        let service = create_test_service();
        let result = service.get_single_page_schema(&"non_existent".into());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page not found"));
    }

    #[test]
    fn test_add_single_page_schema_success() {
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

        let result = service.add_single_page_schema(&"test_page".into(), &schema);
        assert!(result.is_ok());

        let page_names = service.get_all_page_names().unwrap();
        assert_eq!(page_names, vec!["test_page".into()]);
        let retrieved_schema = service.get_single_page_schema(&"test_page".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_update_single_page_schema_success() {
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
        service.add_single_page_schema(&"test_page".into(), &initial_schema).unwrap();

        let updated_schema = vec![
            FieldSchema {
                name: "title2".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.update_single_page_schema(&"test_page".into(), &updated_schema);
        assert!(result.is_ok());

        let retrieved_schema = service.get_single_page_schema(&"test_page".into()).unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[test]
    fn test_add_single_page_schema_already_exists() {
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
        service.add_single_page_schema(&"test_page".into(), &schema).unwrap();

        let duplicate_schema = vec![
            FieldSchema {
                name: "other".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.add_single_page_schema(&"test_page".into(), &duplicate_schema);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::Conflict("Single page with id 'test_page' already exists"));

        let retrieved_schema = service.get_single_page_schema(&"test_page".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_delete_single_page_success() {
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
        service.add_single_page_schema(&"test_page".into(), &schema).unwrap();

        let result = service.delete_single_page(&"test_page".into());
        assert!(result.is_ok());

        let page_names = service.get_all_page_names().unwrap();
        assert_eq!(page_names.len(), 0);
    }
    #[test]
    fn test_delete_non_exists_single_page() {
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
        service.add_single_page_schema(&"test_page".into(), &schema).unwrap();

        let result = service.delete_single_page(&"non_existent".into());
        assert!(result.is_err());

        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
        let page_names = service.get_all_page_names().unwrap();
        assert_eq!(page_names, vec!["test_page".into()]);
    }

    #[test]
    fn test_update_missing_schema() {
        let service = create_test_service();
        let update_result = service.update_single_page_schema(
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
    fn test_create_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_composite".into(),
            &create_test_item("Test Title", 42.0),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.update_single_page_item(&"non_existent".into(), &FieldValueMap(HashMap::new(), PhantomData));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_create_single_page_item_invalid_data() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_composite".into(),
            &FieldValueMap(HashMap::from([("count".to_string(), FieldValue::Number(Some(42.0)))]), PhantomData),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Field 'title' is missing"));
    }

    #[test]
    fn test_get_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_schema".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_schema".into(), create_test_item("Sample Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.get_single_page_item(&"test_schema".into());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), create_test_item_response("Sample Title", 10.0));
    }

    #[test]
    fn test_get_single_page_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_schema".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository {}), Arc::new(NoopNotifier));
        let result = service.get_single_page_item(&"test_schema".into());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HashMap::from([
            ("title".to_string(), FieldValueResponse::Text("".to_string())),
            ("count".to_string(), FieldValueResponse::Number(None)),
        ]));
    }

    #[test]
    fn test_get_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.get_single_page_item(&"non_existent".into());
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_update_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Original Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository {}), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_page".into(),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_ok());

        let item = service.get_single_page_item(&"test_page".into()).unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[test]
    fn test_update_single_page_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository {}), Arc::new(NoopNotifier));
        let result = service.update_single_page_item(
            &"test_page".into(),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_ok());

        let item = service.get_single_page_item(&"test_page".into()).unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[test]
    fn test_update_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.update_single_page_item(
            &"non_existent".into(),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_update_single_page_item_invalid_data() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Original Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository {}), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_page".into(),
            &FieldValueMap(HashMap::from([
            ("count".to_string(), FieldValue::Number(Some(1.0))),
        ]), PhantomData),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Field 'title' is missing"));
    }

    #[test]
    fn test_get_single_page_items_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Sample Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository {}), Arc::new(NoopNotifier));

        let result = service.get_single_page_item(&"test_page".into());
        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items, create_test_item_response("Sample Title", 10.0));
    }
}
