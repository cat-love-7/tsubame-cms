use std::future::Future;
use std::{collections::HashMap, error::Error};

use crate::models::{field::CompositeFieldSchema, schema::CompositeFieldId};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// `Send + Sync + 'static` is required so that a storage adapter implementing this trait
/// can be used as shared HTTP state (axum requires `Sync` state/handlers). The other
/// repository traits already carry this bound.
pub trait CompositeFieldRepository: Send + Sync + 'static {
    fn list_composite_field_schemas(
        &self,
    ) -> impl Future<Output = Result<HashMap<CompositeFieldId, CompositeFieldSchema>, BoxError>> + Send;
    fn get_composite_field_schema(
        &self,
        id: &CompositeFieldId,
    ) -> impl Future<Output = Result<Option<CompositeFieldSchema>, BoxError>> + Send;
    fn add_composite_field_schema(
        &self,
        id: &CompositeFieldId,
        schema: &CompositeFieldSchema,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    fn delete_composite_field_schema(
        &self,
        id: &CompositeFieldId,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}
