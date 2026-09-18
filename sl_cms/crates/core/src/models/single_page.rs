use std::collections::HashMap;

use crate::models::{
    identity::StringId,
    values::{FieldSchema, FieldValueMap, FieldValueResponse},
};
pub type SinglePageSchema = Vec<FieldSchema>;

pub type SinglePageItem = FieldValueMap<SinglePageSchema>;
pub type SinglePageItemResponse = HashMap<String, FieldValueResponse>;
pub type SinglePageName = StringId<SinglePageSchema>;
