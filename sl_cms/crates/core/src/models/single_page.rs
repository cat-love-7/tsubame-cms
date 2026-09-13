use std::collections::HashMap;

use crate::models::{field::{FieldSchema,FieldValueMap, FieldValueResponse}, identity::StringId};
pub type SinglePageSchema = Vec<FieldSchema>;

pub type SinglePageItem = FieldValueMap<SinglePageSchema>;
pub type SinglePageItemResponse = HashMap<String, FieldValueResponse>;
pub type SinglePageName = StringId<SinglePageSchema>;