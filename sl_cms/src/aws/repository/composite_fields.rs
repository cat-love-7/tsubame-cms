//! Composite field schemas: one record per schema, listed in a single query.
//!
//! They are small and few, and a collection's field can reference one by id, so the whole set is
//! read on every schema edit; one partition (`pk = composite_fields`) keeps that to one query.

use std::collections::HashMap;

use super::*;
use crate::models::field::CompositeFieldSchema;
use crate::models::schema::CompositeFieldId;
use crate::repositories::composite_field_repository::CompositeFieldRepository;

impl CompositeFieldRepository for AwsRepository {
    fn list_composite_field_schemas(
        &self,
    ) -> Result<HashMap<CompositeFieldId, CompositeFieldSchema>, BoxError> {
        let inner = self.inner.clone();
        self.runtime.block_on(async move {
            let mut schemas = HashMap::new();
            for (sk, data) in list(&inner, key::COMPOSITE_FIELD_INDEX, "composite#").await? {
                let id = CompositeFieldId::from(sk.trim_start_matches("composite#"));
                schemas.insert(id, AwsRepository::decode(&data)?);
            }
            Ok(schemas)
        })
    }

    fn get_composite_field_schema(
        &self,
        id: &CompositeFieldId,
    ) -> Result<Option<CompositeFieldSchema>, BoxError> {
        let inner = self.inner.clone();
        let id = id.clone();
        self.runtime.block_on(async move {
            match read(&inner, key::COMPOSITE_FIELD_INDEX, &key::composite_field(&id)).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn add_composite_field_schema(
        &self,
        id: &CompositeFieldId,
        schema: &CompositeFieldSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let id = id.clone();
        let data = AwsRepository::encode(schema)?;
        self.runtime.block_on(async move {
            write(&inner, key::COMPOSITE_FIELD_INDEX, &key::composite_field(&id), &data).await
        })
    }

    fn delete_composite_field_schema(&self, id: &CompositeFieldId) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let id = id.clone();
        self.runtime.block_on(async move {
            remove(&inner, key::COMPOSITE_FIELD_INDEX, &key::composite_field(&id)).await
        })
    }
}
