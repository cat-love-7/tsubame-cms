use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::models::error::{HttpError, map_internal_error};
use crate::models::field::{CompositeFieldSchema, FieldSchema};
use crate::models::schema::{
    validate_composite_references, validate_no_composite_cycles, validate_schema, CompositeFieldId,
};
use crate::repositories::composite_field_repository::CompositeFieldRepository;

pub struct CompositeFieldService<CFR: CompositeFieldRepository> {
    composite_field_repository: Arc<CFR>,
}

impl<CFR: CompositeFieldRepository> CompositeFieldService<CFR> {
    pub fn new(composite_field_repository: Arc<CFR>) -> Self {
        CompositeFieldService {
            composite_field_repository,
        }
    }
    pub fn get_composite_field_schema(&self, field_name: &CompositeFieldId) -> Result<CompositeFieldSchema, HttpError> {
        self.composite_field_repository.get_composite_field_schema(field_name)
            .map_err(map_internal_error)
            .and_then(|opt_schema| {
                match opt_schema {
                    Some(schema) => Ok(schema),
                    None => Err(HttpError::NotFound(&format!("Composite field schema not found: {}", field_name))),
                }
            })
    }
    pub fn list_composite_field_schemas(&self) -> Result<HashMap<CompositeFieldId, CompositeFieldSchema>, HttpError> {
        self.composite_field_repository.list_composite_field_schemas()
            .map_err(map_internal_error)
    }

    /// Reject references to composites that do not exist, and references that would form a
    /// cycle. Both matter because values are parsed and rendered by following references:
    /// a cycle never terminates.
    fn validate_composite_graph(
        &self,
        id: &CompositeFieldId,
        schema: &CompositeFieldSchema,
    ) -> Result<(), HttpError> {
        let all = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?;

        // A composite is never an available target for its own references; if it were,
        // the cycle check below would not report the self-reference.
        let mut available: HashSet<CompositeFieldId> = all.keys().cloned().collect();
        available.remove(id);

        validate_no_composite_cycles(id, schema, &all).map_err(|e| HttpError::BadRequest(&e))?;
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))
    }
    pub fn add_composite_field_schema(&self, field_name: &CompositeFieldId, schema: &CompositeFieldSchema) -> Result<(), HttpError> {
        validate_schema(schema).map_err(|e| HttpError::BadRequest(&e))?;
        self.validate_composite_graph(field_name, schema)?;
        let s = self.composite_field_repository.get_composite_field_schema(&field_name)
            .map_err(map_internal_error)?;
        match s {
            Some(_) => {
                return Err(HttpError::Conflict(&format!("Composite field schema already exists: {}", field_name)));
            },
            None => {
                self.composite_field_repository.add_composite_field_schema(field_name, schema)
                    .map_err(map_internal_error)
            },
        }
    }
    pub fn update_composite_field_schema(&self, field_name: &CompositeFieldId, schema: &Vec<FieldSchema>) -> Result<(), HttpError> {
        validate_schema(schema).map_err(|e| HttpError::BadRequest(&e))?;
        self.validate_composite_graph(field_name, schema)?;
        let s = self.composite_field_repository.get_composite_field_schema(field_name)
            .map_err(map_internal_error)?;
        match s {
            Some(_) => {
                self.composite_field_repository.add_composite_field_schema(field_name,schema)
                    .map_err(map_internal_error)
            },
            None => {
                return Err(HttpError::NotFound(&format!("Composite field schema not found: {}", field_name)));
            },
        }
    }
    pub fn delete_composite_field_schema(&self, field_name: &CompositeFieldId) -> Result<(), HttpError> {
        let s = self.composite_field_repository.get_composite_field_schema(field_name)
            .map_err(map_internal_error)?;
        match s {
            Some(_) => {
                self.composite_field_repository.delete_composite_field_schema(field_name)
                    .map_err(map_internal_error)
            },
            None => {
                return Err(HttpError::NotFound(&format!("Composite field schema not found: {}", field_name)));
            },
        }
    }
}


#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use std::vec;

    use crate::models::field::{CompositeFieldSchema, TextFieldOptions};
    use crate::models::field::{FieldSchema, FieldType};

    use super::*;

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

    // Test helper functions
    fn create_test_service() -> CompositeFieldService<MockCompositeFieldRepository> {
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        CompositeFieldService::new(Arc::new(composite_field_repository))
    }

    #[test]
    fn test_create_composite_field_service() {
        let service = create_test_service();
        assert!(service.list_composite_field_schemas().is_ok());
    }
    #[test]
    fn test_list_composite_field_schemas_empty() {
        let service = create_test_service();
        let composite_field_schemas = service.list_composite_field_schemas().unwrap();
        assert_eq!(composite_field_schemas.len(), 0);
    }

    #[test]
    fn test_get_composite_field_schema_not_found() {
        let service = create_test_service();
        let result = service.get_composite_field_schema(&"non_existent".into());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Composite field schema not found: non_existent"));
    }

    #[test]
    fn test_add_composite_field_schema_success() {
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

        let result = service.add_composite_field_schema(&"test_field".into(), &schema);
        assert!(result.is_ok());

        let schemas_map = service.list_composite_field_schemas().unwrap();
        assert_eq!(schemas_map, HashMap::from_iter(vec![("test_field".into(), schema.clone())]));
        let retrieved_schema = service.get_composite_field_schema(&"test_field".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_update_composite_field_schema_success() {
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
        service.add_composite_field_schema(&"test_field".into(), &initial_schema).unwrap();

        let updated_schema = vec![
            FieldSchema {
                name: "title2".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.update_composite_field_schema(&"test_field".into(), &updated_schema);
        assert!(result.is_ok());

        let retrieved_schema = service.get_composite_field_schema(&"test_field".into()).unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[test]
    fn test_add_composite_field_schema_already_exists() {
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
        service.add_composite_field_schema(&"test_field".into(), &schema).unwrap();

        let duplicate_schema = vec![
            FieldSchema {
                name: "other".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.add_composite_field_schema(&"test_field".into(), &duplicate_schema);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::Conflict("Composite field schema already exists: test_field"));

        let retrieved_schema = service.get_composite_field_schema(&"test_field".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_delete_composite_field_success() {
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
        service.add_composite_field_schema(&"test_field".into(), &schema).unwrap();

        let result = service.delete_composite_field_schema(&"test_field".into());
        assert!(result.is_ok());

        let schemas_map = service.list_composite_field_schemas().unwrap();
        assert_eq!(schemas_map.len(), 0);
    }

    #[test]
    fn test_update_missing_schema() {
        let service = create_test_service();
        let update_result = service.update_composite_field_schema(
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
}
