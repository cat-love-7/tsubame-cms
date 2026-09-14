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

/// Validate a field schema before it is stored.
///
/// Without this an unusable schema can be saved and only fails later, when someone tries
/// to write content against it (or, worse, cannot interpret what is already stored).
pub fn validate_schema(fields: &[FieldSchema]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        let name = field.name.trim();
        if name.is_empty() {
            return Err("field names must not be empty".to_string());
        }
        if !seen.insert(name.to_string()) {
            return Err(format!("duplicate field name '{name}'"));
        }
        if !(1..=12).contains(&field.width) {
            return Err(format!(
                "field '{name}': width must be between 1 and 12 (got {})",
                field.width
            ));
        }
        if field.height < 1 {
            return Err(format!(
                "field '{name}': height must be at least 1 (got {})",
                field.height
            ));
        }
        validate_field_type(name, &field.field_type)?;
    }
    Ok(())
}

fn validate_field_type(name: &str, field_type: &FieldType) -> Result<(), String> {
    let FieldType::Array(items) = field_type else {
        return Ok(());
    };

    if items.is_empty() {
        return Err(format!(
            "field '{name}': an array must declare at least one item type"
        ));
    }
    for item in items {
        if let FieldType::Array(_) = item {
            return Err(format!("field '{name}': nested arrays are not supported"));
        }
    }

    // Array items are untyped, and an image id is a JSON number, so a bare number would
    // be ambiguous if both Number and Image were declared. Either on its own is fine.
    let has_number = items.iter().any(|item| matches!(item, FieldType::Number));
    let has_image = items.iter().any(|item| matches!(item, FieldType::Image));
    if has_number && has_image {
        return Err(format!(
            "field '{name}': an array cannot declare both Number and Image items, because \
             an image id is a number and array items carry no type tag"
        ));
    }
    Ok(())
}

/// Every composite id referenced by `fields`, including inside array item types.
pub fn referenced_composite_ids(fields: &[FieldSchema]) -> Vec<CompositeFieldId> {
    let mut ids = Vec::new();
    collect_type_references_in(fields, &mut ids);
    ids
}

fn collect_type_references_in(fields: &[FieldSchema], into: &mut Vec<CompositeFieldId>) {
    for field in fields {
        collect_type_references(&field.field_type, into);
    }
}

fn collect_type_references(field_type: &FieldType, into: &mut Vec<CompositeFieldId>) {
    match field_type {
        FieldType::CompositeField(reference) => into.push(reference.id.clone()),
        FieldType::Array(items) => {
            for item in items {
                collect_type_references(item, into);
            }
        }
        _ => {}
    }
}

/// A composite reference must point at a composite that exists, otherwise the schema
/// cannot be used to read or write values at all.
pub fn validate_composite_references(
    fields: &[FieldSchema],
    available: &std::collections::HashSet<CompositeFieldId>,
) -> Result<(), String> {
    for id in referenced_composite_ids(fields) {
        if !available.contains(&id) {
            return Err(format!("composite field '{id}' does not exist"));
        }
    }
    Ok(())
}

