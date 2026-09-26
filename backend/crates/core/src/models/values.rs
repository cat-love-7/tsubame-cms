use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, marker::PhantomData};

#[cfg(test)]
use strum_macros::EnumIter;

use crate::models::{
    error::FieldRefusal,
    image::{Image, ImageId, ImageResponse},
    owner::ItemOwner,
    schema::CompositeFieldId,
};

pub use super::schema::{
    CompositeFieldReference, CompositeFieldSchema, FieldSchema, FieldType, RelationOptions,
    RelationTarget, TextFieldOptions,
};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CompositeFieldValue {
    pub id: CompositeFieldId,
    pub values: FieldValueMap<CompositeFieldSchema>,
}

#[derive(Debug, Clone, Default)]
pub struct FieldValueMap<T>(pub HashMap<String, FieldValue>, pub PhantomData<fn() -> T>);
impl<T> FieldValueMap<T>
where
    for<'a> &'a T: IntoIterator<Item = &'a FieldSchema>,
{
    pub fn format_to_schema(
        &self,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
        schema: &T,
    ) -> FieldValueMap<T> {
        let mut ret = HashMap::new();
        for field in schema {
            if let Some(v) = self.0.get(&field.name) {
                ret.insert(
                    field.name.clone(),
                    v.format_field_value(field, composite_schemas),
                );
            } else {
                ret.insert(field.name.clone(), field.get_default_value());
            }
        }
        FieldValueMap(ret, PhantomData)
    }

    /// The wire shape of every field.
    ///
    /// The images are handed in already resolved rather than looked up here: a field's value
    /// may point at an image, and resolving that is a storage call. Doing it here would make
    /// formatting asynchronous all the way down (and a lookup per value); the service reads the
    /// library once per request and passes it in, which also keeps this a pure function.
    pub fn to_response(
        &self,
        images: &HashMap<ImageId, Image>,
    ) -> HashMap<String, FieldValueResponse> {
        self.0
            .iter()
            .map(|(k, v)| (k.clone(), v.to_response(images)))
            .collect()
    }

    /// Every field the schema names must hold something the schema accepts.
    ///
    /// A refusal carries the path to the input - `title`, `tags[2]`, `seo.description` - so the
    /// screen can mark it and the wording can name it. `prefix` is that path so far, which is how
    /// a refusal inside a composite or an array is still located once it surfaces.
    pub fn validate_to_schema(
        &self,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
        schema: &T,
    ) -> Result<(), FieldRefusal> {
        self.validate_at("", composite_schemas, schema)
    }

    /// Validate values for **storing a working copy**, where a required field may still be empty.
    ///
    /// A draft is what an editor is in the middle of: it is not served, and a schema that gains a
    /// required field must not make every item stored before it unsavable. Everything *else* the
    /// schema says about the value still holds - the value is stored either way, and a type or a
    /// length that the field will never accept is worth refusing at the moment it is typed.
    ///
    /// Completeness is required at publication instead
    /// (`CollectionService::set_item_status`), which is the moment the site is affected.
    pub fn validate_draft(
        &self,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
        schema: &T,
    ) -> Result<(), FieldRefusal> {
        match self.validate_to_schema(composite_schemas, schema) {
            Err(refusal) if refusal.is_missing_required() => Ok(()),
            other => other,
        }
    }

    fn validate_at(
        &self,
        prefix: &str,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
        schema: &T,
    ) -> Result<(), FieldRefusal> {
        for field in schema {
            if let Some(v) = self.0.get(&field.name) {
                v.validate_field_at(field, composite_schemas, prefix)?;
            } else if field.required {
                return Err(FieldRefusal::required(&format!("{prefix}{}", field.name)));
            }
        }
        Ok(())
    }

    /// Build a value map from an **untagged** JSON object, using `schema` to decide how
    /// each field is read.
    ///
    /// Values travel untyped on the wire (`{"title":"Hello","count":5}`) because the
    /// schema already states each field's type; carrying a second, redundant type tag
    /// would allow a value whose tag contradicts the schema. The schema is therefore the
    /// single source of truth here.
    ///
    /// Types that do not match are **rejected** rather than coerced, and keys that the
    /// schema does not declare are rejected too, so a client typo cannot silently drop
    /// content.
    pub fn from_untyped(
        value: &serde_json::Value,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
        schema: &T,
    ) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "expected a JSON object of field values".to_string())?;

        for key in object.keys() {
            let mut known = false;
            for field in schema {
                if &field.name == key {
                    known = true;
                    break;
                }
            }
            if !known {
                return Err(format!("Unknown field '{key}'"));
            }
        }

        let mut values = HashMap::new();
        for field in schema {
            if let Some(raw) = object.get(&field.name) {
                values.insert(
                    field.name.clone(),
                    parse_field_value(field, raw, composite_schemas)?,
                );
            }
        }
        Ok(FieldValueMap(values, PhantomData))
    }
}

/// Read one untagged JSON value as the `FieldValue` that `field`'s type implies.
fn parse_field_value(
    field: &FieldSchema,
    raw: &serde_json::Value,
    composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
) -> Result<FieldValue, String> {
    match &field.field_type {
        FieldType::Array(allowed) => {
            let items = raw.as_array().ok_or_else(|| mismatch(field, "an array"))?;
            let mut values = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                values.push(parse_array_element(
                    field,
                    allowed,
                    index,
                    item,
                    composite_schemas,
                )?);
            }
            // An array of relations is several references, and a set has no duplicates: the same
            // one written twice is written once, at the place it first appeared (see
            // `docs/relations-design.md` §3). Nothing else in an array is deduplicated - two equal
            // numbers are two numbers.
            let mut seen = std::collections::HashSet::new();
            values.retain(|value| match value {
                FieldValue::Relation(Some(reference)) => seen.insert(reference.target_owner()),
                _ => true,
            });
            Ok(FieldValue::Array(values))
        }
        FieldType::CompositeField(reference) => match raw {
            serde_json::Value::Null => Ok(FieldValue::CompositeField(None)),
            serde_json::Value::Object(map) => {
                let nested_schema = composite_schemas.get(&reference.id).ok_or_else(|| {
                    format!(
                        "Field '{}': composite field schema '{}' not found",
                        field.name, reference.id
                    )
                })?;

                // Reads wrap a composite as `{ "id": ..., "values": { ... } }`, while a
                // write accepts the bare object of sub-values. Accepting the wrapper back
                // means a client can load an item, change one field and save the rest
                // untouched. A composite that genuinely declares a `values` sub-field is
                // not wrapped, so it is left alone.
                let declares_values = nested_schema.iter().any(|field| field.name == "values");
                let inner = match (map.get("values"), declares_values) {
                    (Some(values @ serde_json::Value::Object(_)), false) => values,
                    _ => raw,
                };

                let nested = FieldValueMap::<CompositeFieldSchema>::from_untyped(
                    inner,
                    composite_schemas,
                    nested_schema,
                )
                .map_err(|e| format!("Field '{}': {e}", field.name))?;
                Ok(FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: reference.id.clone(),
                    values: nested,
                })))
            }
            _ => Err(mismatch(field, "an object")),
        },
        scalar => parse_untagged_scalar(field, scalar, raw),
    }
}

/// The scalar half of [`parse_field_value`]; shared with array elements.
fn parse_untagged_scalar(
    field: &FieldSchema,
    field_type: &FieldType,
    raw: &serde_json::Value,
) -> Result<FieldValue, String> {
    match field_type {
        FieldType::Text(_) | FieldType::Slug(_) => raw
            .as_str()
            .map(|s| FieldValue::Text(s.to_string()))
            .ok_or_else(|| mismatch(field, "a string")),
        FieldType::Markdown(_) => raw
            .as_str()
            .map(|s| FieldValue::Markdown(s.to_string()))
            .ok_or_else(|| mismatch(field, "a string")),
        FieldType::Number => match raw {
            serde_json::Value::Null => Ok(FieldValue::Number(None)),
            serde_json::Value::Number(n) => n
                .as_f64()
                .map(|v| FieldValue::Number(Some(v)))
                .ok_or_else(|| mismatch(field, "a number")),
            _ => Err(mismatch(field, "a number")),
        },
        FieldType::Boolean => raw
            .as_bool()
            .map(FieldValue::Boolean)
            .ok_or_else(|| mismatch(field, "a boolean")),
        FieldType::Date => match raw {
            serde_json::Value::Null => Ok(FieldValue::Date(None)),
            serde_json::Value::String(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| FieldValue::Date(Some(d)))
                .map_err(|_| mismatch(field, "an ISO date (YYYY-MM-DD)")),
            _ => Err(mismatch(field, "an ISO date (YYYY-MM-DD)")),
        },
        FieldType::DateTime => match raw {
            serde_json::Value::Null => Ok(FieldValue::DateTime(None)),
            serde_json::Value::String(s) => DateTime::parse_from_rfc3339(s)
                .map(|dt| FieldValue::DateTime(Some(dt)))
                .map_err(|_| mismatch(field, "an RFC 3339 date-time")),
            _ => Err(mismatch(field, "an RFC 3339 date-time")),
        },
        FieldType::Image => match raw {
            serde_json::Value::Null => Ok(FieldValue::Image(None)),
            serde_json::Value::Number(n) => n
                .as_u64()
                .map(|id| FieldValue::Image(Some(ImageId::from_u64(id))))
                .ok_or_else(|| mismatch(field, "an image id (unsigned integer)")),
            // Responses carry `{ "id": n, "url": "..." }`; accepting that shape back means
            // a client can load an item, change one field and save the rest untouched.
            serde_json::Value::Object(map) => map
                .get("id")
                .and_then(|id| id.as_u64())
                .map(|id| FieldValue::Image(Some(ImageId::from_u64(id))))
                .ok_or_else(|| mismatch(field, "an image id")),
            _ => Err(mismatch(field, "an image id (unsigned integer)")),
        },
        FieldType::TextEnum(options) => {
            let items = raw
                .as_array()
                .ok_or_else(|| mismatch(field, "an array of strings"))?;
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                let value = item
                    .as_str()
                    .ok_or_else(|| mismatch(field, "an array of strings"))?;
                if !options.iter().any(|option| option == value) {
                    return Err(format!(
                        "Field '{}': '{value}' is not one of the allowed options",
                        field.name
                    ));
                }
                values.push(value.to_string());
            }
            Ok(FieldValue::TextEnum(values))
        }
        // One reference, or none: the wire says the cardinality the way the Rust type does, so a
        // list here is a client sending the shape several references used to have.
        FieldType::Relation(options) => match raw {
            serde_json::Value::Null => Ok(FieldValue::Relation(None)),
            serde_json::Value::Object(_) => Ok(FieldValue::Relation(Some(parse_reference(
                field, options, raw,
            )?))),
            _ => Err(mismatch(field, "one { target, item } reference, or null")),
        },
        // Handled by `parse_field_value` before reaching here.
        FieldType::Array(_) | FieldType::CompositeField(_) => {
            Err(mismatch(field, "a scalar value"))
        }
    }
}

/// Array elements are untyped too, so the declared element types are tried in order and
/// the first one that accepts the JSON value wins. This keeps interpretation
/// deterministic given the schema.
///
/// A composite element is an object, and reads wrap it as `{"id": …, "values": {…}}`. When
/// that wrapper names a definition, only the candidate with that id is tried: two composites
/// can accept the same object of sub-values, and the id is what says which one this is.
fn parse_array_element(
    field: &FieldSchema,
    allowed: &[FieldType],
    index: usize,
    raw: &serde_json::Value,
    composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
) -> Result<FieldValue, String> {
    // `CompositeFieldId` is a named string, so the comparison is on the string it holds.
    let declared_id = raw.get("id").and_then(|id| id.as_str());
    for candidate in allowed {
        if let (Some(id), FieldType::CompositeField(reference)) = (declared_id, candidate)
            && reference.id.as_str() != id
        {
            continue;
        }
        let probe = FieldSchema {
            is_title: false,
            show_in_list: false,
            name: format!("{}[{index}]", field.name),
            field_type: candidate.clone(),
            required: false,
            width: 0,
            height: 0,
            unique: false,
        };
        if let Ok(value) = parse_field_value(&probe, raw, composite_schemas) {
            return Ok(value);
        }
    }
    Err(format!(
        "Field '{}[{index}]': value does not match any declared array item type",
        field.name
    ))
}

/// One reference off the wire: `{ "target": "…", "item": n }`, checked against the target the
/// schema (or the array item type) declares.
///
/// A page's identity is its name, so a page reference carries no `item` - and one sent anyway is a
/// value the schema has nowhere to keep.
fn parse_reference(
    field: &FieldSchema,
    options: &RelationOptions,
    raw: &serde_json::Value,
) -> Result<RelationRef, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| mismatch(field, "a { target, item } reference"))?;
    let target = object
        .get("target")
        .and_then(|target| target.as_str())
        .ok_or_else(|| mismatch(field, "a reference with a target name"))?;
    if target != options.target.name() {
        return Err(format!(
            "Field '{}': this field points at '{}', but the value names '{target}'",
            field.name,
            options.target.name()
        ));
    }
    let item = match options.target {
        RelationTarget::SinglePage { .. } => match object.get("item") {
            None | Some(serde_json::Value::Null) => None,
            Some(_) => {
                return Err(mismatch(
                    field,
                    "a reference to a single page, which has no item id",
                ));
            }
        },
        RelationTarget::Collection { .. } => Some(
            object
                .get("item")
                .and_then(|item| item.as_u64())
                .ok_or_else(|| mismatch(field, "an item id (unsigned integer)"))?,
        ),
    };
    Ok(RelationRef {
        target: target.to_string(),
        item,
    })
}

