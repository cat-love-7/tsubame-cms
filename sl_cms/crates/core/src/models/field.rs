use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, marker::PhantomData};

#[cfg(test)]
use strum_macros::EnumIter;

use crate::models::{
    error::FieldRefusal,
    image::{Image, ImageId, ImageResponse},
    schema::CompositeFieldId,
};

pub use super::schema::{
    CompositeFieldReference, CompositeFieldSchema, FieldSchema, FieldType, TextFieldOptions,
};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CompositeFieldValue {
    pub id: CompositeFieldId,
    pub values: FieldValueMap<CompositeFieldSchema>,
}

#[derive(Debug, Clone, Default)]
pub struct FieldValueMap<T>(pub HashMap<String, FieldValue>,pub PhantomData<fn() -> T>);
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
        // Handled by `parse_field_value` before reaching here.
        FieldType::Array(_) | FieldType::CompositeField(_) => Err(mismatch(field, "a scalar value")),
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
        if let (Some(id), FieldType::CompositeField(reference)) = (declared_id, candidate) {
            if reference.id.as_str() != id {
                continue;
            }
        }
        let probe = FieldSchema {
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

fn mismatch(field: &FieldSchema, expected: &str) -> String {
    format!("Field '{}': expected {expected}", field.name)
}

#[cfg(test)]
mod untyped_parsing_tests {
    use super::*;
    use crate::models::collection::CollectionSchema;
    use serde_json::json;

    fn field(name: &str, field_type: FieldType, required: bool) -> FieldSchema {
        FieldSchema { name: name.to_string(), field_type, required, width: 12, height: 1, unique: false }
    }

    fn schema() -> CollectionSchema {
        vec![
            field("title", FieldType::Text(TextFieldOptions::default()), true),
            field("body", FieldType::Markdown(TextFieldOptions::default()), false),
            field("count", FieldType::Number, false),
            field("live", FieldType::Boolean, false),
            field("published", FieldType::Date, false),
            field("at", FieldType::DateTime, false),
            field("cover", FieldType::Image, false),
            field("tags", FieldType::TextEnum(vec!["news".into(), "blog".into()]), false),
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
        assert_eq!(parsed.0["cover"], FieldValue::Image(Some(ImageId::from_u64(7))));
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
        let parsed = parse(json!({ "title": "t", "count": null, "published": null, "cover": null }))
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
        assert_eq!(parsed.0["cover"], FieldValue::Image(Some(ImageId::from_u64(7))));

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
        assert!(err.contains("Unknown field 'titel'"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_enum_values_outside_the_declared_options() {
        let err = parse(json!({ "title": "t", "tags": ["news", "nope"] })).unwrap_err();
        assert!(err.contains("not one of the allowed options"), "unexpected error: {err}");
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
            FieldType::CompositeField(CompositeFieldReference { id: composite_id.clone() }),
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
            FieldValueMap::from_untyped(&json!({ "seo": null }), &schemas, &outer).unwrap().0["seo"],
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
            FieldType::CompositeField(CompositeFieldReference { id: composite_id.clone() }),
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
            FieldType::CompositeField(CompositeFieldReference { id: composite_id.clone() }),
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
    Array(Vec<FieldValue>),
    TextEnum(Vec<String>),
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
                        id: id.clone(),
                        url: img.url.clone(),
                    })
                });
                FieldValueResponse::Image(img_response)
            }
            FieldValue::CompositeField(cv) => {
                FieldValueResponse::CompositeField(cv.as_ref().map(|v| {
                    CompositeFieldValueResponse {
                        id: v.id.clone(),
                        values: v.values.0.iter()
                            .map(|(k, val)| (k.clone(), val.to_response(images)))
                            .collect(),
                    }
                }))
            }
            FieldValue::Array(arr) => {
                FieldValueResponse::Array(arr.iter().map(|v| v.to_response(images)).collect())
            }
            FieldValue::TextEnum(vals) => FieldValueResponse::TextEnum(vals.clone()),
        }
    }
}

