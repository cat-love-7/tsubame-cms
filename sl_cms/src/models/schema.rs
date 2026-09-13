use serde::{Deserialize, Serialize};

use crate::models::identity::StringId;

use super::field::{FieldValue};

#[cfg(test)]
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    /// in colspan units (1-12)
    pub width: u32,
    /// in rowspan units (1-)
    pub height: u32,
}
pub type CompositeFieldId = StringId<CompositeFieldSchema>;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CompositeFieldReference {
    pub id: CompositeFieldId,
}

pub type CompositeFieldSchema = Vec<FieldSchema>;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, Copy)]
pub struct TextFieldOptions {
    pub max_length: Option<usize>,
    pub min_length: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(EnumIter))]
pub enum FieldType {
    Text(TextFieldOptions),
    Markdown(TextFieldOptions),
    Number,
    Boolean,
    Date,
    DateTime,
    Image,
    CompositeField(CompositeFieldReference),
    Array(Vec<FieldType>),
    TextEnum(Vec<String>),
}

impl Default for FieldType {
    fn default() -> Self {
        FieldType::Text(TextFieldOptions::default())
    }
}
impl FieldType {
    pub fn test_required(&self, value: &FieldValue) -> bool {
        match (self, value) {
            (FieldType::Text(_), FieldValue::Text(text)) => !text.is_empty(),
            (FieldType::Markdown(_), FieldValue::Markdown(markdown)) => !markdown.is_empty(),
            (FieldType::Number, FieldValue::Number(num_opt)) => num_opt.is_some(),
            (FieldType::Boolean, FieldValue::Boolean(_)) => true,
            (FieldType::Date, FieldValue::Date(date_opt)) => date_opt.is_some(),
            (FieldType::DateTime, FieldValue::DateTime(date_time_opt)) => date_time_opt.is_some(),
            (FieldType::Image, FieldValue::Image(img_opt)) => img_opt.is_some(),
            (FieldType::CompositeField(_), FieldValue::CompositeField(value)) => value.is_some(),
            (FieldType::Array(_), FieldValue::Array(values)) => !values.is_empty(),
            (FieldType::TextEnum(_), FieldValue::TextEnum(vals)) => !vals.is_empty(),
            _ => false,
        }
    }
}
impl FieldSchema {
    pub fn validate_text_length(
        &self,
        value: &str,
        options: &TextFieldOptions,
        index: Option<usize>,
    ) -> Result<(), String> {
        if let Some(max_length) = options.max_length {
            if value.len() > max_length {
                return Err(match index {
                    Some(idx) => format!(
                        "field {}[{}] exceeds maximum length of {}",
                        self.name, idx, max_length
                    ),
                    None => format!(
                        "field {} exceeds maximum length of {}",
                        self.name, max_length
                    ),
                });
            }
        }
        if let Some(min_length) = options.min_length {
            if value.len() < min_length && !value.is_empty() {
                return Err(match index {
                    Some(idx) => format!(
                        "field {}[{}] is below minimum length of {}",
                        self.name, idx, min_length
                    ),
                    None => format!(
                        "field {} is below minimum length of {}",
                        self.name, min_length
                    ),
                });
            }
        }
        Ok(())
    }
    pub fn get_default_value(&self) -> FieldValue {
        match &self.field_type {
            FieldType::Text(_) => FieldValue::Text(String::new()),
            FieldType::Markdown(_) => FieldValue::Markdown(String::new()),
            FieldType::Number => FieldValue::Number(None),
            FieldType::Boolean => FieldValue::Boolean(false),
            FieldType::Date => FieldValue::Date(None),
            FieldType::DateTime => FieldValue::DateTime(None),
            FieldType::Image => FieldValue::Image(None),
            FieldType::CompositeField(_) => FieldValue::CompositeField(None),
            FieldType::Array(_schema) => FieldValue::Array(vec![]),
            FieldType::TextEnum(_options) => FieldValue::TextEnum(vec![]),
        }
    }
}
/*
pub fn format_to_schema(
    composite_schemas: &HashMap<String, CompositeFieldSchema>,
    schema: &Vec<FieldSchema>,
    values: &FieldValueMap,
) -> FieldValueMap {
    let mut ret = HashMap::new();
    for field in schema {
        if let Some(v) = values.get(&field.name) {
            ret.insert(
                field.name.clone(),
                field.format_field_value(composite_schemas, v),
            );
        } else {
            ret.insert(field.name.clone(), field.get_default_value());
        }
    }
    ret
}
pub fn validate_to_schema(
    composite_schemas: &HashMap<String, CompositeFieldSchema>,
    schema: &Vec<FieldSchema>,
    values: &FieldValueMap,
) -> Result<(), String> {
    for field in schema {
        if let Some(v) = values.get(&field.name) {
            field.validate_field_value(composite_schemas, v)?;
        } else if field.required {
            return Err(format!("Field '{}' is missing", field.name));
        }
    }
    Ok(())
}
*/