fn mismatch(field: &FieldSchema, expected: &str) -> String {
    format!("Field '{}': expected {expected}", field.name)
}

#[cfg(test)]
mod untyped_parsing_tests {
    use super::*;
    use crate::models::collection::CollectionSchema;
    use serde_json::json;

    fn field(name: &str, field_type: FieldType, required: bool) -> FieldSchema {
        FieldSchema {
            is_title: false,
            show_in_list: false,
            name: name.to_string(),
            field_type,
            required,
            width: 12,
            height: 1,
            unique: false,
        }
    }

    fn schema() -> CollectionSchema {
        vec![
            field("title", FieldType::Text(TextFieldOptions::default()), true),
            field(
                "body",
                FieldType::Markdown(TextFieldOptions::default()),
                false,
            ),
            field("count", FieldType::Number, false),
            field("live", FieldType::Boolean, false),
            field("published", FieldType::Date, false),
            field("at", FieldType::DateTime, false),
            field("cover", FieldType::Image, false),
            field(
                "tags",
                FieldType::TextEnum(vec!["news".into(), "blog".into()]),
                false,
            ),
            field("scores", FieldType::Array(vec![FieldType::Number]), false),
        ]
    }

    fn parse(value: serde_json::Value) -> Result<FieldValueMap<CollectionSchema>, String> {
        FieldValueMap::from_untyped(&value, &HashMap::new(), &schema())
    }

    #[test]
    fn parses_every_scalar_type_without_tags() {
        let parsed = parse(json!({
            "title": "Hello",
            "body": "**bold**",
            "count": 5,
            "live": true,
            "published": "2024-03-01",
            "at": "2024-03-01T10:00:00+00:00",
            "cover": 7,
            "tags": ["news"],
            "scores": [1, 2.5]
        }))
        .unwrap();

        assert_eq!(parsed.0["title"], FieldValue::Text("Hello".into()));
        assert_eq!(parsed.0["body"], FieldValue::Markdown("**bold**".into()));
        assert_eq!(parsed.0["count"], FieldValue::Number(Some(5.0)));
        assert_eq!(parsed.0["live"], FieldValue::Boolean(true));
        assert_eq!(
            parsed.0["published"],
            FieldValue::Date(Some(NaiveDate::from_ymd_opt(2024, 3, 1).unwrap()))
        );
        assert_eq!(
            parsed.0["cover"],
            FieldValue::Image(Some(ImageId::from_u64(7)))
        );
        assert_eq!(parsed.0["tags"], FieldValue::TextEnum(vec!["news".into()]));
        assert_eq!(
            parsed.0["scores"],
            FieldValue::Array(vec![
                FieldValue::Number(Some(1.0)),
                FieldValue::Number(Some(2.5))
            ])
        );
    }

    #[test]
    fn explicit_null_means_none_for_optional_types() {
        let parsed =
            parse(json!({ "title": "t", "count": null, "published": null, "cover": null }))
                .unwrap();
        assert_eq!(parsed.0["count"], FieldValue::Number(None));
        assert_eq!(parsed.0["published"], FieldValue::Date(None));
        assert_eq!(parsed.0["cover"], FieldValue::Image(None));
    }

    #[test]
    fn image_accepts_the_response_shape_so_items_round_trip() {
        // Reads return `{id, url}`; a client that loads an item and saves it back must not
        // be rejected for echoing the untouched image field.
        let parsed = parse(json!({
            "title": "t",
            "cover": { "id": 7, "url": "/images/abc.png" }
        }))
        .unwrap();
        assert_eq!(
            parsed.0["cover"],
            FieldValue::Image(Some(ImageId::from_u64(7)))
        );

        // An object without a usable id is still an error.
        assert!(parse(json!({ "title": "t", "cover": { "url": "/images/abc.png" } })).is_err());
    }

    #[test]
    fn omitted_fields_are_absent_rather_than_defaulted() {
        // Validation decides whether a missing field is acceptable; parsing must not
        // invent a value.
        let parsed = parse(json!({ "title": "t" })).unwrap();
        assert_eq!(parsed.0.len(), 1);
        assert!(!parsed.0.contains_key("count"));
    }

