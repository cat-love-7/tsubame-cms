use std::{collections::HashMap, error::Error};
use crate::models::{field::CompositeFieldSchema, schema::CompositeFieldId};

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

pub trait CompositeFieldRepository {
    fn list_composite_field_schemas(&self) -> Result<HashMap<CompositeFieldId, CompositeFieldSchema>, BoxError>;
    fn get_composite_field_schema(&self, id: &CompositeFieldId) -> Result<Option<CompositeFieldSchema>, BoxError>;
    fn add_composite_field_schema(&self, id: &CompositeFieldId, schema: &CompositeFieldSchema) -> Result<(), BoxError>;
    fn delete_composite_field_schema(&self, id: &CompositeFieldId) -> Result<(), BoxError>;
}