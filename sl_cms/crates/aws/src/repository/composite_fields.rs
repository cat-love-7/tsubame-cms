//! Composite field schemas: one record per schema, listed in a single query.
//!
//! They are small and few, and a collection's field can reference one by id, so the whole set is
//! read on every schema edit; one partition (`pk = composite_fields`) keeps that to one query.

use std::collections::HashMap;

use super::*;
use sl_cms_core::models::field::CompositeFieldSchema;
use sl_cms_core::models::schema::CompositeFieldId;
use sl_cms_core::repositories::composite_field_repository::CompositeFieldRepository;

impl CompositeFieldRepository for AwsRepository {
    async fn list_composite_field_schemas(
        &self,
    ) -> Result<HashMap<CompositeFieldId, CompositeFieldSchema>, BoxError> {
        let inner = self.inner.clone();
        let mut schemas = HashMap::new();
        for (sk, data) in list(&inner, key::COMPOSITE_FIELD_INDEX, "composite#").await? {
            let id = CompositeFieldId::from(sk.trim_start_matches("composite#"));
            schemas.insert(id, AwsRepository::decode(&data)?);
        }
        Ok(schemas)
    }

    async fn get_composite_field_schema(
        &self,
        id: &CompositeFieldId,
    ) -> Result<Option<CompositeFieldSchema>, BoxError> {
        let inner = self.inner.clone();
        let id = id.clone();
        match read(&inner, key::COMPOSITE_FIELD_INDEX, &key::composite_field(&id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn add_composite_field_schema(
        &self,
        id: &CompositeFieldId,
        schema: &CompositeFieldSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let id = id.clone();
        let data = AwsRepository::encode(schema)?;
        write(&inner, key::COMPOSITE_FIELD_INDEX, &key::composite_field(&id), &data).await
    }

    async fn delete_composite_field_schema(&self, id: &CompositeFieldId) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let id = id.clone();
        remove(&inner, key::COMPOSITE_FIELD_INDEX, &key::composite_field(&id)).await
    }
}