    #[test]
    fn rejects_values_of_the_wrong_type() {
        // A string where a number is expected must not be coerced.
        let err = parse(json!({ "title": "t", "count": "5" })).unwrap_err();
        assert!(err.contains("count"), "unexpected error: {err}");
        assert!(err.contains("number"), "unexpected error: {err}");

        let err = parse(json!({ "title": 42 })).unwrap_err();
        assert!(err.contains("string"), "unexpected error: {err}");

        let err = parse(json!({ "title": "t", "live": "yes" })).unwrap_err();
        assert!(err.contains("boolean"), "unexpected error: {err}");

        let err = parse(json!({ "title": "t", "published": "01/03/2024" })).unwrap_err();
        assert!(err.contains("ISO date"), "unexpected error: {err}");

        let err = parse(json!({ "title": "t", "scores": ["x"] })).unwrap_err();
        assert!(err.contains("array item type"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_unknown_fields_so_typos_are_not_silently_dropped() {
        let err = parse(json!({ "title": "t", "titel": "oops" })).unwrap_err();
        assert!(
            err.contains("Unknown field 'titel'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_enum_values_outside_the_declared_options() {
        let err = parse(json!({ "title": "t", "tags": ["news", "nope"] })).unwrap_err();
        assert!(
            err.contains("not one of the allowed options"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_a_body_that_is_not_an_object() {
        assert!(parse(json!([1, 2, 3])).is_err());
        assert!(parse(json!("just a string")).is_err());
        assert!(parse(json!(null)).is_err());
    }

    #[test]
    fn parses_nested_composite_fields() {
        let composite_id = CompositeFieldId::from("seo");
        let composite_schema: CompositeFieldSchema = vec![field(
            "description",
            FieldType::Text(TextFieldOptions::default()),
            false,
        )];
        let mut schemas = HashMap::new();
        schemas.insert(composite_id.clone(), composite_schema);

        let outer: CollectionSchema = vec![field(
            "seo",
            FieldType::CompositeField(CompositeFieldReference {
                id: composite_id.clone(),
            }),
            false,
        )];

        let parsed = FieldValueMap::from_untyped(
            &json!({ "seo": { "description": "nested value" } }),
            &schemas,
            &outer,
        )
        .unwrap();

        match &parsed.0["seo"] {
            FieldValue::CompositeField(Some(value)) => {
                assert_eq!(value.id, composite_id);
                assert_eq!(
                    value.values.0["description"],
                    FieldValue::Text("nested value".into())
                );
            }
            other => panic!("expected a composite value, got {other:?}"),
        }

        // A null composite is allowed; an unknown composite id is not.
        assert_eq!(
            FieldValueMap::from_untyped(&json!({ "seo": null }), &schemas, &outer)
                .unwrap()
                .0["seo"],
            FieldValue::CompositeField(None)
        );
        let unknown: CollectionSchema = vec![field(
            "seo",
            FieldType::CompositeField(CompositeFieldReference {
                id: CompositeFieldId::from("missing"),
            }),
            false,
        )];
        assert!(FieldValueMap::from_untyped(&json!({ "seo": {} }), &schemas, &unknown).is_err());
    }

    /// A relation: one reference, or - with `multiple` - an array of them, which is how several
    /// are declared (see `docs/relations-design.md` §3).
    fn relation_field(name: &str, target: RelationTarget, multiple: bool) -> FieldSchema {
        let relation = FieldType::Relation(RelationOptions {
            target,
            inverse_name: None,
        });
        field(
            name,
            if multiple {
                FieldType::Array(vec![relation])
            } else {
                relation
            },
            false,
        )
    }

    fn author_target() -> RelationTarget {
        RelationTarget::Collection {
            name: "authors".to_string(),
        }
    }

    #[test]
    fn an_array_of_relations_keeps_the_order_it_was_written_in() {
        let schema = vec![relation_field("author", author_target(), true)];

        // The order is the editor's, not the id order; the same reference written twice is written
        // once, and it stays where it first appeared.
        let parsed = FieldValueMap::from_untyped(
            &json!({ "author": [
                { "target": "authors", "item": 3 },
                { "target": "authors", "item": 1 },
                { "target": "authors", "item": 3 },
            ]}),
            &HashMap::new(),
            &schema,
        )
        .unwrap();
        assert_eq!(
            parsed.0["author"],
            FieldValue::Array(vec![
                FieldValue::Relation(Some(RelationRef {
                    target: "authors".to_string(),
                    item: Some(3),
                })),
                FieldValue::Relation(Some(RelationRef {
                    target: "authors".to_string(),
                    item: Some(1),
                })),
            ])
        );

        // Several references are an array, so "none" is the empty array - while a field nobody sent
        // at all is simply absent (see the omitted-fields test).
        assert_eq!(
            FieldValueMap::from_untyped(&json!({ "author": [] }), &HashMap::new(), &schema)
                .unwrap()
                .0["author"],
            FieldValue::Array(vec![])
        );
        assert!(
            !FieldValueMap::from_untyped(&json!({}), &HashMap::new(), &schema)
                .unwrap()
                .0
                .contains_key("author")
        );
    }

    #[test]
    fn a_reference_to_a_page_carries_no_item_id() {
        let page = RelationTarget::SinglePage {
            name: "home".to_string(),
        };
        let schema = vec![relation_field("landing", page, false)];

        // Absent or null, the value is the same: a page is named, not numbered.
        for value in [
            json!({ "target": "home" }),
            json!({ "target": "home", "item": null }),
        ] {
            assert_eq!(
                FieldValueMap::from_untyped(&json!({ "landing": value }), &HashMap::new(), &schema)
                    .unwrap()
                    .0["landing"],
                FieldValue::Relation(Some(RelationRef {
                    target: "home".to_string(),
                    item: None,
                }))
            );
        }

        // An id for a page names something the schema has nowhere to keep.
        assert!(
            FieldValueMap::from_untyped(
                &json!({ "landing": { "target": "home", "item": 1 } }),
                &HashMap::new(),
                &schema
            )
            .is_err()
        );
    }

    #[test]
    fn a_relation_value_has_to_match_the_field_holding_it() {
        let one = vec![relation_field("author", author_target(), false)];
        let many = vec![relation_field("authors", author_target(), true)];

        // The field points at one collection, so a reference naming another is refused.
        assert!(
            FieldValueMap::from_untyped(
                &json!({ "author": [{ "target": "writers", "item": 1 }] }),
                &HashMap::new(),
                &one
            )
            .is_err()
        );
        // A collection reference needs the id of the item it points at.
        assert!(
            FieldValueMap::from_untyped(
                &json!({ "author": [{ "target": "authors" }] }),
                &HashMap::new(),
                &one
            )
            .is_err()
        );
        // A single reference holds one, not two; several is what an array of relations is for.
        assert!(
            FieldValueMap::from_untyped(
                &json!({ "author": [
                    { "target": "authors", "item": 1 },
                    { "target": "authors", "item": 2 },
                ]}),
                &HashMap::new(),
                &one
            )
            .is_err()
        );
        assert!(
            FieldValueMap::from_untyped(
                &json!({ "authors": [
                    { "target": "authors", "item": 1 },
                    { "target": "authors", "item": 2 },
                ]}),
                &HashMap::new(),
                &many
            )
            .is_ok()
        );
        // A relation is a list of references, not a scalar or a list of lists.
        assert!(
            FieldValueMap::from_untyped(&json!({ "author": 3 }), &HashMap::new(), &one).is_err()
        );
        assert!(
            FieldValueMap::from_untyped(&json!({ "author": [[]] }), &HashMap::new(), &one).is_err()
        );
    }

    /// The index's view of a value: which pieces of content it names, and what is left when one of
    /// them is detached.
    #[test]
    fn the_index_sees_what_the_values_reference() {
        let schema = vec![
            relation_field("author", author_target(), false),
            relation_field("also", author_target(), true),
            field("title", FieldType::Text(TextFieldOptions::default()), false),
        ];
        let parsed = FieldValueMap::from_untyped(
            &json!({
                "title": "Hello",
                "author": { "target": "authors", "item": 1 },
                "also": [
                    { "target": "authors", "item": 2 },
                    { "target": "authors", "item": 1 },
                ],
            }),
            &HashMap::new(),
            &schema,
        )
        .unwrap();

        // One entry per referenced piece of content, whichever field named it and however often.
        assert_eq!(
            referenced_items(&parsed),
            vec![
                ItemOwner::collection_item("authors", 1),
                ItemOwner::collection_item("authors", 2),
            ]
        );

        // Detaching one is dropping that reference and leaving everything else alone.
        let stripped = parsed.without_reference(&ItemOwner::collection_item("authors", 1));
        assert_eq!(
            referenced_items(&stripped),
            vec![ItemOwner::collection_item("authors", 2)]
        );
        assert_eq!(stripped.0["title"], FieldValue::Text("Hello".to_string()));
        assert_eq!(stripped.0["author"], FieldValue::Relation(None));
        assert_eq!(
            stripped.0["also"],
            FieldValue::Array(vec![FieldValue::Relation(Some(RelationRef {
                target: "authors".to_string(),
                item: Some(2),
            }))])
        );

        // A value that does not name the target is not rewritten at all.
        assert_eq!(
            stripped
                .without_reference(&ItemOwner::single_page("home"))
                .0,
            stripped.0
        );

        // A page reference is a different target from a collection of the same name.
        let page = FieldValueMap::from_untyped(
            &json!({ "author": { "target": "authors", "item": 1 } }),
            &HashMap::new(),
            &schema,
        )
        .unwrap();
        assert_eq!(
            page.without_reference(&ItemOwner::single_page("authors")).0,
            page.0
        );

        // And nothing at all references nothing at all.
        assert!(referenced_items(&FieldValueMap::<Vec<FieldSchema>>::default()).is_empty());
    }

    /// The same, for references the schema puts inside a composite definition: the index has to
    /// see them wherever they sit, and detaching has to reach them there.
    #[test]
    fn the_index_sees_a_reference_inside_a_composite() {
        let composite_id = CompositeFieldId::from("cta");
        let mut composites = HashMap::new();
        composites.insert(
            composite_id.clone(),
            vec![
                field("label", FieldType::Text(TextFieldOptions::default()), false),
                relation_field("author", author_target(), false),
            ],
        );
        let schema = vec![
            field(
                "cta",
                FieldType::CompositeField(CompositeFieldReference {
                    id: composite_id.clone(),
                }),
                false,
            ),
            // An array of composites: one relation per element, each its own reference.
            field(
                "blocks",
                FieldType::Array(vec![FieldType::CompositeField(CompositeFieldReference {
                    id: composite_id,
                })]),
                false,
            ),
        ];
        let parsed = FieldValueMap::from_untyped(
            &json!({
                "cta": { "label": "read this", "author": { "target": "authors", "item": 1 } },
                "blocks": [
                    { "label": "first", "author": { "target": "authors", "item": 2 } },
                    { "label": "second", "author": null },
                ],
            }),
            &composites,
            &schema,
        )
        .unwrap();

        assert_eq!(
            referenced_items(&parsed),
            vec![
                ItemOwner::collection_item("authors", 1),
                ItemOwner::collection_item("authors", 2),
            ]
        );

        // Detaching one reaches into the composite and leaves everything else as it was.
        let stripped = parsed.without_reference(&ItemOwner::collection_item("authors", 1));
        assert_eq!(
            referenced_items(&stripped),
            vec![ItemOwner::collection_item("authors", 2)]
        );
        let FieldValue::CompositeField(Some(cta)) = &stripped.0["cta"] else {
            panic!("expected a composite value");
        };
        assert_eq!(cta.values.0["author"], FieldValue::Relation(None));
        assert_eq!(
            cta.values.0["label"],
            FieldValue::Text("read this".to_string())
        );

        // And into an array of them, one element at a time.
        let FieldValue::Array(blocks) = &stripped.0["blocks"] else {
            panic!("expected an array value");
        };
        let FieldValue::CompositeField(Some(first)) = &blocks[0] else {
            panic!("expected a composite value");
        };
        assert_eq!(
            first.values.0["label"],
            FieldValue::Text("first".to_string())
        );
        let FieldValue::CompositeField(Some(second)) = &blocks[1] else {
            panic!("expected a composite value");
        };
        assert_eq!(
            second.values.0["label"],
            FieldValue::Text("second".to_string())
        );
    }

    #[test]
    fn a_required_relation_needs_at_least_one_reference() {
        let required = |multiple| {
            let mut field = relation_field("author", author_target(), multiple);
            field.required = true;
            field
        };

        // A required relation says something whether it holds one reference or an array of them.
        for multiple in [false, true] {
            let schema = required(multiple);
            let reference = RelationRef {
                target: "authors".to_string(),
                item: Some(1),
            };
            // Empty is `null` for one reference and `[]` for an array of them; both are "nothing",
            // and a required field is refused either way.
            let empty = if multiple {
                FieldValue::Array(vec![])
            } else {
                FieldValue::Relation(None)
            };
            assert_eq!(
                empty
                    .validate_field_value(&schema, &HashMap::new())
                    .unwrap_err()
                    .code,
                "field_required"
            );
            let filled = if multiple {
                FieldValue::Array(vec![FieldValue::Relation(Some(reference))])
            } else {
                FieldValue::Relation(Some(reference))
            };
            assert!(
                filled
                    .validate_field_value(&schema, &HashMap::new())
                    .is_ok()
            );
        }
    }

    #[test]
    fn composite_values_accept_the_response_wrapper_so_items_round_trip() {
        let composite_id = CompositeFieldId::from("seo");
        let composite_schema: CompositeFieldSchema = vec![field(
            "description",
            FieldType::Text(TextFieldOptions::default()),
            false,
        )];
        let mut schemas = HashMap::new();
        schemas.insert(composite_id.clone(), composite_schema);

        let outer: CollectionSchema = vec![field(
            "seo",
            FieldType::CompositeField(CompositeFieldReference {
                id: composite_id.clone(),
            }),
            false,
        )];

        // The bare object is what a write normally sends.
        let bare = FieldValueMap::from_untyped(
            &json!({ "seo": { "description": "nested" } }),
            &schemas,
            &outer,
        )
        .unwrap();

        // The wrapper is what a read returns.
        let wrapped = FieldValueMap::from_untyped(
            &json!({ "seo": { "id": "seo", "values": { "description": "nested" } } }),
            &schemas,
            &outer,
        )
        .unwrap();

        assert_eq!(bare.0["seo"], wrapped.0["seo"]);
    }

    #[test]
    fn a_composite_that_declares_a_values_field_is_not_unwrapped() {
        // `values` is a legitimate sub-field name here, so an object with that key is the
        // composite's own content rather than a response wrapper.
        let composite_id = CompositeFieldId::from("wrapper");
        let composite_schema: CompositeFieldSchema =
            vec![field("values", FieldType::Number, false)];
        let mut schemas = HashMap::new();
        schemas.insert(composite_id.clone(), composite_schema);

        let outer: CollectionSchema = vec![field(
            "wrapped",
            FieldType::CompositeField(CompositeFieldReference {
                id: composite_id.clone(),
            }),
            false,
        )];

        let parsed =
            FieldValueMap::from_untyped(&json!({ "wrapped": { "values": 3 } }), &schemas, &outer)
                .unwrap();

        match &parsed.0["wrapped"] {
            FieldValue::CompositeField(Some(value)) => {
                assert_eq!(value.values.0["values"], FieldValue::Number(Some(3.0)));
            }
            other => panic!("expected a composite value, got {other:?}"),
        }
    }

    #[test]
    fn array_elements_pick_the_first_declared_type_that_fits() {
        let mixed: CollectionSchema = vec![field(
            "values",
            FieldType::Array(vec![FieldType::Boolean, FieldType::Number]),
            false,
        )];
        let parsed =
            FieldValueMap::from_untyped(&json!({ "values": [true, 3] }), &HashMap::new(), &mixed)
                .unwrap();
        assert_eq!(
            parsed.0["values"],
            FieldValue::Array(vec![
                FieldValue::Boolean(true),
                FieldValue::Number(Some(3.0))
            ])
        );
    }
}

impl<T> std::ops::Deref for FieldValueMap<T> {
    type Target = HashMap<String, FieldValue>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> Serialize for FieldValueMap<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}
impl<'de, T> Deserialize<'de> for FieldValueMap<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let map = HashMap::<String, FieldValue>::deserialize(deserializer)?;
        Ok(FieldValueMap(map, PhantomData))
    }
}
impl<T> PartialEq for FieldValueMap<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(EnumIter))]
pub enum FieldValue {
    Text(String),
    Markdown(String),
    Number(Option<f64>),
    Boolean(bool),
    Date(Option<NaiveDate>),
    DateTime(Option<DateTime<FixedOffset>>),
    Image(Option<ImageId>),
    CompositeField(Option<CompositeFieldValue>),
    /// A reference to another collection's item, or to a single page: one, or none.
    ///
    /// Several references are an `Array` of these (see `docs/relations-design.md` §3): the element
    /// carries its own target, which is what lets one array name more than one.
    Relation(Option<RelationRef>),
    Array(Vec<FieldValue>),
    TextEnum(Vec<String>),
}

/// One line of a relation's value: what it points at, and which item.
///
/// The *kind* of target lives in the schema, not here, so a collection reference carries the item
/// id and a single-page reference does not: a page's identity is its name. Which shape is allowed
/// is what `from_untyped` checks the field's own target against.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RelationRef {
    /// The collection or single page the related item belongs to.
    pub target: String,
    /// The item's id. Absent for a single page, which has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
}

impl RelationRef {
    /// What this reference points at, named the way an index names content.
    ///
    /// An absent id is what says "a single page": the schema has to agree (the value is checked
    /// against it when it is read), and the two shapes are exactly the two kinds of content.
    pub fn target_owner(&self) -> ItemOwner {
        match self.item {
            Some(item) => ItemOwner::collection_item(&self.target, item),
            None => ItemOwner::single_page(&self.target),
        }
    }
}

impl<T> FieldValueMap<T> {
    /// The same values with one reference dropped from every relation field.
    ///
    /// What detaching does to the content on the other side: the reference goes and everything else
    /// is left as it was. A relation that ends up empty stays an empty set, which is what "no
    /// references" is, and a field that does not hold the target is not rewritten at all.
    ///
    /// The whole value is walked, composites and arrays included: detaching is what makes a delete
    /// possible, and a reference the walk misses would refuse that delete for ever.
    pub fn without_reference(&self, target: &ItemOwner) -> Self {
        let mut values = self.0.clone();
        for value in values.values_mut() {
            drop_reference(value, target);
        }
        FieldValueMap(values, PhantomData)
    }
}

/// Drop one reference from a value, wherever it sits.
fn drop_reference(value: &mut FieldValue, target: &ItemOwner) {
    match value {
        FieldValue::Relation(reference) => {
            // One reference: detaching it leaves the field holding nothing, which is `None`.
            if reference
                .as_ref()
                .is_some_and(|reference| &reference.target_owner() == target)
            {
                *reference = None;
            }
        }
        FieldValue::CompositeField(Some(composite)) => {
            for nested in composite.values.0.values_mut() {
                drop_reference(nested, target);
            }
        }
        FieldValue::Array(items) => {
            // An array of relations is a list of references: detaching one takes it out rather than
            // leaving a blank element where it was. A composite element that holds it is kept, and
            // the recursion below is what empties it.
            items.retain(|item| match item {
                FieldValue::Relation(Some(reference)) => &reference.target_owner() != target,
                _ => true,
            });
            for item in items.iter_mut() {
                drop_reference(item, target);
            }
        }
        _ => {}
    }
}

/// One relation field of a value, with the path that names it.
///
/// The path is what a refusal can name (`author`, `cta.author`, `blocks[0].author`), so an editor
/// knows which input to open even when the field is nested in a composite.
pub struct RelationValue<'a> {
    /// Where the field sits in the value.
    pub path: String,
    /// Whether the schema asks for this field, which is what the publish-time rules turn on.
    pub required: bool,
    /// The references the value holds, in the canonical order they were stored in: one for a
    /// `Relation`, several for an array of them. Each carries its own target, because an array may
    /// name more than one.
    pub references: Vec<&'a RelationRef>,
}

/// Every relation a value holds, with the schema that declares it.
///
/// Composites and arrays are walked: a relation may sit inside either (see
/// `docs/relations-design.md` §3), and a rule that only looked at the top level would let a required
/// relation inside a block go live pointing at nothing.
///
/// A field the value does not hold is not reported: whether a missing field is a problem is
/// `validate_to_schema`'s question, and it has an answer about the field rather than about a
/// target.
pub fn relation_values<'a>(
    schema: &'a [FieldSchema],
    values: &'a FieldValueMap<Vec<FieldSchema>>,
    composites: &'a HashMap<CompositeFieldId, CompositeFieldSchema>,
) -> Vec<RelationValue<'a>> {
    let mut found = Vec::new();
    collect_relation_values(schema, values, composites, "", &mut found);
    found
}

