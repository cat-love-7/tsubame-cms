use serde::{Deserialize, Serialize};

use crate::models::error::FieldRefusal;
use crate::models::identity::StringId;

use super::field::FieldValue;
use crate::repositories::collection_repository::UniqueValue;

#[cfg(test)]
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    /// Whether this field's value has to be unique inside its collection.
    ///
    /// Held by an index of value to item (see `UniqueValue`), so the check costs a point read
    /// rather than a scan, and two saves of the same value cannot both succeed. Only text fields
    /// may set it; a slug needs normalising, which is a separate field type's job.
    ///
    /// Omitted from the wire when false, so a schema that uses no unique fields reads exactly as
    /// it did before this existed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
    /// in colspan units (1-12)
    pub width: u32,
    /// in rowspan units (1-)
    pub height: u32,
}
/// `#[serde(skip_serializing_if)]` takes a predicate, and `bool` has no `is_false`.
fn is_false(value: &bool) -> bool {
    !*value
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
    /// Whether `value` is within the lengths the field was given.
    ///
    /// `path` is where the value sits in the item (`body`, `tags[2]`, `seo.description`), which is
    /// what the refusal names: the caller knows the path, including the array index, and this
    /// knows the limits. An empty optional value is never below the minimum - a field that must
    /// not be left empty says so with `required`.
    ///
    /// The limits are counted in **characters**, not bytes: a schema that says 20 means twenty
    /// Japanese characters as much as twenty Latin ones, and the editor counts the same way (it
    /// counts code points, which is one per character for everything but astral emoji).
    pub fn validate_text_length(
        &self,
        value: &str,
        options: &TextFieldOptions,
        path: &str,
    ) -> Result<(), FieldRefusal> {
        let length = value.chars().count();
        if let Some(max_length) = options.max_length {
            if length > max_length {
                return Err(FieldRefusal::too_long(path, max_length));
            }
        }
        if let Some(min_length) = options.min_length {
            if length < min_length && !value.is_empty() {
                return Err(FieldRefusal::too_short(path, min_length));
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

/// What a schema is for, which decides what `unique` can mean.
///
/// A collection holds many items, so a value can be compared across them. A single page holds
/// exactly one, and a composite definition's fields are embedded in whatever item uses them, so
/// neither has anything to compare against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchemaScope {
    Collection,
    SinglePage,
    CompositeDefinition,
}

/// Validate a field schema before it is stored.
///
/// Without this an unusable schema can be saved and only fails later, when someone tries
/// to write content against it (or, worse, cannot interpret what is already stored).
pub fn validate_schema(fields: &[FieldSchema], scope: SchemaScope) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        let name = field.name.trim();
        if name.is_empty() {
            return Err("field names must not be empty".to_string());
        }
        if !seen.insert(name.to_string()) {
            return Err(format!("duplicate field name '{name}'"));
        }
        if field.unique {
            match scope {
                SchemaScope::Collection => {}
                SchemaScope::SinglePage => {
                    return Err(format!(
                        "field '{name}': a single page has one item, so 'unique' would compare                          nothing"
                    ));
                }
                SchemaScope::CompositeDefinition => {
                    return Err(format!(
                        "field '{name}': a composite's values are stored inside the item that                          uses it, so 'unique' is not tracked for its fields"
                    ));
                }
            }
            if !matches!(field.field_type, FieldType::Text(_)) {
                return Err(format!(
                    "field '{name}': only a text field can be unique"
                ));
            }
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

/// The unique values `item` holds, ready for the index.
///
/// Trimmed, and blank ones left out: an optional unique field that nobody filled in must not
/// collide with every other item that left it blank either. A field the item does not have is
/// the same as blank.
pub fn unique_values(
    fields: &[FieldSchema],
    item: &crate::models::field::FieldValueMap<Vec<FieldSchema>>,
) -> Vec<UniqueValue> {
    let mut values = Vec::new();
    for field in fields {
        if !field.unique {
            continue;
        }
        let Some(FieldValue::Text(text)) = item.get(&field.name) else {
            continue;
        };
        let value = text.trim();
        if value.is_empty() {
            continue;
        }
        values.push(UniqueValue {
            field: field.name.clone(),
            value: value.to_string(),
        });
    }
    values
}

/// Whether any field declares `unique`, so a caller can skip the bookkeeping (and the reads it
/// needs) for the collections that do not.
pub fn has_unique_fields(fields: &[FieldSchema]) -> bool {
    fields.iter().any(|field| field.unique)
}

/// Every composite id referenced by `fields`, including inside array item types.
pub fn referenced_composite_ids(fields: &[FieldSchema]) -> Vec<CompositeFieldId> {
    let mut ids = Vec::new();
    collect_type_references_in(fields, &mut ids);
    ids
}

/// The composite ids a value of `fields` reaches *without* going through an array.
///
/// This is the graph [`validate_no_composite_cycles`] walks: a loop through an array is fine,
/// because the array's elements come from the stored value and an empty array stops there.
pub fn directly_referenced_composite_ids(fields: &[FieldSchema]) -> Vec<CompositeFieldId> {
    let mut ids = Vec::new();
    for field in fields {
        collect_direct_references(&field.field_type, &mut ids);
    }
    ids
}

fn collect_direct_references(field_type: &FieldType, into: &mut Vec<CompositeFieldId>) {
    if let FieldType::CompositeField(reference) = field_type {
        into.push(reference.id.clone());
    }
    // An array is where the recursion stops being about the schema: its elements are whatever
    // the value holds, so its item types are not part of this graph.
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

/// A composite must not reach itself without passing through an array.
///
/// The editor draws a composite's sub-fields from the *schema*, so `a → b → a` would build
/// component after component until the browser gives up, whatever the value says. Through an
/// array it stops: elements are drawn from the *value*, so an empty array draws nothing, and a
/// deeper tree costs a deeper value. That is what lets a block definition hold a list of blocks.
pub fn validate_no_composite_cycles(
    id: &CompositeFieldId,
    fields: &[FieldSchema],
    all: &std::collections::HashMap<CompositeFieldId, CompositeFieldSchema>,
) -> Result<(), String> {
    let mut stack = directly_referenced_composite_ids(fields);
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
            stack.extend(directly_referenced_composite_ids(schema));
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
            unique: false,
        }
    }

    /// The limits are characters, not bytes: a twenty-character Japanese title fits a schema
    /// that says 20, and the refusal names the path the caller gave.
    #[test]
    fn a_text_limit_counts_characters() {
        let options = TextFieldOptions {
            max_length: Some(20),
            min_length: Some(4),
        };
        let field = field("title", FieldType::Text(options));

        // Twenty Japanese characters are twenty characters, however many bytes they take.
        assert_eq!("あ".repeat(20).chars().count(), 20);
        assert!(field.validate_text_length(&"あ".repeat(20), &options, "title").is_ok());
        assert!(field.validate_text_length(&"あ".repeat(21), &options, "title").is_err());
        assert!(field.validate_text_length("短い", &options, "title").is_err());

        let refusal = field
            .validate_text_length(&"あ".repeat(21), &options, "title")
            .unwrap_err();
        assert_eq!(refusal.code, "field_too_long");
        assert_eq!(refusal.field, "title");

        let refusal = field
            .validate_text_length("短い", &options, "tags[2]")
            .unwrap_err();
        assert_eq!(refusal.code, "field_too_short");
        assert_eq!(refusal.field, "tags[2]");

        // An empty optional value is not below the minimum; `required` is what says that.
        assert!(field.validate_text_length("", &options, "title").is_ok());
    }

    #[test]
    fn accepts_a_reasonable_schema() {
        let schema = vec![
            field("title", FieldType::Text(TextFieldOptions::default())),
            field("count", FieldType::Number),
            field("covers", FieldType::Array(vec![FieldType::Image])),
        ];
        assert!(validate_schema(&schema, SchemaScope::Collection).is_ok());
        // An empty schema is allowed: a collection can be created before its fields.
        assert!(validate_schema(&[], SchemaScope::Collection).is_ok());
    }

    #[test]
    fn rejects_empty_and_duplicate_names() {
        assert!(validate_schema(&[field("  ", FieldType::Number)], SchemaScope::Collection)
            .unwrap_err()
            .contains("must not be empty"));

        let duplicate = vec![field("title", FieldType::Number), field("title", FieldType::Boolean)];
        assert!(validate_schema(&duplicate, SchemaScope::Collection).unwrap_err().contains("duplicate field name"));
    }

    #[test]
    fn rejects_out_of_range_layout() {
        let mut too_wide = field("a", FieldType::Number);
        too_wide.width = 13;
        assert!(validate_schema(&[too_wide], SchemaScope::Collection).unwrap_err().contains("width"));

        let mut too_narrow = field("a", FieldType::Number);
        too_narrow.width = 0;
        assert!(validate_schema(&[too_narrow], SchemaScope::Collection).unwrap_err().contains("width"));

        let mut no_height = field("a", FieldType::Number);
        no_height.height = 0;
        assert!(validate_schema(&[no_height], SchemaScope::Collection).unwrap_err().contains("height"));
    }

    #[test]
    fn an_image_array_is_allowed_on_its_own() {
        let schema = vec![field("covers", FieldType::Array(vec![FieldType::Image]))];
        assert!(validate_schema(&schema, SchemaScope::Collection).is_ok());
    }

    #[test]
    fn rejects_number_and_image_array_items_together() {
        let schema = vec![field(
            "mixed",
            FieldType::Array(vec![FieldType::Number, FieldType::Image]),
        )];
        let error = validate_schema(&schema, SchemaScope::Collection).unwrap_err();
        assert!(error.contains("both Number and Image"), "unexpected error: {error}");
    }

    #[test]
    fn rejects_empty_nested_and_composite_array_items() {
        assert!(validate_schema(&[field("a", FieldType::Array(vec![]))], SchemaScope::Collection)
            .unwrap_err()
            .contains("at least one item type"));

        assert!(validate_schema(
            &[field(
                "a",
                FieldType::Array(vec![FieldType::Array(vec![FieldType::Number])])
            )],
            SchemaScope::Collection
        )
        .unwrap_err()
        .contains("nested arrays"));

        // A composite as an array item is allowed: it is an object on the wire, so it cannot be
        // confused with a scalar, and the reference is checked like any other.
        validate_schema(
            &[field(
                "a",
                FieldType::Array(vec![FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::from("seo"),
                })])
            )],
            SchemaScope::Collection,
        )
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

    /// A block that holds a list of blocks is the point of an array of composites, and it is a
    /// loop in the reference graph. It is allowed because every edge of this loop goes through an
    /// array: the editor draws array elements from the value, so an empty array draws nothing.
    #[test]
    fn allows_a_composite_that_reaches_itself_through_an_array() {
        let id = CompositeFieldId::from("tree");
        let tree = vec![
            field("line", FieldType::Text(TextFieldOptions::default())),
            field(
                "children",
                FieldType::Array(vec![composite_ref("tree")]),
            ),
        ];
        assert!(validate_no_composite_cycles(&id, &tree, &HashMap::new()).is_ok());

        // ...including when the loop takes a detour: tree -> other -> array of tree.
        let mut all: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        all.insert(
            CompositeFieldId::from("other"),
            vec![field("again", FieldType::Array(vec![composite_ref("tree")]))],
        );
        let detour = vec![field("other", composite_ref("other"))];
        assert!(validate_no_composite_cycles(&id, &detour, &all).is_ok());

        // But a route back that never enters an array is still refused.
        let mut back: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        back.insert(
            CompositeFieldId::from("other"),
            vec![field("back", composite_ref("tree"))],
        );
        assert!(validate_no_composite_cycles(&id, &detour, &back)
            .unwrap_err()
            .contains("cannot reference itself"));
    }

    #[test]
    fn the_two_reference_walks_differ_only_at_arrays() {
        let fields = vec![
            field("direct", composite_ref("direct")),
            field(
                "in_array",
                FieldType::Array(vec![composite_ref("in_array")]),
            ),
        ];

        let mut both: Vec<String> = referenced_composite_ids(&fields)
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        both.sort();
        assert_eq!(both, vec!["direct".to_string(), "in_array".to_string()]);

        let stops: Vec<String> = directly_referenced_composite_ids(&fields)
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        assert_eq!(stops, vec!["direct".to_string()]);
    }
}