/// A composite must not reference itself, directly or through other composites.
///
/// Parsing a value would recurse forever, and the editor renders sub-fields by following
/// the references, so a cycle would hang the browser rather than fail visibly.
pub fn validate_no_composite_cycles(
    id: &CompositeFieldId,
    fields: &[FieldSchema],
    all: &std::collections::HashMap<CompositeFieldId, CompositeFieldSchema>,
) -> Result<(), String> {
    let mut stack = referenced_composite_ids(fields);
    let mut visited = std::collections::HashSet::new();
    while let Some(current) = stack.pop() {
        if &current == id {
            return Err(format!(
                "composite field '{id}' cannot reference itself, directly or through \
                 another composite"
            ));
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        if let Some(schema) = all.get(&current) {
            stack.extend(referenced_composite_ids(schema));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    fn field(name: &str, field_type: FieldType) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            field_type,
            required: false,
            width: 12,
            height: 1,
        }
    }

    #[test]
    fn accepts_a_reasonable_schema() {
        let schema = vec![
            field("title", FieldType::Text(TextFieldOptions::default())),
            field("count", FieldType::Number),
            field("covers", FieldType::Array(vec![FieldType::Image])),
        ];
        assert!(validate_schema(&schema).is_ok());
        // An empty schema is allowed: a collection can be created before its fields.
        assert!(validate_schema(&[]).is_ok());
    }

    #[test]
    fn rejects_empty_and_duplicate_names() {
        assert!(validate_schema(&[field("  ", FieldType::Number)])
            .unwrap_err()
            .contains("must not be empty"));

        let duplicate = vec![field("title", FieldType::Number), field("title", FieldType::Boolean)];
        assert!(validate_schema(&duplicate).unwrap_err().contains("duplicate field name"));
    }

    #[test]
    fn rejects_out_of_range_layout() {
        let mut too_wide = field("a", FieldType::Number);
        too_wide.width = 13;
        assert!(validate_schema(&[too_wide]).unwrap_err().contains("width"));

        let mut too_narrow = field("a", FieldType::Number);
        too_narrow.width = 0;
        assert!(validate_schema(&[too_narrow]).unwrap_err().contains("width"));

        let mut no_height = field("a", FieldType::Number);
        no_height.height = 0;
        assert!(validate_schema(&[no_height]).unwrap_err().contains("height"));
    }

    #[test]
    fn an_image_array_is_allowed_on_its_own() {
        let schema = vec![field("covers", FieldType::Array(vec![FieldType::Image]))];
        assert!(validate_schema(&schema).is_ok());
    }

    #[test]
    fn rejects_number_and_image_array_items_together() {
        let schema = vec![field(
            "mixed",
            FieldType::Array(vec![FieldType::Number, FieldType::Image]),
        )];
        let error = validate_schema(&schema).unwrap_err();
        assert!(error.contains("both Number and Image"), "unexpected error: {error}");
    }

    #[test]
    fn rejects_empty_nested_and_composite_array_items() {
        assert!(validate_schema(&[field("a", FieldType::Array(vec![]))])
            .unwrap_err()
            .contains("at least one item type"));

        assert!(validate_schema(&[field(
            "a",
            FieldType::Array(vec![FieldType::Array(vec![FieldType::Number])])
        )])
        .unwrap_err()
        .contains("nested arrays"));

        // A composite as an array item is allowed: it is an object on the wire, so it cannot be
        // confused with a scalar, and the reference is checked like any other.
        validate_schema(&[field(
            "a",
            FieldType::Array(vec![FieldType::CompositeField(CompositeFieldReference {
                id: CompositeFieldId::from("seo"),
            })])
        )])
        .expect("an array of composites is a usable schema");
    }

    fn composite_ref(id: &str) -> FieldType {
        FieldType::CompositeField(CompositeFieldReference {
            id: CompositeFieldId::from(id),
        })
    }

    #[test]
    fn collects_referenced_composite_ids() {
        let fields = vec![
            field("title", FieldType::Number),
            field("seo", composite_ref("seo")),
            field("nested", composite_ref("other")),
        ];
        let mut ids: Vec<String> = referenced_composite_ids(&fields)
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["other".to_string(), "seo".to_string()]);

        assert!(referenced_composite_ids(&[field("plain", FieldType::Number)]).is_empty());
    }

    #[test]
    fn rejects_references_to_composites_that_do_not_exist() {
        let available: HashSet<CompositeFieldId> =
            [CompositeFieldId::from("seo")].into_iter().collect();

        assert!(validate_composite_references(&[field("seo", composite_ref("seo"))], &available).is_ok());
        assert!(validate_composite_references(&[field("nope", composite_ref("nope"))], &available)
            .unwrap_err()
            .contains("composite field 'nope' does not exist"));
    }

    #[test]
    fn rejects_composites_that_reference_themselves() {
        let id = CompositeFieldId::from("seo");

        // Directly.
        let direct = vec![field("self", composite_ref("seo"))];
        assert!(validate_no_composite_cycles(&id, &direct, &HashMap::new())
            .unwrap_err()
            .contains("cannot reference itself"));

        // Through another composite: seo -> other -> seo.
        let indirect = vec![field("other", composite_ref("other"))];
        let mut all: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        all.insert(
            CompositeFieldId::from("other"),
            vec![field("back", composite_ref("seo"))],
        );
        assert!(validate_no_composite_cycles(&id, &indirect, &all)
            .unwrap_err()
            .contains("cannot reference itself"));

        // A plain chain is fine, and a missing target must not loop forever.
        let chain = vec![field("a", composite_ref("a")), field("b", composite_ref("b"))];
        let mut all: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        all.insert(CompositeFieldId::from("a"), vec![field("b", composite_ref("b"))]);
        all.insert(CompositeFieldId::from("b"), vec![]);
        assert!(validate_no_composite_cycles(&id, &chain, &all).is_ok());
        assert!(validate_no_composite_cycles(&id, &vec![field("gone", composite_ref("gone"))], &all).is_ok());
    }
}