impl FieldValue {
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
                None => FieldType::CompositeField(CompositeFieldReference { id: CompositeFieldId::default() }),
            },
            FieldValue::Array(_values) => FieldType::Array(vec![]),
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
                    None => acc = Some(options.clone()),
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
        match (&schema.field_type, self) {
            (FieldType::Text(field_params), FieldValue::Text(text))
            | (FieldType::Markdown(field_params), FieldValue::Markdown(text)) => {
                if schema.required && schema.field_type.test_required(self) == false {
                    return Err(FieldRefusal::required(&path));
                }
                schema.validate_text_length(text, &field_params, &path)?;
                Ok(())
            }
            (FieldType::Slug(_), FieldValue::Text(slug)) => {
                if schema.required && schema.field_type.test_required(self) == false {
                    return Err(FieldRefusal::required(&path));
                }
                if slug.chars().count() > crate::models::slug::SLUG_MAX_LENGTH {
                    return Err(FieldRefusal::too_long(
                        &path,
                        crate::models::slug::SLUG_MAX_LENGTH,
                    ));
                }
                // Everything a slug may hold, and nothing else. A value that is not canonical is
                // refused rather than rewritten here: normalising on this side would quietly store
                // something other than what was sent, and the caller would never learn.
                let canonical = slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if !canonical {
                    return Err(FieldRefusal::invalid_slug(&path));
                }
                Ok(())
            }
            (FieldType::Number, FieldValue::Number(_))
            | (FieldType::Boolean, FieldValue::Boolean(_))
            | (FieldType::Date, FieldValue::Date(_))
            | (FieldType::DateTime, FieldValue::DateTime(_))
            | (FieldType::Image, FieldValue::Image(_)) => {
                if schema.required && schema.field_type.test_required(self) == false {
                    return Err(FieldRefusal::required(&path));
                }
                Ok(())
            }
            (FieldType::CompositeField(s), FieldValue::CompositeField(field_value)) => {
                if schema.required && schema.field_type.test_required(self) == false {
                    return Err(FieldRefusal::required(&path));
                }
                if let Some(v) = field_value {
                    if s.id != v.id {
                        return Err(FieldRefusal::composite_mismatch(&path));
                    }
                    let composite_schema = composite_schemas
                        .get(&s.id)
                        .ok_or_else(|| FieldRefusal::unknown_composite(&path, &s.id))?;
                    v.values
                        .validate_at(&format!("{path}."), composite_schemas, &composite_schema)?;
                    Ok(())
                } else {
                    Ok(())
                }
            }
            (FieldType::Array(schemas), FieldValue::Array(values)) => {
                if schema.required && schema.field_type.test_required(self) == false {
                    return Err(FieldRefusal::required(&path));
                }
                for (index, value) in values.iter().enumerate() {
                    // An array item is named by the path plus its index: `tags[2]`.
                    let item = format!("{path}[{index}]");
                    match value {
                        FieldValue::Text(value) => {
                            let option = Self::extract_text_options(&schemas, |ft| {
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
                            let option = Self::extract_text_options(&schemas, |ft| {
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
                            let value_type = value.get_type();
                            if schemas.contains(&value_type) == false {
                                return Err(FieldRefusal::type_mismatch(&item));
                            }
                            if value_type.test_required(value) == false {
                                return Err(FieldRefusal::required(&item));
                            }
                        }
                        FieldValue::CompositeField(cv) => match cv {
                            Some(cv) => {
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
                                        let composite_schema = composite_schemas
                                            .get(&s.id)
                                            .ok_or_else(|| {
                                                FieldRefusal::unknown_composite(&item, &s.id)
                                            })?;

                                        cv.values.validate_at(
                                            &format!("{item}."),
                                            composite_schemas,
                                            &composite_schema,
                                        )?;
                                    }
                                    None => {
                                        // The element names a definition this array does not
                                        // declare, so the element itself is the wrong kind.
                                        return Err(FieldRefusal::type_mismatch(&item));
                                    }
                                }
                            }
                            None => {
                                return Err(FieldRefusal::required(&item));
                            }
                        },
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
                            if vals.is_empty() && schema.required {
                                return Err(FieldRefusal::required(&item));
                            }
                        }
                    }
                }
                Ok(())
            }
            (FieldType::TextEnum(options), FieldValue::TextEnum(vals)) => {
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
        }
    }
    pub fn format_field_value(
        &self,
        schema: &FieldSchema,
        composite_schemas: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    ) -> FieldValue {
        match (&schema.field_type, self) {
            (FieldType::Text(_), FieldValue::Text(_))
            | (FieldType::Slug(_), FieldValue::Text(_))
            | (FieldType::Markdown(_), FieldValue::Markdown(_))
            | (FieldType::Number, FieldValue::Number(_))
            | (FieldType::Boolean, FieldValue::Boolean(_))
            | (FieldType::Date, FieldValue::Date(_))
            | (FieldType::DateTime, FieldValue::DateTime(_))
            | (FieldType::Image, FieldValue::Image(_)) => self.clone(),
            (FieldType::CompositeField(s), FieldValue::CompositeField(value)) => match value {
                Some(v) => {
                    if s.id != v.id {
                        schema.get_default_value()
                    } else {
                        let composite_schema = composite_schemas.get(&s.id);
                        match composite_schema {
                            None => schema.get_default_value(),
                            Some(cs) => {
                                let formatted_values =
                                    v.values.format_to_schema(composite_schemas, &cs);
                                FieldValue::CompositeField(Some(CompositeFieldValue {
                                    id: s.id.clone(),
                                    values: formatted_values,
                                }))
                            }
                        }
                    }
                }
                None => schema.get_default_value(),
            },
            (FieldType::Array(schemas), FieldValue::Array(values)) => {
                let mut formatted_values = Vec::new();
                for value in values {
                    match value {
                        FieldValue::Text(_)
                        | FieldValue::Markdown(_)
                        | FieldValue::Number(_)
                        | FieldValue::Boolean(_)
                        | FieldValue::Date(_)
                        | FieldValue::DateTime(_)
                        | FieldValue::Image(_) => {
                            let item_type = value.get_type();
                            if schemas.contains(&item_type) {
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
                                        let composite_schema = composite_schemas.get(&schema.id);
                                        match composite_schema {
                                            None => continue,
                                            Some(cs) => {
                                                let formatted_values_map = cv.values.format_to_schema(
                                                    composite_schemas,
                                                    &cs,
                                                );
                                                formatted_values.push(FieldValue::CompositeField(
                                                    Some(CompositeFieldValue {
                                                        id: schema.id.clone(),
                                                        values: formatted_values_map,
                                                    }),
                                                ));
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
            (FieldType::TextEnum(options), FieldValue::TextEnum(vals)) => {
                let filtered_vals: Vec<String> = vals
                    .iter()
                    .filter(|v| options.contains(v))
                    .cloned()
                    .collect();
                FieldValue::TextEnum(filtered_vals)
            }
            _ => schema.get_default_value(),
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
            values: FieldValueMap(HashMap::from([(field_name.to_string(), FieldValue::Text(value.to_string()))]), PhantomData),
        }
    }

    fn create_composite_schemas_map() -> HashMap<CompositeFieldId, CompositeFieldSchema> {
        HashMap::from([
            (
                "comp_1".into(),
                create_composite_schema("sub_field", true),
            ),
            (
                "comp_2".into(),
                create_composite_schema("sub_field2", true),
            ),
        ])
    }

    #[test]
    fn test_field_schema_array_validation() {
        let composite_schemas = HashMap::new();
        let field_schema = FieldSchema {
            name: "test_array".to_string(),
            field_type: FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Number]),
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
                .validate_field_value(&field_schema,&composite_schemas)
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

    /// Reading is the other half of the round trip: an array of composites has to come back
    /// from JSON as the values it was written from.
    #[test]
    fn reads_an_array_of_composites() {
        let composite_schemas = create_composite_schemas_map();
        let field = FieldSchema {
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
            FieldValue::Array(vec![FieldValue::CompositeField(Some(create_composite_value(
                "comp_2",
                "sub_field2",
                "Value2"
            )))])
        );

        // A write may leave the wrapper off; then the first declared type that accepts the
        // object wins, which is the documented rule.
        let bare = json!([{ "sub_field": "Value1" }]);
        assert_eq!(
            parse_field_value(&field, &bare, &composite_schemas).unwrap(),
            FieldValue::Array(vec![FieldValue::CompositeField(Some(create_composite_value(
                "comp_1",
                "sub_field",
                "Value1"
            )))])
        );

        // A definition the array does not declare is not silently read as another one.
        let unknown = json!([{ "id": "comp_9", "values": { "sub_field": "Value1" } }]);
        assert!(parse_field_value(&field, &unknown, &composite_schemas)
            .unwrap_err()
            .contains("does not match any declared array item type"));
    }

    #[test]
    fn test_format_array_type_field() {
        let pattern = vec![
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Number]),
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
                                name: "sub_field2".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
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
    fn test_schema_serialization() {
        for field_type in FieldType::iter() {
            let (field_schema, json_schema) = match field_type {
                FieldType::Text(options) => {
                    let field_schema = FieldSchema {
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
                        name: "description".to_string(),
                        field_type: FieldType::Markdown(options),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema =
                        r#"{"name":"description","field_type":{"Markdown":{"max_length":null,"min_length":null}},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Slug(ref options) => {
                    let field_schema = FieldSchema {
                        name: "address".to_string(),
                        field_type: FieldType::Slug(options.clone()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    // A slug without a suggestion carries no options at all: the character rule
                    // and the length cap belong to the type, not to the schema.
                    let json_schema =
                        r#"{"name":"address","field_type":{"Slug":{}},"required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Number => {
                    let field_schema = FieldSchema {
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
                        name: "is_active".to_string(),
                        field_type: FieldType::Boolean,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema =
                        r#"{"name":"is_active","field_type":"Boolean","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Date => {
                    let field_schema = FieldSchema {
                        name: "create_date".to_string(),
                        field_type: FieldType::Date,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema =
                        r#"{"name":"create_date","field_type":"Date","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::DateTime => {
                    let field_schema = FieldSchema {
                        name: "update_time".to_string(),
                        field_type: FieldType::DateTime,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema =
                        r#"{"name":"update_time","field_type":"DateTime","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::Image => {
                    let field_schema = FieldSchema {
                        name: "profile_image".to_string(),
                        field_type: FieldType::Image,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    };
                    let json_schema =
                        r#"{"name":"profile_image","field_type":"Image","required":true,"width":12,"height":1}"#;
                    (field_schema, json_schema)
                }
                FieldType::CompositeField(_) => {
                    let field_schema = FieldSchema {
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
                FieldType::Array(_) => {
                    let field_schema = FieldSchema {
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Number]),
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
    fn test_field_value_serialization() {
        for field_type in FieldType::iter() {
            let (field_value, field_value_json) = match field_type {
                FieldType::Text(_) => (FieldValue::Text("Hello".to_string()), r#"{"Text":"Hello"}"#),
                FieldType::Slug(_) => (
                    FieldValue::Text("hello".to_string()),
                    r#"{"Text":"hello"}"#,
                ),
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
                FieldType::Image => (FieldValue::Image(Some(ImageId::from_u64(1))), r#"{"Image":1}"#),
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
    fn test_default_values() {
        for field_type in FieldType::iter() {
            let test_schema = FieldSchema {
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
                FieldType::TextEnum(_) => assert_eq!(default_value, FieldValue::TextEnum(vec![])),
            }
        }
        assert_eq!(FieldValue::default(), FieldValue::Text(String::new()));
        assert_eq!(FieldType::default(), FieldType::Text(TextFieldOptions::default()));
    }
    #[test]
    fn test_schema_validation_valid_values() {
        for field_type in FieldType::iter() {
            let (field_schema, composite_schemas, field_value) = match field_type {
                FieldType::Text(options) => (
                    FieldSchema {
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
                                name: "street".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
                                name: "city".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
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
                        values: FieldValueMap(HashMap::from([
                            (
                                "street".to_string(),
                                FieldValue::Text("123 Main St".to_string()),
                            ),
                            ("city".to_string(), FieldValue::Text("Anytown".to_string())),
                            ("zip".to_string(), FieldValue::Number(Some(12345.0))),
                        ]), PhantomData),
                    })),
                ),
                FieldType::Array(_) => (
                    FieldSchema {
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Number]),
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
            };
            let result = field_value.validate_field_value(&field_schema, &composite_schemas);
            assert!(result.is_ok(), "Failed on field type {:?}", field_type);
        }
    }
    #[test]
    fn test_field_type_test_required() {
        for field_type in FieldType::iter() {
            let value = match field_type {
                FieldType::Text(_) | FieldType::Slug(_) => {
                    FieldValue::Text("Sample".to_string())
                }
                FieldType::Markdown(_) => FieldValue::Markdown("**Sample**".to_string()),
                FieldType::Number => FieldValue::Number(Some(10.0)),
                FieldType::Boolean => FieldValue::Boolean(true),
                FieldType::Date => FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                FieldType::DateTime => FieldValue::DateTime(Some(
                    DateTime::parse_from_rfc3339("2023-01-01T12:00:00+00:00").unwrap(),
                )),
                FieldType::Image => FieldValue::Image(Some(ImageId::from_u64(1))),
                FieldType::CompositeField(_) => FieldValue::CompositeField(Some(CompositeFieldValue::default())),
                FieldType::Array(_) => FieldValue::Array(vec![FieldValue::Text("Item".to_string())]),
                FieldType::TextEnum(_) => FieldValue::TextEnum(vec!["Option".to_string()]),
            };
            assert!(field_type.test_required(&value), "Failed on field type {:?}", field_type);
        }
        assert!(!FieldType::Text(TextFieldOptions::default()).test_required(&FieldValue::Boolean(true)));

    }

    #[test]
    fn test_schema_validation_required_field_missing_value() {
        for field_type in FieldType::iter() {
            let (field_schema, field_value) = match field_type {
                FieldType::Text(options) => (
                    FieldSchema {
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
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Number]),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Array(vec![]),
                ),
                FieldType::TextEnum(_) => (
                    FieldSchema {
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
    fn test_schema_validation_non_required_field_missing_value() {
        for field_type in FieldType::iter() {
            let (field_schema, field_value) = match field_type {
                FieldType::Text(options) => (
                    FieldSchema {
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
                        name: "tags".to_string(),
                        field_type: FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Number]),
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldValue::Array(vec![]),
                ),
                FieldType::TextEnum(_) => (
                    FieldSchema {
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
    fn test_schema_validation_composite_field_edge_cases() {
        let pattern = vec![
            (
                FieldSchema {
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
                    values: FieldValueMap(HashMap::from_iter(vec![(
                        "sub_field".to_string(),
                        FieldValue::Text("test".to_string()),
                    )]), PhantomData),
                })),
                true,
            ),
            (
                FieldSchema {
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
                    values: FieldValueMap(HashMap::from_iter(vec![(
                        "sub_field".to_string(),
                        FieldValue::Text("".to_string()),
                    )]), PhantomData),
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
    fn test_validate_field_value() {
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
                FieldType::CompositeField(_) | FieldType::Array(_) | FieldType::TextEnum(_) => {
                    continue;
                }
            };
            let test_schema = FieldSchema {
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
                FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }),
                HashMap::new(),
                FieldValue::Text("Hello".to_string()),
                true,
                true,
            ),
            (
                FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }),
                HashMap::new(),
                FieldValue::Text("Hi".to_string()),
                true,
                false,
            ),
            (
                FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }),
                HashMap::new(),
                FieldValue::Text("Hello, World!".to_string()),
                true,
                false,
            ),
            (
                FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }),
                HashMap::new(),
                FieldValue::Text("".to_string()),
                true,
                false,
            ),
            (
                FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }),
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
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                        FieldSchema {
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
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                        FieldSchema {
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
                            name: "sub_field".to_string(),
                            field_type: FieldType::Text(TextFieldOptions::default()),
                            required: true,
                            width: 12,
                            height: 1,
                            unique: false,
                        },
                        FieldSchema {
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
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![]),
                false,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Boolean(true),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }), FieldType::Text(TextFieldOptions { max_length: Some(8), min_length: Some(3) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("length 9.".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(5) }), FieldType::Text(TextFieldOptions { max_length: Some(15), min_length: Some(7) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("length is 12".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions { max_length: None, min_length: Some(5) }), FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(7) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("length is 12".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: None }), FieldType::Text(TextFieldOptions { max_length: Some(10), min_length: Some(7) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Text("1".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions { max_length: Some(5), min_length: Some(3) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("1".to_string()),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions { max_length: Some(5), min_length: Some(3) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("length is 12".to_string()),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                    FieldValue::Number(Some(10.0)),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Number(Some(10.0)),
                    FieldValue::Number(Some(20.0)),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Markdown(TextFieldOptions { max_length: Some(10), min_length: None }), FieldType::Markdown(TextFieldOptions { max_length: Some(10), min_length: Some(7) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Markdown("Item1".to_string()),
                    FieldValue::Markdown("1".to_string()),
                ]),
                true,
                true,
            ),
            (
                FieldType::Array(vec![FieldType::Markdown(TextFieldOptions { max_length: Some(5), min_length: Some(3) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Markdown("1".to_string()),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Markdown(TextFieldOptions { max_length: Some(5), min_length: Some(3) })]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Markdown("length is 12".to_string()),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Text("Item1".to_string()),
                ]),
                true,
                false,
            ),
            (
                FieldType::Array(vec![FieldType::Boolean]),
                HashMap::new(),
                FieldValue::Array(vec![
                    FieldValue::Markdown("Item1".to_string()),
                ]),
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
                FieldValue::Array(vec![FieldValue::TextEnum(vec![])]),
                true,
                false,
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
    fn test_format_field_value() {
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
                                name: "sub_field".to_string(),
                                field_type: FieldType::Text(TextFieldOptions::default()),
                                required: true,
                                width: 12,
                                height: 1,
                                unique: false,
                            },
                            FieldSchema {
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
                    FieldType::Array(vec![FieldType::Text(TextFieldOptions::default()), FieldType::Boolean]),
                    HashMap::new(),
                    FieldValue::Array(vec![
                        FieldValue::Text("Item1".to_string()),
                        FieldValue::Number(Some(10.0)),
                    ]),
                    FieldValue::Array(vec![FieldValue::Text("Item1".to_string())]),
                ),
                FieldType::TextEnum(_) => (
                    FieldType::TextEnum(vec!["Option1".to_string()]),
                    HashMap::new(),
                    FieldValue::TextEnum(vec!["Option1".to_string(), "InvalidOption".to_string()]),
                    FieldValue::TextEnum(vec!["Option1".to_string()]),
                ),
            };
            let scheme = FieldSchema {
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
    fn test_get_field_type() {
        for value in FieldValue::iter() {
            let field_type = value.get_type();
            match value {
                FieldValue::Text(_) => assert_eq!(field_type, FieldType::Text(TextFieldOptions::default())),
                FieldValue::Markdown(_) => assert_eq!(field_type, FieldType::Markdown(TextFieldOptions::default())),
                FieldValue::Number(_) => assert_eq!(field_type, FieldType::Number),
                FieldValue::Boolean(_) => assert_eq!(field_type, FieldType::Boolean),
                FieldValue::Date(_) => assert_eq!(field_type, FieldType::Date),
                FieldValue::DateTime(_) => assert_eq!(field_type, FieldType::DateTime),
                FieldValue::Image(_) => assert_eq!(field_type, FieldType::Image),
                FieldValue::CompositeField(_) => {
                    assert_eq!(
                        field_type,
                        FieldType::CompositeField(CompositeFieldReference { id: CompositeFieldId::default() })
                    );
                }
                FieldValue::Array(_) => assert_eq!(field_type, FieldType::Array(vec![])),
                FieldValue::TextEnum(_) => assert_eq!(field_type, FieldType::TextEnum(vec![])),
            }
        }
    }
    #[test]
    fn test_format_to_schema() {
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            },
            FieldSchema {
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
            FieldSchema {
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
                name: "tags".to_string(),
                field_type: FieldType::Array(vec![
                    FieldType::CompositeField(CompositeFieldReference {
                        id: "tag_1".into(),
                    }),
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
                        name: "bio".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
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
                    name: "name".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                }],
            ),
        ]);
        let values = FieldValueMap(HashMap::from([
            ("title".to_string(), FieldValue::Text("Hello".to_string())),
            (
                "profile".to_string(),
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "profile_1".into(),
                    values: FieldValueMap(HashMap::from([(
                        "bio".to_string(),
                        FieldValue::Text("This is my bio".to_string()),
                    )]), PhantomData),
                })),
            ),
            (
                "tags".to_string(),
                FieldValue::Array(vec![
                    FieldValue::CompositeField(Some(CompositeFieldValue {
                        id: "tag_1".into(),
                        values: FieldValueMap(HashMap::from([(
                            "name".to_string(),
                            FieldValue::Text("rust".to_string()),
                        )]), PhantomData),
                    })),
                    FieldValue::Date(Some(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap())),
                    FieldValue::Text("programming".to_string()),
                ]),
            ),
        ]), PhantomData);
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
                    values: FieldValueMap(HashMap::from([(
                        "name".to_string(),
                        FieldValue::Text("rust".to_string())
                    )]), PhantomData),
                })),
                FieldValue::Text("programming".to_string()),
            ]))
        );
    }

    #[test]
    fn test_validate_to_schema() {
        let pattern = vec![
            (
                vec![
                    FieldSchema {
                        name: "title".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
                FieldValueMap(HashMap::from([
                    ("title".to_string(), FieldValue::Text("Hello".to_string())),
                    ("age".to_string(), FieldValue::Number(Some(25.0))),
                ]), PhantomData),
                true,
            ),
            (
                vec![
                    FieldSchema {
                        name: "title".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
                FieldValueMap(HashMap::from([("title".to_string(), FieldValue::Text("Hello".to_string()))]), PhantomData),
                true,
            ),
            (
                vec![
                    FieldSchema {
                        name: "title".to_string(),
                        field_type: FieldType::Text(TextFieldOptions::default()),
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                    FieldSchema {
                        name: "age".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                        width: 12,
                        height: 1,
                        unique: false,
                    },
                ],
                FieldValueMap(HashMap::from([("title".to_string(), FieldValue::Text("Hello".to_string()))]), PhantomData),
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
    fn test_field_type_enum_validation() {
        let field_type = FieldType::TextEnum(vec!["Option1".to_string(), "Option2".to_string()]);
        let schema = FieldSchema {
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
