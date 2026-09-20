use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::models::error::{HttpError, map_internal_error};
use crate::models::schema::{
    CompositeFieldId, RelationTarget, SchemaScope, referenced_relation_targets,
    validate_composite_references, validate_no_composite_cycles, validate_relation_targets,
    validate_schema,
};
use crate::models::values::{CompositeFieldSchema, FieldSchema};
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::relation_targets::RelationTargetSource;

pub struct CompositeFieldService<CFR: CompositeFieldRepository> {
    composite_field_repository: Arc<CFR>,
    /// What a relation may point at. A definition may hold one, and it is stored on its own, so
    /// this is where such a target is checked.
    relation_targets: Arc<dyn RelationTargetSource>,
}

impl<CFR: CompositeFieldRepository> CompositeFieldService<CFR> {
    pub fn new(
        composite_field_repository: Arc<CFR>,
        relation_targets: Arc<dyn RelationTargetSource>,
    ) -> Self {
        CompositeFieldService {
            composite_field_repository,
            relation_targets,
        }
    }
    pub async fn get_composite_field_schema(
        &self,
        field_name: &CompositeFieldId,
    ) -> Result<CompositeFieldSchema, HttpError> {
        self.composite_field_repository
            .get_composite_field_schema(field_name)
            .await
            .map_err(map_internal_error)
            .and_then(|opt_schema| match opt_schema {
                Some(schema) => Ok(schema),
                None => Err(HttpError::NotFound(&format!(
                    "Composite field schema not found: {}",
                    field_name
                ))),
            })
    }
    pub async fn list_composite_field_schemas(
        &self,
    ) -> Result<HashMap<CompositeFieldId, CompositeFieldSchema>, HttpError> {
        self.composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)
    }

    /// Reject references to composites that do not exist, references that would form a cycle, and
    /// relations that point at nothing.
    ///
    /// All three matter because values are parsed and rendered by following references: a cycle
    /// never terminates, and a target that does not exist leaves a field that can never hold
    /// anything. The relation is checked here rather than only where the definition is embedded,
    /// because a definition that nobody embeds yet is still a schema somebody can save.
    async fn validate_composite_graph(
        &self,
        id: &CompositeFieldId,
        schema: &CompositeFieldSchema,
    ) -> Result<(), HttpError> {
        let all = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?;

        // The definition being saved counts as existing: a block that holds a list of blocks
        // references itself, and it will exist the moment this returns. The loops that would not
        // terminate are refused by the cycle check below, which reads the reference graph rather
        // than this set.
        let mut available: HashSet<CompositeFieldId> = all.keys().cloned().collect();
        available.insert(id.clone());

        validate_no_composite_cycles(id, schema, &all).map_err(|e| HttpError::BadRequest(&e))?;
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))?;

        // The saved definition is what a relation inside it is read through, so it is part of the
        // map this walk follows - including when it references itself through an array.
        let mut composites = all;
        composites.insert(id.clone(), schema.clone());
        if referenced_relation_targets(schema, &composites).is_empty() {
            return Ok(());
        }
        let targets: Vec<RelationTarget> = self
            .relation_targets
            .relation_targets()
            .await
            .map_err(map_internal_error)?;
        validate_relation_targets(schema, &composites, &targets)
            .map_err(|e| HttpError::BadRequest(&e))
    }
    pub async fn add_composite_field_schema(
        &self,
        field_name: &CompositeFieldId,
        schema: &CompositeFieldSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::CompositeDefinition)
            .map_err(|e| HttpError::BadRequest(&e))?;
        self.validate_composite_graph(field_name, schema).await?;
        let s = self
            .composite_field_repository
            .get_composite_field_schema(&field_name)
            .await
            .map_err(map_internal_error)?;
        match s {
            Some(_) => {
                return Err(HttpError::Conflict(&format!(
                    "Composite field schema already exists: {}",
                    field_name
                )));
            }
            None => self
                .composite_field_repository
                .add_composite_field_schema(field_name, schema)
                .await
                .map_err(map_internal_error),
        }
    }
    pub async fn update_composite_field_schema(
        &self,
        field_name: &CompositeFieldId,
        schema: &Vec<FieldSchema>,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::CompositeDefinition)
            .map_err(|e| HttpError::BadRequest(&e))?;
        self.validate_composite_graph(field_name, schema).await?;
        let s = self
            .composite_field_repository
            .get_composite_field_schema(field_name)
            .await
            .map_err(map_internal_error)?;
        match s {
            Some(_) => self
                .composite_field_repository
                .add_composite_field_schema(field_name, schema)
                .await
                .map_err(map_internal_error),
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Composite field schema not found: {}",
                    field_name
                )));
            }
        }
    }
    pub async fn delete_composite_field_schema(
        &self,
        field_name: &CompositeFieldId,
    ) -> Result<(), HttpError> {
        let s = self
            .composite_field_repository
            .get_composite_field_schema(field_name)
            .await
            .map_err(map_internal_error)?;
        match s {
            Some(_) => self
                .composite_field_repository
                .delete_composite_field_schema(field_name)
                .await
                .map_err(map_internal_error),
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Composite field schema not found: {}",
                    field_name
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use std::vec;

    use crate::models::values::{CompositeFieldSchema, TextFieldOptions};
    use crate::models::values::{FieldSchema, FieldType, RelationOptions};
    use crate::repositories::relation_targets::StaticRelationTargets;

    use super::*;

    struct MockCompositeFieldRepository {
        schemas: Arc<RwLock<HashMap<CompositeFieldId, CompositeFieldSchema>>>,
    }
    impl CompositeFieldRepository for MockCompositeFieldRepository {
        async fn list_composite_field_schemas(
            &self,
        ) -> Result<
            HashMap<CompositeFieldId, CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().clone())
        }
        async fn get_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<Option<CompositeFieldSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(id).cloned())
        }
        async fn add_composite_field_schema(
            &self,
            id: &CompositeFieldId,
            schema: &CompositeFieldSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas
                .write()
                .unwrap()
                .insert(id.clone(), schema.clone());
            Ok(())
        }
        async fn delete_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(id);
            Ok(())
        }
    }

    // Test helper functions
    fn create_test_service() -> CompositeFieldService<MockCompositeFieldRepository> {
        create_test_service_over(Vec::new())
    }

    /// The same, for a site that has something a relation could point at.
    fn create_test_service_over(
        targets: Vec<RelationTarget>,
    ) -> CompositeFieldService<MockCompositeFieldRepository> {
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        CompositeFieldService::new(
            Arc::new(composite_field_repository),
            Arc::new(StaticRelationTargets::new(targets)),
        )
    }

    fn relation_field(name: &str, target: &str) -> FieldSchema {
        FieldSchema {
            is_title: false,
            show_in_list: false,
            name: name.to_string(),
            field_type: FieldType::Relation(RelationOptions {
                target: RelationTarget::Collection {
                    name: target.to_string(),
                },
                has_many: false,
                inverse_name: None,
            }),
            required: false,
            width: 12,
            height: 1,
            unique: false,
        }
    }

    /// A definition is stored on its own and reused, so a relation inside one points at a
    /// collection or a page of the site - and that target is checked when the definition is
    /// saved, not only when somebody embeds it.
    #[tokio::test]
    async fn a_composite_definition_may_hold_a_relation_to_what_the_site_has() {
        let service = create_test_service_over(vec![RelationTarget::Collection {
            name: "authors".to_string(),
        }]);
        let schema = vec![relation_field("author", "authors")];

        assert!(
            service
                .add_composite_field_schema(&"cta".into(), &schema)
                .await
                .is_ok()
        );

        // A target the site does not have is refused rather than stored.
        let service = create_test_service();
        let error = service
            .add_composite_field_schema(&"cta".into(), &vec![relation_field("author", "writers")])
            .await
            .unwrap_err();
        assert_eq!(error.status_code, 400, "{}", error.message);
        assert!(
            error.message.contains("'writers' is not a collection"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn create_composite_field_service() {
        let service = create_test_service();
        assert!(service.list_composite_field_schemas().await.is_ok());
    }
    #[tokio::test]
    async fn list_composite_field_schemas_empty() {
        let service = create_test_service();
        let composite_field_schemas = service.list_composite_field_schemas().await.unwrap();
        assert_eq!(composite_field_schemas.len(), 0);
    }

    #[tokio::test]
    async fn get_composite_field_schema_not_found() {
        let service = create_test_service();
        let result = service
            .get_composite_field_schema(&"non_existent".into())
            .await;
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Composite field schema not found: non_existent")
        );
    }

    #[tokio::test]
    async fn add_composite_field_schema_success() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];

        let result = service
            .add_composite_field_schema(&"test_field".into(), &schema)
            .await;
        assert!(result.is_ok());

        let schemas_map = service.list_composite_field_schemas().await.unwrap();
        assert_eq!(
            schemas_map,
            HashMap::from_iter(vec![("test_field".into(), schema.clone())])
        );
        let retrieved_schema = service
            .get_composite_field_schema(&"test_field".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[tokio::test]
    async fn update_composite_field_schema_success() {
        let service = create_test_service();
        let initial_schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_composite_field_schema(&"test_field".into(), &initial_schema)
            .await
            .unwrap();

        let updated_schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title2".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        let result = service
            .update_composite_field_schema(&"test_field".into(), &updated_schema)
            .await;
        assert!(result.is_ok());

        let retrieved_schema = service
            .get_composite_field_schema(&"test_field".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[tokio::test]
    async fn add_composite_field_schema_already_exists() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_composite_field_schema(&"test_field".into(), &schema)
            .await
            .unwrap();

        let duplicate_schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "other".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        let result = service
            .add_composite_field_schema(&"test_field".into(), &duplicate_schema)
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::Conflict("Composite field schema already exists: test_field")
        );

        let retrieved_schema = service
            .get_composite_field_schema(&"test_field".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[tokio::test]
    async fn delete_composite_field_success() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_composite_field_schema(&"test_field".into(), &schema)
            .await
            .unwrap();

        let result = service
            .delete_composite_field_schema(&"test_field".into())
            .await;
        assert!(result.is_ok());

        let schemas_map = service.list_composite_field_schemas().await.unwrap();
        assert_eq!(schemas_map.len(), 0);
    }

    #[tokio::test]
    async fn update_missing_schema() {
        let service = create_test_service();
        let update_result = service
            .update_composite_field_schema(
                &"non_existent".into(),
                &vec![FieldSchema {
                    is_title: false,
                    show_in_list: false,
                    name: "title".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                }],
            )
            .await;
        assert!(update_result.is_err());
    }
}