fn collect_relation_values<'a>(
    schema: &'a [FieldSchema],
    values: &'a FieldValueMap<Vec<FieldSchema>>,
    composites: &'a HashMap<CompositeFieldId, CompositeFieldSchema>,
    prefix: &str,
    found: &mut Vec<RelationValue<'a>>,
) {
    for field in schema {
        let path = format!("{prefix}{}", field.name);
        match &field.field_type {
            FieldType::Relation(_) => {
                if let Some(FieldValue::Relation(Some(reference))) = values.0.get(&field.name) {
                    found.push(RelationValue {
                        path,
                        required: field.required,
                        references: vec![reference],
                    });
                }
            }
            FieldType::CompositeField(reference) => {
                let (Some(definition), Some(FieldValue::CompositeField(Some(composite)))) =
                    (composites.get(&reference.id), values.0.get(&field.name))
                else {
                    continue;
                };
                collect_relation_values(
                    definition,
                    &composite.values,
                    composites,
                    &format!("{path}."),
                    found,
                );
            }
            FieldType::Array(items) => {
                let Some(FieldValue::Array(elements)) = values.0.get(&field.name) else {
                    continue;
                };
                // An array of relations *is* several references, and they are one field's: the rule
                // is "this field has no published target left", which a partially published array
                // passes (see `docs/relations-design.md` §4). Its elements may name different
                // targets, so the set is what is gathered, not each element on its own.
                let references: Vec<&RelationRef> = elements
                    .iter()
                    .filter_map(|element| match element {
                        FieldValue::Relation(Some(reference)) => Some(reference),
                        _ => None,
                    })
                    .collect();
                if !references.is_empty() {
                    found.push(RelationValue {
                        path: path.clone(),
                        required: field.required,
                        references,
                    });
                }
                for (index, element) in elements.iter().enumerate() {
                    let FieldValue::CompositeField(Some(composite)) = element else {
                        continue;
                    };
                    let definition = items.iter().find_map(|item| match item {
                        FieldType::CompositeField(reference) if reference.id == composite.id => {
                            composites.get(&reference.id)
                        }
                        _ => None,
                    });
                    let Some(definition) = definition else {
                        continue;
                    };
                    collect_relation_values(
                        definition,
                        &composite.values,
                        composites,
                        &format!("{path}[{index}]."),
                        found,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Every piece of content a set of values references.
///
/// The whole value is walked, composites and arrays included: a relation may sit in either, and
/// what is stored is what the index has to agree with. That also means a value left behind by a
/// field the schema no longer declares is still found here, and is still a reason not to delete
/// what it points at.
///
/// The walk terminates on any value: a schema cannot reach itself except through an array, whose
/// elements come from the value, and a value is finite.
pub fn referenced_items<T>(item: &FieldValueMap<T>) -> Vec<ItemOwner> {
    let mut found = std::collections::BTreeSet::new();
    for value in item.0.values() {
        collect_referenced_items(value, &mut found);
    }
    found.into_iter().collect()
}

/// Add every item a value points at, wherever the references in it sit.
fn collect_referenced_items(value: &FieldValue, found: &mut std::collections::BTreeSet<ItemOwner>) {
    match value {
        FieldValue::Relation(Some(reference)) => {
            found.insert(reference.target_owner());
        }
        FieldValue::Relation(None) => {}
        FieldValue::CompositeField(Some(composite)) => {
            for nested in composite.values.0.values() {
                collect_referenced_items(nested, found);
            }
        }
        FieldValue::Array(items) => {
            for item in items {
                collect_referenced_items(item, found);
            }
        }
        _ => {}
    }
}

// API レスポンス用の型
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum FieldValueResponse {
    Text(String),
    Markdown(String),
    Number(Option<f64>),
    Boolean(bool),
    Date(Option<NaiveDate>),
    DateTime(Option<DateTime<FixedOffset>>),
    Image(Option<ImageResponse>),
    CompositeField(Option<CompositeFieldValueResponse>),
    Relation(Option<RelationRef>),
    Array(Vec<FieldValueResponse>),
    TextEnum(Vec<String>),
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CompositeFieldValueResponse {
    pub id: CompositeFieldId,
    pub values: HashMap<String, FieldValueResponse>,
}
impl Default for FieldValue {
    fn default() -> Self {
        FieldValue::Text(String::new())
    }
}
impl FieldValue {
    pub fn to_response(&self, images: &HashMap<ImageId, Image>) -> FieldValueResponse {
        match self {
            FieldValue::Text(s) => FieldValueResponse::Text(s.clone()),
            FieldValue::Markdown(s) => FieldValueResponse::Markdown(s.clone()),
            FieldValue::Number(n) => FieldValueResponse::Number(*n),
            FieldValue::Boolean(b) => FieldValueResponse::Boolean(*b),
            FieldValue::Date(d) => FieldValueResponse::Date(*d),
            FieldValue::DateTime(dt) => FieldValueResponse::DateTime(*dt),
            FieldValue::Image(img_id) => {
                let img_response = img_id.as_ref().and_then(|id| {
                    images.get(id).map(|img| ImageResponse {
                        id: *id,
                        url: img.url.clone(),
                    })
                });
                FieldValueResponse::Image(img_response)
            }
            FieldValue::CompositeField(cv) => {
                FieldValueResponse::CompositeField(cv.as_ref().map(|v| {
                    CompositeFieldValueResponse {
                        id: v.id.clone(),
                        values: v
                            .values
                            .0
                            .iter()
                            .map(|(k, val)| (k.clone(), val.to_response(images)))
                            .collect(),
                    }
                }))
            }
            FieldValue::Array(arr) => {
                FieldValueResponse::Array(arr.iter().map(|v| v.to_response(images)).collect())
            }
            // Nothing is resolved here yet: the reference is answered as it is stored. Expansion
            // is a delivery-api decision (`?populate=`), taken in phase 4 of the design.
            FieldValue::Relation(refs) => FieldValueResponse::Relation(refs.clone()),
            FieldValue::TextEnum(vals) => FieldValueResponse::TextEnum(vals.clone()),
        }
    }
}

impl FieldValue {
    /// Whether `field_type` is the kind of field this value belongs to.
    ///
    /// The dispatch is on the **field type**, with every variant named, so that adding one is a
    /// compile error here rather than a value that quietly falls through a catch-all. `get_type`
    /// below is the same guard on the other axis: it names every `FieldValue`, so a new *value* kind
    /// is caught there (and by `to_response`). A value of the wrong kind is `false`, which is what
    /// the `matches!` inside each arm is for.
    pub fn matches_type(&self, field_type: &FieldType) -> bool {
        match field_type {
            FieldType::Text(_) | FieldType::Slug(_) => matches!(self, FieldValue::Text(_)),
            FieldType::Markdown(_) => matches!(self, FieldValue::Markdown(_)),
            FieldType::Number => matches!(self, FieldValue::Number(_)),
            FieldType::Boolean => matches!(self, FieldValue::Boolean(_)),
            FieldType::Date => matches!(self, FieldValue::Date(_)),
            FieldType::DateTime => matches!(self, FieldValue::DateTime(_)),
            FieldType::Image => matches!(self, FieldValue::Image(_)),
            FieldType::CompositeField(_) => matches!(self, FieldValue::CompositeField(_)),
            FieldType::Relation(_) => matches!(self, FieldValue::Relation(_)),
            FieldType::Array(_) => matches!(self, FieldValue::Array(_)),
            FieldType::TextEnum(_) => matches!(self, FieldValue::TextEnum(_)),
        }
    }

    /// The field type a value of this kind answers to, as something to talk *about* rather than a
    /// check to make.
    ///
    /// It is lossy on purpose, and nothing in the server asks it any more: a text value does not
    /// remember whether it was written for a `Text` or a `Slug` field (the schema is where that
    /// lives), and the options come back empty. Asking a question about a value and a field is
    /// `matches_type`'s job. This stays because it describes a value's kind, and because it names
    /// every `FieldValue` - which is half of what makes a new value kind a compile error.
    pub fn get_type(&self) -> FieldType {
        match self {
            FieldValue::Text(_) => FieldType::Text(TextFieldOptions::default()),
            FieldValue::Markdown(_) => FieldType::Markdown(TextFieldOptions::default()),
            FieldValue::Number(_) => FieldType::Number,
            FieldValue::Boolean(_) => FieldType::Boolean,
            FieldValue::Date(_) => FieldType::Date,
            FieldValue::DateTime(_) => FieldType::DateTime,
            FieldValue::Image(_) => FieldType::Image,
            FieldValue::CompositeField(cv) => match cv {
                Some(cv) => {
                    FieldType::CompositeField(CompositeFieldReference { id: cv.id.clone() })
                }
                None => FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::default(),
                }),
            },
            FieldValue::Array(_values) => FieldType::Array(vec![]),
            // Lossy on purpose, like the two above: a relation value does not carry what it points
            // at, and the schema is where that lives.
            FieldValue::Relation(_) => FieldType::Relation(RelationOptions::default()),
            FieldValue::TextEnum(_vals) => FieldType::TextEnum(vec![]),
        }
    }
    fn extract_text_options<F>(schemas: &[FieldType], matcher: F) -> Option<TextFieldOptions>
    where
        F: Fn(&FieldType) -> Option<&TextFieldOptions>,
    {
        schemas.iter().fold(None, |mut acc, ft| {
            if let Some(options) = matcher(ft) {
                match acc {
                    None => acc = Some(*options),
                    Some(existing_options) => {
                        let merged_options = TextFieldOptions {
                            max_length: match (existing_options.max_length, options.max_length) {
                                (Some(len1), Some(len2)) => Some(len1.max(len2)),
                                (_, None) | (None, _) => None,
                            },
                            min_length: match (existing_options.min_length, options.min_length) {
                                (Some(len1), Some(len2)) => Some(len1.min(len2)),
                                (_, None) | (None, _) => None,
                            },
                            // Multi-line is the looser of the two: an array whose items are edited
                            // in boxes is a box.
                            multiline: existing_options.multiline || options.multiline,
                        };
                        acc = Some(merged_options);
                    }
                }
            }
            acc
        })
    }

    /// Whether this value is one the field's schema accepts.
    ///
    /// `validate_field_at` does the work and knows where the value sits; this is the entry point
    /// for a value at the top level of an item.
    pub fn validate_field_value(
        &self,
        schema: &FieldSchema,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    ) -> Result<(), FieldRefusal> {
        self.validate_field_at(schema, composite_schemas, "")
    }

    fn validate_field_at(
        &self,
        schema: &FieldSchema,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
        prefix: &str,
    ) -> Result<(), FieldRefusal> {
        let path = format!("{prefix}{}", schema.name);
        // The dispatch is on the **field type**, with every variant named, so that adding one is a
        // compile error here rather than a value that quietly falls through a catch-all. Inside an
        // arm, `_` is a value of the wrong kind, and `matches_type` is where the pairs live.
        match &schema.field_type {
            FieldType::Text(params) => match self {
                FieldValue::Text(text) => {
                    if schema.required && !schema.field_type.test_required(self) {
                        return Err(FieldRefusal::required(&path));
                    }
                    schema.validate_text_length(text, params, &path)
                }
                _ => Err(FieldRefusal::type_mismatch(&path)),
            },
            FieldType::Markdown(params) => match self {
                FieldValue::Markdown(text) => {
                    if schema.required && !schema.field_type.test_required(self) {
                        return Err(FieldRefusal::required(&path));
                    }
                    schema.validate_text_length(text, params, &path)
                }
                _ => Err(FieldRefusal::type_mismatch(&path)),
            },
            FieldType::Slug(_) => match self {
                FieldValue::Text(slug) => {
                    if schema.required && !schema.field_type.test_required(self) {
                        return Err(FieldRefusal::required(&path));
                    }
                    if slug.chars().count() > crate::models::slug::SLUG_MAX_LENGTH {
                        return Err(FieldRefusal::too_long(
                            &path,
                            crate::models::slug::SLUG_MAX_LENGTH,
                        ));
                    }
                    // Everything a slug may hold, and nothing else. A value that is not canonical
                    // is refused rather than rewritten here: normalising on this side would quietly
                    // store something other than what was sent, and the caller would never learn.
                    let canonical = slug
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                    if !canonical {
                        return Err(FieldRefusal::invalid_slug(&path));
                    }
                    Ok(())
                }
                _ => Err(FieldRefusal::type_mismatch(&path)),
            },
            FieldType::Number
            | FieldType::Boolean
            | FieldType::Date
            | FieldType::DateTime
            | FieldType::Image
            | FieldType::Relation(_) => {
                if !self.matches_type(&schema.field_type) {
                    return Err(FieldRefusal::type_mismatch(&path));
                }
                if schema.required && !schema.field_type.test_required(self) {
                    return Err(FieldRefusal::required(&path));
                }
                Ok(())
            }
            FieldType::CompositeField(s) => match self {
                FieldValue::CompositeField(field_value) => {
                    if schema.required && !schema.field_type.test_required(self) {
                        return Err(FieldRefusal::required(&path));
                    }
                    if let Some(v) = field_value {
                        if s.id != v.id {
                            return Err(FieldRefusal::composite_mismatch(&path));
                        }
                        let composite_schema = composite_schemas
                            .get(&s.id)
                            .ok_or_else(|| FieldRefusal::unknown_composite(&path, &s.id))?;
                        v.values.validate_at(
                            &format!("{path}."),
                            composite_schemas,
                            composite_schema,
                        )?;
                        Ok(())
                    } else {
                        Ok(())
                    }
                }
                _ => Err(FieldRefusal::type_mismatch(&path)),
            },
            FieldType::Array(schemas) => match self {
                FieldValue::Array(values) => {
                    if schema.required && !schema.field_type.test_required(self) {
                        return Err(FieldRefusal::required(&path));
                    }
                    // An element that holds nothing is a value like any other: `required` asks the
                    // *array* to hold something (see `test_required`, which is `!values.is_empty()`),
                    // and a blank element is what a text element has always been allowed to be -
                    // `[""]` passes. An element used to be refused here as `field_required`, which a
                    // working copy swallows and publishing does not, so a draft could be saved and
                    // never published, on a field that nobody had marked required.
                    for (index, value) in values.iter().enumerate() {
                        // An array item is named by the path plus its index: `tags[2]`.
                        let item = format!("{path}[{index}]");
                        match value {
                            FieldValue::Text(value) => {
                                let option = Self::extract_text_options(schemas, |ft| {
                                    if let FieldType::Text(options) = ft {
                                        Some(options)
                                    } else {
                                        None
                                    }
                                });
                                match option {
                                    Some(options) => {
                                        schema.validate_text_length(value, &options, &item)?;
                                    }
                                    None => {
                                        return Err(FieldRefusal::type_mismatch(&item));
                                    }
                                }
                            }
                            FieldValue::Markdown(value) => {
                                let option = Self::extract_text_options(schemas, |ft| {
                                    if let FieldType::Markdown(options) = ft {
                                        Some(options)
                                    } else {
                                        None
                                    }
                                });
                                match option {
                                    Some(options) => {
                                        schema.validate_text_length(value, &options, &item)?;
                                    }
                                    None => {
                                        return Err(FieldRefusal::type_mismatch(&item));
                                    }
                                }
                            }
                            FieldValue::Number(_)
                            | FieldValue::Boolean(_)
                            | FieldValue::Date(_)
                            | FieldValue::DateTime(_)
                            | FieldValue::Image(_) => {
                                if !schemas
                                    .iter()
                                    .any(|field_type| value.matches_type(field_type))
                                {
                                    return Err(FieldRefusal::type_mismatch(&item));
                                }
                            }
                            FieldValue::CompositeField(cv) => {
                                // A composite element that is null is the same case as a blank
                                // scalar: an empty element, not a missing required field, so there
                                // is nothing to check.
                                if let Some(cv) = cv {
                                    let schema = schemas
                                        .iter()
                                        .find(|ft| match ft {
                                            FieldType::CompositeField(s) => s.id == cv.id,
                                            _ => false,
                                        })
                                        .and_then(|ft| match ft {
                                            FieldType::CompositeField(s) => Some(s),
                                            _ => None,
                                        });
                                    match schema {
                                        Some(s) => {
                                            let composite_schema =
                                                composite_schemas.get(&s.id).ok_or_else(|| {
                                                    FieldRefusal::unknown_composite(&item, &s.id)
                                                })?;

                                            cv.values.validate_at(
                                                &format!("{item}."),
                                                composite_schemas,
                                                composite_schema,
                                            )?;
                                        }
                                        None => {
                                            // The element names a definition this array does not
                                            // declare, so the element itself is the wrong kind.
                                            return Err(FieldRefusal::type_mismatch(&item));
                                        }
                                    }
                                }
                            }
                            FieldValue::Array(_) => {
                                return Err(FieldRefusal::nested_array(&item));
                            }
                            FieldValue::TextEnum(vals) => {
                                let option = schemas.iter().fold(Vec::new(), |mut acc, ft| {
                                    if let FieldType::TextEnum(options) = ft {
                                        acc.extend(options.clone());
                                    }
                                    acc
                                });
                                for val in vals {
                                    if !option.contains(val) {
                                        return Err(FieldRefusal::enum_value(&item, val));
                                    }
                                }
                            }
                            FieldValue::Relation(reference) => {
                                // An element is one reference, and the target it names has to be
                                // one this array declares - which is also how the targets are told
                                // apart, since the element carries it. A blank element holds
                                // nothing, which every array allows.
                                let declared = reference.as_ref().is_none_or(|reference| {
                                    schemas.iter().any(|candidate| match candidate {
                                        FieldType::Relation(options) => {
                                            options.target.name() == reference.target
                                        }
                                        _ => false,
                                    })
                                });
                                if !declared {
                                    return Err(FieldRefusal::type_mismatch(&item));
                                }
                            }
                        }
                    }
                    Ok(())
                }
                _ => Err(FieldRefusal::type_mismatch(&path)),
            },
            FieldType::TextEnum(options) => match self {
                FieldValue::TextEnum(vals) => {
                    for val in vals {
                        if !options.contains(val) {
                            return Err(FieldRefusal::enum_value(&path, val));
                        }
                    }
                    if vals.is_empty() && schema.required {
                        return Err(FieldRefusal::required(&path));
                    }
                    Ok(())
                }
                _ => Err(FieldRefusal::type_mismatch(&path)),
            },
        }
    }
    pub fn format_field_value(
        &self,
        schema: &FieldSchema,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    ) -> FieldValue {
        // The same shape as `validate_field_at`: every field type is named, so a new one cannot be
        // added without deciding how it formats. A value of the wrong kind formats to the field's
        // own default, which is what the pair match used to say.
        match &schema.field_type {
            FieldType::Text(_)
            | FieldType::Slug(_)
            | FieldType::Markdown(_)
            | FieldType::Number
            | FieldType::Boolean
            | FieldType::Date
            | FieldType::DateTime
            | FieldType::Image
            | FieldType::Relation(_) => {
                if self.matches_type(&schema.field_type) {
                    self.clone()
                } else {
                    schema.get_default_value()
                }
            }
            FieldType::CompositeField(s) => match self {
                FieldValue::CompositeField(Some(v)) => {
                    // A composite of another definition is not this field's value: the schema's own
                    // default stands in for it, as for a value that is missing entirely.
                    match composite_schemas.get(&s.id).filter(|_| s.id == v.id) {
                        None => schema.get_default_value(),
                        Some(cs) => {
                            let formatted_values = v.values.format_to_schema(composite_schemas, cs);
                            FieldValue::CompositeField(Some(CompositeFieldValue {
                                id: s.id.clone(),
                                values: formatted_values,
                            }))
                        }
                    }
                }
                _ => schema.get_default_value(),
            },
            FieldType::Array(schemas) => match self {
                FieldValue::Array(values) => {
                    let mut formatted_values = Vec::new();
                    for value in values {
                        match value {
                            FieldValue::Text(_)
                            | FieldValue::Markdown(_)
                            | FieldValue::Number(_)
                            | FieldValue::Boolean(_)
                            | FieldValue::Date(_)
                            | FieldValue::DateTime(_)
                            | FieldValue::Image(_)
                            | FieldValue::Relation(_) => {
                                if schemas
                                    .iter()
                                    .any(|field_type| value.matches_type(field_type))
                                {
                                    formatted_values.push(value.clone());
                                }
                            }
                            FieldValue::CompositeField(cv) => match cv {
                                Some(cv) => {
                                    let s = schemas
                                        .iter()
                                        .find(|ft| match ft {
                                            FieldType::CompositeField(s) => s.id == cv.id,
                                            _ => false,
                                        })
                                        .and_then(|ft| match ft {
                                            FieldType::CompositeField(s) => Some(s),
                                            _ => None,
                                        });
                                    match s {
                                        Some(schema) => {
                                            let composite_schema =
                                                composite_schemas.get(&schema.id);
                                            match composite_schema {
                                                None => continue,
                                                Some(cs) => {
                                                    let formatted_values_map = cv
                                                        .values
                                                        .format_to_schema(composite_schemas, cs);
                                                    formatted_values.push(
                                                        FieldValue::CompositeField(Some(
                                                            CompositeFieldValue {
                                                                id: schema.id.clone(),
                                                                values: formatted_values_map,
                                                            },
                                                        )),
                                                    );
                                                }
                                            };
                                        }
                                        None => continue,
                                    };
                                }
                                None => continue,
                            },
                            FieldValue::Array(_) => {
                                continue;
                            }
                            FieldValue::TextEnum(vals) => {
                                let option = schemas.iter().fold(Vec::new(), |mut acc, ft| {
                                    if let FieldType::TextEnum(options) = ft {
                                        acc.extend(options.clone());
                                    }
                                    acc
                                });
                                let filtered_vals: Vec<String> = vals
                                    .iter()
                                    .filter(|v| option.contains(v))
                                    .cloned()
                                    .collect();
                                formatted_values.push(FieldValue::TextEnum(filtered_vals));
                            }
                        };
                    }
                    FieldValue::Array(formatted_values)
                }
                _ => schema.get_default_value(),
            },
            FieldType::TextEnum(options) => match self {
                FieldValue::TextEnum(vals) => {
                    let filtered_vals: Vec<String> = vals
                        .iter()
                        .filter(|v| options.contains(v))
                        .cloned()
                        .collect();
                    FieldValue::TextEnum(filtered_vals)
                }
                _ => schema.get_default_value(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use strum::IntoEnumIterator;

    // Test helper functions
    fn create_composite_schema(field_name: &str, required: bool) -> CompositeFieldSchema {
        vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: field_name.to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required,
            width: 12,
            height: 1,
            unique: false,
        }]
    }

    fn create_composite_value(id: &str, field_name: &str, value: &str) -> CompositeFieldValue {
        CompositeFieldValue {
            id: (id.into()),
            values: FieldValueMap(
                HashMap::from([(field_name.to_string(), FieldValue::Text(value.to_string()))]),
                PhantomData,
            ),
        }
    }

    fn create_composite_schemas_map() -> HashMap<CompositeFieldId, CompositeFieldSchema> {
        HashMap::from([
            ("comp_1".into(), create_composite_schema("sub_field", true)),
            ("comp_2".into(), create_composite_schema("sub_field2", true)),
        ])
    }

    fn array_field(name: &str, items: Vec<FieldType>, required: bool) -> FieldSchema {
        FieldSchema {
            is_title: false,
            show_in_list: false,
            name: name.to_string(),
            field_type: FieldType::Array(items),
            required,
            width: 12,
            height: 1,
            unique: false,
        }
    }

    #[test]
    fn field_schema_array_validation() {
        let composite_schemas = HashMap::new();
        let field_schema = FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "test_array".to_string(),
            field_type: FieldType::Array(vec![
                FieldType::Text(TextFieldOptions::default()),
                FieldType::Number,
            ]),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        };
        let valid_value = FieldValue::Array(vec![
            FieldValue::Text("Hello".to_string()),
            FieldValue::Number(Some(42.0)),
        ]);
        let invalid_value = FieldValue::Array(vec![FieldValue::Boolean(true)]);
        assert!(
            valid_value
                .validate_field_value(&field_schema, &composite_schemas)
                .is_ok()
        );
        assert!(
            invalid_value
                .validate_field_value(&field_schema, &composite_schemas)
                .is_err()
        );

        let nested_array_value =
            FieldValue::Array(vec![FieldValue::Array(vec![FieldValue::Text(
                "Nested".to_string(),
            )])]);
        assert!(
            nested_array_value
                .validate_field_value(&field_schema, &composite_schemas)
                .is_err()
        );

        let composite_schemas = create_composite_schemas_map();

        let composite_field_schema = FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "test_array".to_string(),
            field_type: FieldType::Array(vec![
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                }),
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_2".into(),
                }),
            ]),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        };
        let valid_composite_value = FieldValue::Array(vec![
            FieldValue::CompositeField(Some(create_composite_value(
                "comp_1",
                "sub_field",
                "Value1",
            ))),
            FieldValue::CompositeField(Some(create_composite_value(
                "comp_2",
                "sub_field2",
                "Value2",
            ))),
        ]);
        assert!(
            valid_composite_value
                .validate_field_value(&composite_field_schema, &composite_schemas)
                .is_ok()
        );

        let text_enum_array_value =
            FieldValue::Array(vec![FieldValue::TextEnum(vec!["Option".to_string()])]);
        assert!(
            text_enum_array_value
                .validate_field_value(&composite_field_schema, &composite_schemas)
                .is_err()
        );
    }

    /// A blank element is a value like any other: `required` asks the array to hold *something*,
    /// and `[""]` in a text array has always passed. `[null]` used to come back as
    /// `field_required`, which a working copy swallows and publishing does not - so a draft saved
    /// and could never be published, on a field nobody had marked required.
    #[test]
    fn a_blank_array_element_is_not_a_missing_required_field() {
        let composite_schemas = create_composite_schemas_map();

        for (name, items, value) in [
            ("scores", vec![FieldType::Number], json!({"scores": [null]})),
            ("cover", vec![FieldType::Image], json!({"cover": [null]})),
            (
                "blocks",
                vec![FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                })],
                json!({"blocks": [null]}),
            ),
        ] {
            let field_schema = array_field(name, items, false);
            let schema = vec![field_schema];
            let values = FieldValueMap::<Vec<FieldSchema>>::from_untyped(
                &value,
                &composite_schemas,
                &schema,
            )
            .unwrap_or_else(|e| panic!("{name}: a blank element has to parse: {e}"));

            assert!(
                values
                    .validate_to_schema(&composite_schemas, &schema)
                    .is_ok(),
                "{name}: publishing accepts a blank element"
            );
            assert!(
                values.validate_draft(&composite_schemas, &schema).is_ok(),
                "{name}: and so does a working copy"
            );
        }

        // What `required` still asks for is that the array holds something at all.
        let schema = vec![array_field("scores", vec![FieldType::Number], true)];
        let values = FieldValueMap::<Vec<FieldSchema>>::from_untyped(
            &json!({"scores": []}),
            &composite_schemas,
            &schema,
        )
        .expect("an empty array parses");
        let refusal = values
            .validate_to_schema(&composite_schemas, &schema)
            .expect_err("a required array has to hold something");
        assert_eq!(refusal.code, "field_required");
        assert_eq!(refusal.field, "scores");
    }

    /// Reading is the other half of the round trip: an array of composites has to come back
    /// from JSON as the values it was written from.
    #[test]
    fn reads_an_array_of_composites() {
        let composite_schemas = create_composite_schemas_map();
        let field = FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "blocks".to_string(),
            field_type: FieldType::Array(vec![
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                }),
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_2".into(),
                }),
            ]),
            required: false,
            width: 12,
            height: 1,
            unique: false,
        };

        // What a read looks like: the wrapper names the definition, so the second candidate is
        // chosen even though the first would also accept an object of sub-values.
        let wrapped = json!([{ "id": "comp_2", "values": { "sub_field2": "Value2" } }]);
        assert_eq!(
            parse_field_value(&field, &wrapped, &composite_schemas).unwrap(),
            FieldValue::Array(vec![FieldValue::CompositeField(Some(
                create_composite_value("comp_2", "sub_field2", "Value2")
            ))])
        );

        // A write may leave the wrapper off; then the first declared type that accepts the
        // object wins, which is the documented rule.
        let bare = json!([{ "sub_field": "Value1" }]);
        assert_eq!(
            parse_field_value(&field, &bare, &composite_schemas).unwrap(),
            FieldValue::Array(vec![FieldValue::CompositeField(Some(
                create_composite_value("comp_1", "sub_field", "Value1")
            ))])
        );

        // A definition the array does not declare is not silently read as another one.
        let unknown = json!([{ "id": "comp_9", "values": { "sub_field": "Value1" } }]);
        assert!(
            parse_field_value(&field, &unknown, &composite_schemas)
                .unwrap_err()
                .contains("does not match any declared array item type")
        );
    }

    #[test]
    fn format_array_type_field() {
        let pattern = vec![
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions::default()),
                    FieldType::Number,
                ]),
                HashMap::from_iter(vec![]),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Number(Some(10.0)),
                    FieldValue::Boolean(true),
                ]),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Number(Some(10.0)),
                ]),
            ),
            (
                FieldType::Array(vec![
                    FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_1".into(),
                    }),
                    FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_2".into(),
                    }),
                    FieldType::Boolean,
                ]),
                HashMap::from_iter(vec![
                    (
                        "comp_1".into(),
                        vec![FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        }],
                    ),
                    (
                        "comp_2".into(),
                        vec![
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "sub_field2".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "sub_field3".to_string(),
                                field_type: FieldType::Number,
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                        ],
                    ),
                ]),
                FieldValue::Array(vec![
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field".to_string(),
                                FieldValue::Text("Value1".to_string()),
                            );
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_2".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert("sub_field3".to_string(), FieldValue::Number(Some(10.0)));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field2".to_string(),
                                FieldValue::Text("Value1".to_string()),
                            );
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_3".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field3".to_string(),
                                FieldValue::Text("Value1".to_string()),
                            );
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::Number(Some(20.0)),
                    FieldValue::Text("Invalid".to_string()),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_2".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert("sub_field2".to_string(), FieldValue::Number(Some(10.0)));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::Boolean(true),
                ]),
                FieldValue::Array(vec![
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field".to_string(),
                                FieldValue::Text("Value1".to_string()),
                            );
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_2".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert("sub_field2".to_string(), FieldValue::Text("".to_string()));
                            v.insert("sub_field3".to_string(), FieldValue::Number(Some(10.0)));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert("sub_field".to_string(), FieldValue::Text("".to_string()));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_2".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert("sub_field2".to_string(), FieldValue::Text("".to_string()));
                            v.insert("sub_field3".to_string(), FieldValue::Number(None));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::Boolean(true),
                ]),
            ),
            (
                FieldType::Array(vec![
                    FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]),
                    FieldType::TextEnum(vec!["Option2".to_string(), "Option3".to_string()]),
                ]),
                HashMap::from_iter(vec![]),
                FieldValue::Array(vec![
                    FieldValue::TextEnum(vec!["Option1".to_string(), "Invalid".to_string()]),
                    FieldValue::TextEnum(vec!["Option3".to_string()]),
                    FieldValue::TextEnum(vec!["Option3".to_string(), "Option1".to_string()]),
                    FieldValue::TextEnum(vec!["InvalidOption".to_string()]),
                ]),
                FieldValue::Array(vec![
                    FieldValue::TextEnum(vec!["Option1".to_string()]),
                    FieldValue::TextEnum(vec!["Option3".to_string()]),
                    FieldValue::TextEnum(vec!["Option3".to_string(), "Option1".to_string()]),
                    FieldValue::TextEnum(vec![]),
                ]),
            ),
            (
                FieldType::Array(vec![FieldType::Array(vec![FieldType::Boolean])]),
                HashMap::from_iter(vec![]),
                FieldValue::Array(vec![FieldValue::Array(vec![
                    FieldValue::Boolean(true),
                    FieldValue::Boolean(false),
                ])]),
                FieldValue::Array(vec![]),
            ),
        ];
        for (fields, composite_schema, field_value, expected_formatted_value) in pattern {
            let scheme = FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "test_array".to_string(),
                field_type: fields,
                required: true,
                width: 12,
                height: 1,
                unique: false,
            };
            let formatted_value = field_value.format_field_value(&scheme, &composite_schema);
            assert_eq!(
                formatted_value, expected_formatted_value,
                "Failed on field type {:?}",
                scheme
            );
        }
    }
    #[test]
    fn schema_serialization() {
        for field_type in FieldType::iter() {
            let (field_schema, json_schema) = match field_type {
                FieldType::Text(options) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(options),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"title","field_type":{"Text":{"max_length":null,"min_length":null}},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Markdown(options) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "description".to_string(),
                        field_type: FieldType::Markdown(options),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"description","field_type":{"Markdown":{"max_length":null,"min_length":null}},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Slug(ref options) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "address".to_string(),
                        field_type: FieldType::Slug(options.clone()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    // A slug without a suggestion carries no options at all: the character rule
                    // and the length cap belong to the type, not to the schema.
                    let json_schema = r#"{"name":"address","field_type":{"Slug":{}},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Number => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"age","field_type":"Number","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Boolean => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "is_active".to_string(),
                        field_type: FieldType::Boolean,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"is_active","field_type":"Boolean","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Date => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "create_date".to_string(),
                        field_type: FieldType::Date,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"create_date","field_type":"Date","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::DateTime => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "update_time".to_string(),
                        field_type: FieldType::DateTime,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"update_time","field_type":"DateTime","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Image => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "profile_image".to_string(),
                        field_type: FieldType::Image,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"profile_image","field_type":"Image","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::CompositeField(_) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "address".to_string(),
                        field_type: FieldType::CompositeField(CompositeFieldReference {
                            id: "address_1".into(),
                        }),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"address","field_type":{"CompositeField":{"id":"address_1"}},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Relation(_) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "author".to_string(),
                        field_type: FieldType::Array(vec![FieldType::Relation(RelationOptions {
                            target: RelationTarget::Collection {
                                name: "authors".to_string(),
                            },
                            inverse_name: Some("articles".to_string()),
                        })]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"author","field_type":{"Array":[{"Relation":{"target":{"kind":"collection","name":"authors"},"inverse_name":"articles"}}]},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Array(_) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![
                            FieldType::Text(TextFieldOptions::default()),
                            FieldType::Number,
                        ]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"tags","field_type":{"Array":[{"Text":{"max_length":null,"min_length":null}},"Number"]},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::TextEnum(_) => {
                    let field_schema = FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "status".to_string(),
                        field_type: FieldType::TextEnum(vec![
                            "Active".to_string(),
                            "Inactive".to_string(),
                        ]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema = r#"{"name":"status","field_type":{"TextEnum":["Active","Inactive"]},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
            };
            let serialized_schema = serde_json::to_string(&field_schema).unwrap();
            let deserialized_schema: FieldSchema =
                serde_json::from_str(&serialized_schema).unwrap();
            let deserialized_from_json_schema: FieldSchema =
                serde_json::from_str(json_schema).unwrap();
            assert_eq!(
                field_schema, deserialized_schema,
                "Failed on field type {:?}",
                field_type
            );
            assert_eq!(
                field_schema, deserialized_from_json_schema,
                "Failed on field type {:?}",
                field_type
            );
        }
    }
    #[test]
    fn field_value_serialization() {
        for field_type in FieldType::iter() {
            let (field_value, field_value_json) = match field_type {
                FieldType::Text(_) => {
                    (FieldValue::Text("Hello".to_string()), r#"{"Text":"Hello"}"#)
                }
                FieldType::Slug(_) => {
                    (FieldValue::Text("hello".to_string()), r#"{"Text":"hello"}"#)
                }
                FieldType::Markdown(_) => (
                    FieldValue::Markdown("**Bold Text**".to_string()),
                    r#"{"Markdown":"**Bold Text**"}"#,
                ),
                FieldType::Number => (FieldValue::Number(Some(42.0)), r#"{"Number":42.0}"#),
                FieldType::Boolean => (FieldValue::Boolean(true), r#"{"Boolean":true}"#),
                FieldType::Date => (
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                    r#"{"Date":"2023-01-01"}"#,
                ),
                FieldType::DateTime => (
                    FieldValue::DateTime(Some(
                        DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                    )),
                    r#"{"DateTime":"2023-01-01T12:00:00+00:00"}"#,
                ),
                FieldType::Image => (
                    FieldValue::Image(Some(ImageId::from_u64(1))),
                    r#"{"Image":1}"#,
                ),
                FieldType::CompositeField(_) => (
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field".to_string(),
                                FieldValue::Text("Value".to_string()),
                            );
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    r#"{"CompositeField":{"id":"comp_1","values":{"sub_field":{"Text":"Value"}}}}"#,
                ),
                FieldType::Relation(_) => (
                    FieldValue::Relation(Some(RelationRef {
                        target: "authors".to_string(),
                        item: Some(7),
                    })),
                    r#"{"Relation":{"target":"authors","item":7}}"#,
                ),
                FieldType::Array(_) => (
                    FieldValue::Array(vec![
                        FieldValue::Text("Item1".to_string()),
                        FieldValue::Number(Some(10.0)),
                    ]),
                    r#"{"Array":[{"Text":"Item1"},{"Number":10.0}]}"#,
                ),
                FieldType::TextEnum(_) => (
                    FieldValue::TextEnum(vec!["Option1".to_string()]),
                    r#"{"TextEnum":["Option1"]}"#,
                ),
            };
            let serialized_value = serde_json::to_string(&field_value).unwrap();
            let deserialized_value: FieldValue = serde_json::from_str(&serialized_value).unwrap();
            let deserialized_from_json_value: FieldValue =
                serde_json::from_str(field_value_json).unwrap();
            assert_eq!(
                field_value, deserialized_value,
                "Failed on field type {:?}",
                field_type
            );
            assert_eq!(
                field_value, deserialized_from_json_value,
                "Failed on field type {:?}",
                field_type
            );
        }
        for field_type in FieldType::iter() {
            let (field_value, field_value_json) = match field_type {
                FieldType::Text(_) => (FieldValue::Text("".to_string()), r#"{"Text":""}"#),
                FieldType::Number => (FieldValue::Number(None), r#"{"Number":null}"#),
                FieldType::Date => (FieldValue::Date(None), r#"{"Date":null}"#),
                FieldType::DateTime => (FieldValue::DateTime(None), r#"{"DateTime":null}"#),
                FieldType::Image => (FieldValue::Image(None), r#"{"Image":null}"#),
                FieldType::Relation(_) => (FieldValue::Relation(None), r#"{"Relation":null}"#),
                FieldType::Array(_) => (FieldValue::Array(vec![]), r#"{"Array":[]}"#),
                FieldType::TextEnum(_) => (FieldValue::TextEnum(vec![]), r#"{"TextEnum":[]}"#),
                _ => continue,
            };
            let serialized_value = serde_json::to_string(&field_value).unwrap();
            let deserialized_value: FieldValue = serde_json::from_str(&serialized_value).unwrap();
            let deserialized_from_json_value: FieldValue =
                serde_json::from_str(field_value_json).unwrap();
            assert_eq!(
                field_value, deserialized_value,
                "Failed on field type {:?}",
                field_type
            );
            assert_eq!(
                field_value, deserialized_from_json_value,
                "Failed on field type {:?}",
                field_type
            );
        }
    }
    #[test]
    fn default_values() {
        for field_type in FieldType::iter() {
            let test_schema = FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "test".to_string(),
                field_type: field_type.clone(),
                required: false,
                width: 12,
                height: 1,
                unique: false,
            };
            let default_value = test_schema.get_default_value();
            match field_type {
                FieldType::Text(_) | FieldType::Slug(_) => {
                    assert_eq!(default_value, FieldValue::Text(String::new()))
                }
                FieldType::Markdown(_) => {
                    assert_eq!(default_value, FieldValue::Markdown(String::new()))
                }
                FieldType::Number => assert_eq!(default_value, FieldValue::Number(None)),
                FieldType::Boolean => assert_eq!(default_value, FieldValue::Boolean(false)),
                FieldType::Date => assert_eq!(default_value, FieldValue::Date(None)),
                FieldType::DateTime => assert_eq!(default_value, FieldValue::DateTime(None)),
                FieldType::Image => assert_eq!(default_value, FieldValue::Image(None)),
                FieldType::CompositeField(_) => {
                    assert_eq!(default_value, FieldValue::CompositeField(None))
                }
                FieldType::Array(_) => assert_eq!(default_value, FieldValue::Array(vec![])),
                FieldType::Relation(_) => assert_eq!(default_value, FieldValue::Relation(None)),
                FieldType::TextEnum(_) => assert_eq!(default_value, FieldValue::TextEnum(vec![])),
            }
        }
        assert_eq!(FieldValue::default(), FieldValue::Text(String::new()));
        assert_eq!(
            FieldType::default(),
            FieldType::Text(TextFieldOptions::default())
        );
    }
    #[test]
    fn schema_validation_valid_values() {
        for field_type in FieldType::iter() {
            let (field_schema, composite_schemas, field_value) = match field_type {
                FieldType::Text(options) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(options),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Text("Hello".to_string()),
                ),
                FieldType::Markdown(options) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "description".to_string(),
                        field_type: FieldType::Markdown(options),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Markdown("**Bold Text**".to_string()),
                ),
                FieldType::Slug(ref options) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "address".to_string(),
                        field_type: FieldType::Slug(options.clone()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Text("a-slug".to_string()),
                ),
                FieldType::Number => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Number(Some(25.0)),
                ),
                FieldType::Boolean => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "is_active".to_string(),
                        field_type: FieldType::Boolean,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Boolean(true),
                ),
                FieldType::Date => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "create_date".to_string(),
                        field_type: FieldType::Date,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                ),
                FieldType::DateTime => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "update_time".to_string(),
                        field_type: FieldType::DateTime,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::DateTime(Some(
                        DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                    )),
                ),
                FieldType::Image => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "profile_image".to_string(),
                        field_type: FieldType::Image,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Image(Some(ImageId::from_u64(1))),
                ),
                FieldType::CompositeField(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "address".to_string(),
                        field_type: FieldType::CompositeField(CompositeFieldReference {
                            id: "address_1".into(),
                        }),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::from([(
                        "address_1".into(),
                        vec![
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "street".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "city".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "zip".to_string(),
                                field_type: FieldType::Number,
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                        ],
                    )]),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "address_1".into(),
                        values: FieldValueMap(
                            HashMap::from([
                                (
                                    "street".to_string(),
                                    FieldValue::Text("123 Main St".to_string()),
                                ),
                                ("city".to_string(), FieldValue::Text("Anytown".to_string())),
                                ("zip".to_string(), FieldValue::Number(Some(12345.0))),
                            ]),
                            PhantomData,
                        ),
                    })),
                ),
                FieldType::Array(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![
                            FieldType::Text(TextFieldOptions::default()),
                            FieldType::Number,
                        ]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Array(vec![
                        FieldValue::Text("tag1".to_string()),
                        FieldValue::Number(Some(1.0)),
                    ]),
                ),
                FieldType::TextEnum(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "status".to_string(),
                        field_type: FieldType::TextEnum(vec![
                            "Active".to_string(),
                            "Inactive".to_string(),
                        ]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::TextEnum(vec!["Active".to_string()]),
                ),
                FieldType::Relation(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "author".to_string(),
                        field_type: FieldType::Array(vec![FieldType::Relation(RelationOptions {
                            target: RelationTarget::Collection {
                                name: "authors".to_string(),
                            },
                            inverse_name: None,
                        })]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    HashMap::new(),
                    FieldValue::Array(vec![FieldValue::Relation(Some(RelationRef {
                        target: "authors".to_string(),
                        item: Some(7),
                    }))]),
                ),
            };
            let result = field_value.validate_field_value(&field_schema, &composite_schemas);
            assert!(result.is_ok(), "Failed on field type {:?}", field_type);
        }
    }
    #[test]
    fn field_type_test_required() {
        for field_type in FieldType::iter() {
            let value = match field_type {
                FieldType::Text(_) | FieldType::Slug(_) => FieldValue::Text("Sample".to_string()),
                FieldType::Markdown(_) => FieldValue::Markdown("**Sample**".to_string()),
                FieldType::Number => FieldValue::Number(Some(10.0)),
                FieldType::Boolean => FieldValue::Boolean(true),
                FieldType::Date => {
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()))
                }
                FieldType::DateTime => FieldValue::DateTime(Some(
                    DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                )),
                FieldType::Image => FieldValue::Image(Some(ImageId::from_u64(1))),
                FieldType::CompositeField(_) => {
                    FieldValue::CompositeField(Some(CompositeFieldValue::default()))
                }
                FieldType::Array(_) => {
                    FieldValue::Array(vec![FieldValue::Text("Item".to_string())])
                }
                FieldType::Relation(_) => FieldValue::Relation(Some(RelationRef {
                    target: "authors".to_string(),
                    item: Some(1),
                })),
                FieldType::TextEnum(_) => FieldValue::TextEnum(vec!["Option".to_string()]),
            };
            assert!(
                field_type.test_required(&value),
                "Failed on field type {:?}",
                field_type
            );
        }
        assert!(
            !FieldType::Text(TextFieldOptions::default()).test_required(&FieldValue::Boolean(true))
        );
    }

    #[test]
    fn schema_validation_required_field_missing_value() {
        for field_type in FieldType::iter() {
            let (field_schema, field_value) = match field_type {
                FieldType::Text(options) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(options),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Text("".to_string()),
                ),
                FieldType::Number => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Number(None),
                ),
                FieldType::Date => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "create_date".to_string(),
                        field_type: FieldType::Date,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Date(None),
                ),
                FieldType::DateTime => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "update_time".to_string(),
                        field_type: FieldType::DateTime,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::DateTime(None),
                ),
                FieldType::Image => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "profile_image".to_string(),
                        field_type: FieldType::Image,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Image(None),
                ),
                FieldType::Array(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![
                            FieldType::Text(TextFieldOptions::default()),
                            FieldType::Number,
                        ]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Array(vec![]),
                ),
                FieldType::TextEnum(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "status".to_string(),
                        field_type: FieldType::TextEnum(vec![
                            "Active".to_string(),
                            "Inactive".to_string(),
                        ]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::TextEnum(vec![]),
                ),
                _ => continue,
            };
            let result = field_value.validate_field_value(&field_schema, &HashMap::new());
            assert!(result.is_err(), "Failed on field type {:?}", field_type);
        }
    }

    #[test]
    fn schema_validation_non_required_field_missing_value() {
        for field_type in FieldType::iter() {
            let (field_schema, field_value) = match field_type {
                FieldType::Text(options) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(options),
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Text("".to_string()),
                ),
                FieldType::Number => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Number(None),
                ),
                FieldType::Date => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "create_date".to_string(),
                        field_type: FieldType::Date,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Date(None),
                ),
                FieldType::DateTime => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "update_time".to_string(),
                        field_type: FieldType::DateTime,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::DateTime(None),
                ),
                FieldType::Image => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "profile_image".to_string(),
                        field_type: FieldType::Image,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Image(None),
                ),
                FieldType::Array(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![
                            FieldType::Text(TextFieldOptions::default()),
                            FieldType::Number,
                        ]),
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Array(vec![]),
                ),
                FieldType::TextEnum(_) => (
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "status".to_string(),
                        field_type: FieldType::TextEnum(vec![
                            "Active".to_string(),
                            "Inactive".to_string(),
                        ]),
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::TextEnum(vec![]),
                ),
                _ => continue,
            };
            let result = field_value.validate_field_value(&field_schema, &HashMap::new());
            assert!(result.is_ok(), "Failed on field type {:?}", field_type);
        }
    }

    #[test]
    fn schema_validation_composite_field_edge_cases() {
        let pattern = vec![
            (
                FieldSchema {
                    is_title: false,
                    show_in_list: false,
                    name: "composite_field".to_string(),
                    field_type: FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_1".into(),
                    }),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                },
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "sub_field".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    }],
                )]),
                FieldValue::CompositeField(None),
                false,
            ),
            (
                FieldSchema {
                    is_title: false,
                    show_in_list: false,
                    name: "composite_field".to_string(),
                    field_type: FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_1".into(),
                    }),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                },
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "sub_field".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    }],
                )]),
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "comp_1".into(),
                    values: FieldValueMap(
                        HashMap::from_iter(vec![(
                            "sub_field".to_string(),
                            FieldValue::Text("test".to_string()),
                        )]),
                        PhantomData,
                    ),
                })),
                true,
            ),
            (
                FieldSchema {
                    is_title: false,
                    show_in_list: false,
                    name: "composite_field".to_string(),
                    field_type: FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_1".into(),
                    }),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                },
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "sub_field".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    }],
                )]),
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "comp_1".into(),
                    values: FieldValueMap(
                        HashMap::from_iter(vec![(
                            "sub_field".to_string(),
                            FieldValue::Text("".to_string()),
                        )]),
                        PhantomData,
                    ),
                })),
                true,
            ),
        ];
        for (field_schema, composite_schemas, field_value, should_be_valid) in pattern {
            let result = field_value.validate_field_value(&field_schema, &composite_schemas);
            if should_be_valid {
                assert!(result.is_ok(), "Failed on field schema {:?}", field_schema);
            } else {
                assert!(result.is_err(), "Failed on field schema {:?}", field_schema);
            }
        }
    }
    #[test]
    fn validate_field_value() {
        for field_type in FieldType::iter() {
            let (field_value, should_be_valid) = match field_type {
                FieldType::Text(_) => (FieldValue::Text("Hello".to_string()), true),
                FieldType::Slug(_) => (FieldValue::Text("hello".to_string()), true),
                FieldType::Markdown(_) => (FieldValue::Markdown("**Bold Text**".to_string()), true),
                FieldType::Number => (FieldValue::Number(Some(42.0)), true),
                FieldType::Boolean => (FieldValue::Boolean(true), true),
                FieldType::Date => (
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                    true,
                ),
                FieldType::DateTime => (
                    FieldValue::DateTime(Some(
                        DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                    )),
                    true,
                ),
                FieldType::Image => (FieldValue::Image(Some(ImageId::from_u64(1))), true),
                FieldType::Relation(_) => (
                    FieldValue::Relation(Some(RelationRef {
                        target: "authors".to_string(),
                        item: Some(3),
                    })),
                    true,
                ),
                FieldType::CompositeField(_) | FieldType::Array(_) | FieldType::TextEnum(_) => {
                    continue;
                }
            };
            let test_schema = FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "test".to_string(),
                field_type: field_type.clone(),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            };
            let result = field_value.validate_field_value(&test_schema, &HashMap::new());
            assert_eq!(
                result.is_ok(),
                should_be_valid,
                "Failed on field type {:?}",
                field_type
            );
        }

        let patterns = vec![
            (
                FieldType::Text(TextFieldOptions {
                    max_length: Some(10),
                    min_length: Some(5),
                    multiline: false,
                }),
                HashMap::new(),
                FieldValue::Text("Hello".to_string()),
                true,
                true,
            ),
            (
                FieldType::Text(TextFieldOptions {
                    max_length: Some(10),
                    min_length: Some(5),
                    multiline: false,
                }),
                HashMap::new(),
                FieldValue::Text("Hi".to_string()),
                true,
                false,
            ),
            (
                FieldType::Text(TextFieldOptions {
                    max_length: Some(10),
                    min_length: Some(5),
                    multiline: false,
                }),
                HashMap::new(),
                FieldValue::Text("Hello, World!".to_string()),
                true,
                false,
            ),
            (
                FieldType::Text(TextFieldOptions {
                    max_length: Some(10),
                    min_length: Some(5),
                    multiline: false,
                }),
                HashMap::new(),
                FieldValue::Text("".to_string()),
                true,
                false,
            ),
            (
                FieldType::Text(TextFieldOptions {
                    max_length: Some(10),
                    min_length: Some(5),
                    multiline: false,
                }),
                HashMap::new(),
                FieldValue::Text("".to_string()),
                false,
                true,
            ),
            (
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                }),
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![
                        FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                        FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "extra_field".to_string(),
                            field_type: FieldType::Number,
                            required: false,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                    ],
                )]),
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "comp_1".into(),
                    values: {
                        let mut v = HashMap::new();
                        v.insert(
                            "sub_field".to_string(),
                            FieldValue::Text("Value".to_string()),
                        );
                        FieldValueMap(v, PhantomData)
                    },
                })),
                true,
                true,
            ),
            (
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                }),
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![
                        FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                        FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "sub_field2".to_string(),
                            field_type: FieldType::Number,
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                    ],
                )]),
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "comp_2".into(),
                    values: FieldValueMap(HashMap::new(), PhantomData),
                })),
                true,
                false,
            ),
            (
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                }),
                HashMap::new(),
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "comp_1".into(),
                    values: FieldValueMap(HashMap::new(), PhantomData),
                })),
                true,
                false,
            ),
            (
                FieldType::CompositeField(CompositeFieldReference {
                    id: "comp_1".into(),
                }),
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![
                        FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                        FieldSchema {
                            is_title: false,
                            show_in_list: false,
                            name: "sub_field2".to_string(),
                            field_type: FieldType::Number,
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                    ],
                )]),
                FieldValue::CompositeField(None),
                false,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions::default()),
                    FieldType::Boolean,
                ]),
                HashMap::new(),
                FieldValue::Array(vec![]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions::default()),
                    FieldType::Boolean,
                ]),
                HashMap::new(),
                FieldValue::Array(vec![]),
                false,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions::default()),
                    FieldType::Boolean,
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Boolean(true),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(10),
                        min_length: Some(5),
                        multiline: false,
                    }),
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(8),
                        min_length: Some(3),
                        multiline: false,
                    }),
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("length 9.".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(10),
                        min_length: Some(5),
                        multiline: false,
                    }),
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(15),
                        min_length: Some(7),
                        multiline: false,
                    }),
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("length is 12".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions {
                        max_length: None,
                        min_length: Some(5),
                        multiline: false,
                    }),
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(10),
                        min_length: Some(7),
                        multiline: false,
                    }),
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("length is 12".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(10),
                        min_length: None,
                        multiline: false,
                    }),
                    FieldType::Text(TextFieldOptions {
                        max_length: Some(10),
                        min_length: Some(7),
                        multiline: false,
                    }),
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("1".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions {
                    max_length: Some(5),
                    min_length: Some(3),
                    multiline: false,
                })]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Text("1".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions {
                    max_length: Some(5),
                    min_length: Some(3),
                    multiline: false,
                })]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Text("length is 12".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Text("Item1".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions::default()),
                    FieldType::Boolean,
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Number(Some(10.0)),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Text(TextFieldOptions::default()),
                    FieldType::Boolean,
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Number(Some(10.0)),
                    FieldValue::Number(Some(20.0)),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Markdown(TextFieldOptions {
                        max_length: Some(10),
                        min_length: None,
                        multiline: false,
                    }),
                    FieldType::Markdown(TextFieldOptions {
                        max_length: Some(10),
                        min_length: Some(7),
                        multiline: false,
                    }),
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Markdown("Item1".to_string()),
                    FieldValue::Markdown("1".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Markdown(TextFieldOptions {
                    max_length: Some(5),
                    min_length: Some(3),
                    multiline: false,
                })]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Markdown("1".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Markdown(TextFieldOptions {
                    max_length: Some(5),
                    min_length: Some(3),
                    multiline: false,
                })]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Markdown("length is 12".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Text("Item1".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Markdown("Item1".to_string())]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Array(vec![FieldType::Boolean])]),
                HashMap::new(),
                FieldValue::Array(vec![FieldValue::Array(vec![
                    FieldValue::Boolean(true),
                    FieldValue::Boolean(false),
                ])]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![
                    FieldType::Boolean,
                    FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_1".into(),
                    }),
                ]),
                HashMap::from_iter(vec![(
                    "comp_1".into(),
                    vec![FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "sub_field".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    }],
                )]),
                FieldValue::Array(vec![
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field".to_string(),
                                FieldValue::Text("Value".to_string()),
                            );
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::Boolean(true),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![
                    FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]),
                    FieldType::TextEnum(vec!["Option2".to_string(), "Option3".to_string()]),
                ]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::TextEnum(vec!["Option1".to_string(), "Option3".to_string()]),
                    FieldValue::TextEnum(vec!["Option2".to_string()]),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::TextEnum(vec![
                    "Option1".to_string(),
                    "Option2".to_string(),
                ])]),
                HashMap::new(),
                // An element that holds nothing is *empty*, not missing: the array holds an
                // element, which is all `required` asks of it (see
                // `a_blank_array_element_is_not_a_missing_required_field`).
                FieldValue::Array(vec![FieldValue::TextEnum(vec![])]),
                true,
                true,
            ),
            (
                FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]),
                HashMap::new(),
                FieldValue::TextEnum(vec!["Option1".to_string()]),
                true,
                true,
            ),
            (
                FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]),
                HashMap::new(),
                FieldValue::TextEnum(vec![]),
                true,
                false,
            ),
            (
                FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]),
                HashMap::new(),
                FieldValue::TextEnum(vec![]),
                false,
                true,
            ),
            (
                FieldType::TextEnum(vec!["Option1".to_string()]),
                HashMap::new(),
                FieldValue::TextEnum(vec!["Option1".to_string(), "InvalidOption".to_string()]),
                true,
                false,
            ),
            (
                FieldType::TextEnum(vec!["invalidTest".to_string()]),
                HashMap::new(),
                FieldValue::TextEnum(vec!["InvalidOption".to_string()]),
                true,
                false,
            ),
            (
                FieldType::TextEnum(vec!["empty".to_string()]),
                HashMap::new(),
                FieldValue::TextEnum(vec![]),
                true,
                false,
            ),
            (
                FieldType::TextEnum(vec!["boolean".to_string()]),
                HashMap::new(),
                FieldValue::Boolean(true),
                true,
                false,
            ),
        ];
        for (field, composite_schemas, value, required, expected_validity) in patterns {
            let schema = FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "test_field".to_string(),
                field_type: field,
                required,
                width: 12,
                height: 1,
                unique: false,
            };
            let result = value.validate_field_value(&schema, &composite_schemas);
            assert_eq!(
                result.is_ok(),
                expected_validity,
                "Failed on field type {:?}, error: {:?}",
                schema,
                result.err()
            );
        }
    }
    #[test]
    fn format_field_value() {
        for field_type in FieldType::iter() {
            let (field, composite_schemas, field_value, expected_formatted_value) = match field_type
            {
                FieldType::Text(_) => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Text("Hello".to_string()),
                    FieldValue::Text("Hello".to_string()),
                ),
                FieldType::Slug(_) => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Text("hello".to_string()),
                    FieldValue::Text("hello".to_string()),
                ),
                FieldType::Markdown(_) => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Markdown("**Bold Text**".to_string()),
                    FieldValue::Markdown("**Bold Text**".to_string()),
                ),
                FieldType::Number => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Number(Some(42.0)),
                    FieldValue::Number(Some(42.0)),
                ),
                FieldType::Boolean => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Boolean(true),
                    FieldValue::Boolean(true),
                ),
                FieldType::Date => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                ),
                FieldType::DateTime => (
                    field_type,
                    HashMap::new(),
                    FieldValue::DateTime(Some(
                        DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                    )),
                    FieldValue::DateTime(Some(
                        DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                    )),
                ),
                FieldType::Image => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Image(Some(ImageId::from_u64(1))),
                    FieldValue::Image(Some(ImageId::from_u64(1))),
                ),
                FieldType::CompositeField(_) => (
                    FieldType::CompositeField(CompositeFieldReference {
                        id: "comp_1".into(),
                    }),
                    HashMap::from_iter(vec![(
                        "comp_1".into(),
                        vec![
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "sub_field".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
                                is_title: false,
                                show_in_list: false,
                                name: "extra_field".to_string(),
                                field_type: FieldType::Number,
                                required: false,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                        ],
                    )]),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field".to_string(),
                                FieldValue::Text("Value".to_string()),
                            );
                            v.insert("unknown_field".to_string(), FieldValue::Boolean(true));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "comp_1".into(),
                        values: {
                            let mut v = HashMap::new();
                            v.insert(
                                "sub_field".to_string(),
                                FieldValue::Text("Value".to_string()),
                            );
                            v.insert("extra_field".to_string(), FieldValue::Number(None));
                            FieldValueMap(v, PhantomData)
                        },
                    })),
                ),
                FieldType::Array(_) => (
                    FieldType::Array(vec![
                        FieldType::Text(TextFieldOptions::default()),
                        FieldType::Boolean,
                    ]),
                    HashMap::new(),
                    FieldValue::Array(vec![
                        FieldValue::Text("Item1".to_string()),
                        FieldValue::Number(Some(10.0)),
                    ]),
                    FieldValue::Array(vec![FieldValue::Text("Item1".to_string())]),
                ),
                FieldType::Relation(_) => (
                    field_type,
                    HashMap::new(),
                    FieldValue::Relation(Some(RelationRef {
                        target: "authors".to_string(),
                        item: Some(7),
                    })),
                    FieldValue::Relation(Some(RelationRef {
                        target: "authors".to_string(),
                        item: Some(7),
                    })),
                ),
                FieldType::TextEnum(_) => (
                    FieldType::TextEnum(vec!["Option1".to_string()]),
                    HashMap::new(),
                    FieldValue::TextEnum(vec!["Option1".to_string(), "InvalidOption".to_string()]),
                    FieldValue::TextEnum(vec!["Option1".to_string()]),
                ),
            };
            let scheme = FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "test".to_string(),
                field_type: field,
                required: true,
                width: 12,
                height: 1,
                unique: false,
            };
            let formatted_value = field_value.format_field_value(&scheme, &composite_schemas);
            assert_eq!(
                formatted_value, expected_formatted_value,
                "Failed on field type {:?}",
                scheme
            );
        }
    }
    #[test]
    fn get_field_type() {
        for value in FieldValue::iter() {
            let field_type = value.get_type();
            match value {
                FieldValue::Text(_) => {
                    assert_eq!(field_type, FieldType::Text(TextFieldOptions::default()))
                }
                FieldValue::Markdown(_) => {
                    assert_eq!(field_type, FieldType::Markdown(TextFieldOptions::default()))
                }
                FieldValue::Number(_) => assert_eq!(field_type, FieldType::Number),
                FieldValue::Boolean(_) => assert_eq!(field_type, FieldType::Boolean),
                FieldValue::Date(_) => assert_eq!(field_type, FieldType::Date),
                FieldValue::DateTime(_) => assert_eq!(field_type, FieldType::DateTime),
                FieldValue::Image(_) => assert_eq!(field_type, FieldType::Image),
                FieldValue::CompositeField(_) => {
                    assert_eq!(
                        field_type,
                        FieldType::CompositeField(CompositeFieldReference {
                            id: CompositeFieldId::default()
                        })
                    );
                }
                FieldValue::Array(_) => assert_eq!(field_type, FieldType::Array(vec![])),
                FieldValue::Relation(_) => {
                    assert_eq!(field_type, FieldType::Relation(RelationOptions::default()))
                }
                FieldValue::TextEnum(_) => assert_eq!(field_type, FieldType::TextEnum(vec![])),
            }
        }
    }

    /// The pairs `matches_type` accepts, named here independently so that a change to the mapping is
    /// a change to this table too.
    ///
    /// The function itself is the guard: it matches on the *field type* with every variant named, so
    /// a new one is a compile error until it is answered here as well. A value of the wrong kind is
    /// `false`.
    #[test]
    fn matches_type_accepts_exactly_these_pairs() {
        fn expected(value: &FieldValue, field_type: &FieldType) -> bool {
            matches!(
                (value, field_type),
                (FieldValue::Text(_), FieldType::Text(_) | FieldType::Slug(_))
                    | (FieldValue::Markdown(_), FieldType::Markdown(_))
                    | (FieldValue::Number(_), FieldType::Number)
                    | (FieldValue::Boolean(_), FieldType::Boolean)
                    | (FieldValue::Date(_), FieldType::Date)
                    | (FieldValue::DateTime(_), FieldType::DateTime)
                    | (FieldValue::Image(_), FieldType::Image)
                    | (FieldValue::CompositeField(_), FieldType::CompositeField(_))
                    | (FieldValue::Relation(_), FieldType::Relation(_))
                    | (FieldValue::Array(_), FieldType::Array(_))
                    | (FieldValue::TextEnum(_), FieldType::TextEnum(_))
            )
        }

        for field_type in FieldType::iter() {
            for value in FieldValue::iter() {
                assert_eq!(
                    value.matches_type(&field_type),
                    expected(&value, &field_type),
                    "{value:?} against {field_type:?}"
                );
            }
        }
    }

    #[test]
    fn format_to_schema() {
        let schema = vec![
            FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            },
            FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
            FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "profile".to_string(),
                field_type: FieldType::CompositeField(CompositeFieldReference {
                    id: "profile_1".into(),
                }),
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
            FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "tags".to_string(),
                field_type: FieldType::Array(vec![
                    FieldType::CompositeField(CompositeFieldReference { id: "tag_1".into() }),
                    FieldType::Text(TextFieldOptions::default()),
                ]),
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
        ];
        let composite_schemas = HashMap::from([
            (
                "profile_1".into(),
                vec![
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "bio".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "avatar".to_string(),
                        field_type: FieldType::Image,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
            ),
            (
                "tag_1".into(),
                vec![FieldSchema {
                    is_title: false,
                    show_in_list: false,
                    name: "name".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                }],
            ),
        ]);
        let values = FieldValueMap(
            HashMap::from([
                ("title".to_string(), FieldValue::Text("Hello".to_string())),
                (
                    "profile".to_string(),
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "profile_1".into(),
                        values: FieldValueMap(
                            HashMap::from([(
                                "bio".to_string(),
                                FieldValue::Text("This is my bio".to_string()),
                            )]),
                            PhantomData,
                        ),
                    })),
                ),
                (
                    "tags".to_string(),
                    FieldValue::Array(vec![
                        FieldValue::CompositeField(Some(CompositeFieldValue {
                            id: "tag_1".into(),
                            values: FieldValueMap(
                                HashMap::from([(
                                    "name".to_string(),
                                    FieldValue::Text("rust".to_string()),
                                )]),
                                PhantomData,
                            ),
                        })),
                        FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                        FieldValue::Text("programming".to_string()),
                    ]),
                ),
            ]),
            PhantomData,
        );
        let formatted = values.format_to_schema(&composite_schemas, &schema);
        assert_eq!(
            formatted.get("title"),
            Some(&FieldValue::Text("Hello".to_string()))
        );
        assert_eq!(formatted.get("age"), Some(&FieldValue::Number(None)));
        assert!(
            formatted.get("profile").is_some(),
            "Profile field should be present"
        );
        let fv = formatted.get("profile").unwrap();
        match fv {
            FieldValue::CompositeField(Some(cv)) => {
                assert_eq!(cv.id, "profile_1".into());
                assert_eq!(
                    cv.values.get("bio"),
                    Some(&FieldValue::Text("This is my bio".to_string()))
                );
                assert_eq!(cv.values.get("avatar"), Some(&FieldValue::Image(None)));
            }
            _ => panic!("Profile field should be a CompositeField with Some value"),
        }

        assert_eq!(
            formatted.get("tags"),
            Some(&FieldValue::Array(vec![
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "tag_1".into(),
                    values: FieldValueMap(
                        HashMap::from([("name".to_string(), FieldValue::Text("rust".to_string()))]),
                        PhantomData
                    ),
                })),
                FieldValue::Text("programming".to_string()),
            ]))
        );
    }

    #[test]
    fn validate_to_schema() {
        let pattern = vec![
            (
                vec![
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
                FieldValueMap(
                    HashMap::from([
                        ("title".to_string(), FieldValue::Text("Hello".to_string())),
                        ("age".to_string(), FieldValue::Number(Some(25.0))),
                    ]),
                    PhantomData,
                ),
                true,
            ),
            (
                vec![
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
                FieldValueMap(
                    HashMap::from([("title".to_string(), FieldValue::Text("Hello".to_string()))]),
                    PhantomData,
                ),
                true,
            ),
            (
                vec![
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "title".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        is_title: false,
                        show_in_list: false,
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
                FieldValueMap(
                    HashMap::from([("title".to_string(), FieldValue::Text("Hello".to_string()))]),
                    PhantomData,
                ),
                false,
            ),
        ];
        for (schema, values, expected_result) in pattern {
            let result = values.validate_to_schema(&HashMap::new(), &schema);
            assert_eq!(
                result.is_ok(),
                expected_result,
                "Schema: {:?}, Values: {:?}",
                schema,
                values
            );
        }
    }
    #[test]
    fn field_type_enum_validation() {
        let field_type = FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]);
        let schema = FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "status".to_string(),
            field_type: field_type.clone(),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        };
        let valid_value = FieldValue::TextEnum(vec!["Option1".to_string()]);
        let invalid_value = FieldValue::TextEnum(vec!["InvalidOption".to_string()]);
        assert!(
            valid_value
                .validate_field_value(&schema, &HashMap::new())
                .is_ok()
        );
        assert!(
            invalid_value
                .validate_field_value(&schema, &HashMap::new())
                .is_err()
        );
    }
}
