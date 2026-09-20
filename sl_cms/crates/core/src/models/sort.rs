//! Putting a list in the order the caller asked for.
//!
//! One key, `-` in front for the other way round: `?sort=-updated_at`. A key is a piece of metadata
//! every item has (`id`, `published_at`, `created_at`, `updated_at`) or the name of a sortable
//! field. The id is always the tie-break - two items that compare equal keep a stable place, or a
//! client walking `next_offset` sees one twice and misses another.
//!
//! The values come in two shapes: the delivery API serves `DeliveryValue` and the management API
//! `FieldValueResponse`. Both are compared here, by the same rules, because "newest first" has to
//! mean the same thing on both screens.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate};

use crate::models::delivery::{DeliveredItem, DeliveryValue};
use crate::models::error::HttpError;
use crate::models::item_status::ItemMetadata;
use crate::models::schema::{FieldSchema, FieldType};
use crate::models::values::FieldValueResponse;

/// Which key a list is ordered by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    Id,
    PublishedAt,
    CreatedAt,
    UpdatedAt,
    Field(String),
}

/// An order a caller asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    key: SortKey,
    descending: bool,
}

impl Sort {
    /// Read `?sort=`, checked against the schema it will be read against.
    ///
    /// Refused rather than ignored: a caller that spelled a key wrong would otherwise be served
    /// the default order and never hear about it.
    pub fn parse(raw: Option<&str>, schema: &[FieldSchema]) -> Result<Option<Sort>, HttpError> {
        let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
            return Ok(None);
        };
        let (descending, name) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest.trim()),
            None => (false, raw),
        };
        let key = match name {
            "id" => SortKey::Id,
            "published_at" => SortKey::PublishedAt,
            "created_at" => SortKey::CreatedAt,
            "updated_at" => SortKey::UpdatedAt,
            other => {
                let Some(field) = schema.iter().find(|field| field.name == other) else {
                    return Err(HttpError::BadRequest(&format!(
                        "no field named '{other}' to sort by"
                    ))
                    .with_field(other));
                };
                if !sortable(&field.field_type) {
                    return Err(HttpError::BadRequest(&format!(
                        "field '{other}' cannot be sorted on"
                    ))
                    .with_field(other));
                }
                SortKey::Field(other.to_string())
            }
        };
        Ok(Some(Sort { key, descending }))
    }

    pub fn key(&self) -> &SortKey {
        &self.key
    }

    pub fn is_descending(&self) -> bool {
        self.descending
    }

    /// The field this order reads, when it reads one.
    pub fn field(&self) -> Option<&str> {
        match &self.key {
            SortKey::Field(name) => Some(name),
            _ => None,
        }
    }

    /// Whether the order is over when an item went live or changed, which lives in its metadata
    /// rather than in its values.
    pub fn is_metadata(&self) -> bool {
        matches!(
            self.key,
            SortKey::PublishedAt | SortKey::CreatedAt | SortKey::UpdatedAt
        )
    }
}

/// Whether a field holds something two items can be put in order by.
///
/// A reference, an image, an array or a composite does not: there is no one value to compare, and
/// guessing (the first element? the name?) would make an order nobody can explain.
fn sortable(field_type: &FieldType) -> bool {
    matches!(
        field_type,
        FieldType::Text(_)
            | FieldType::Slug(_)
            | FieldType::Markdown(_)
            | FieldType::Number
            | FieldType::Boolean
            | FieldType::Date
            | FieldType::DateTime
            | FieldType::TextEnum(_)
    )
}

/// What two values of one field compare as: numbers, dates, flags, or text without regard to case.
///
/// Nothing sorts before something in either direction, which is what "empty first" means when the
/// caller flips the order (`-title` puts the filled ones on top).
#[derive(PartialEq, PartialOrd)]
enum Comparable {
    Number(f64),
    Boolean(bool),
    Date(NaiveDate),
    Stamp(DateTime<FixedOffset>),
    Text(String),
}

fn compare_optional(a: Option<Comparable>, b: Option<Comparable>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        // Every variant is the same kind inside one field, so a cross-kind pair cannot happen;
        // `Equal` keeps that from being a panic if a schema change ever makes one.
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

/// The order two delivered items go in (see [`Sort`]).
pub fn compare_delivered(
    a: &SortableItem<DeliveredItem>,
    b: &SortableItem<DeliveredItem>,
    sort: &Sort,
) -> Ordering {
    compare_items(a, b, sort, comparable_delivered)
}

/// The order two management items go in: the same rules over `FieldValueResponse`.
pub fn compare_responses(
    a: &SortableItem<HashMap<String, FieldValueResponse>>,
    b: &SortableItem<HashMap<String, FieldValueResponse>>,
    sort: &Sort,
) -> Ordering {
    compare_items(a, b, sort, comparable_response)
}

/// One item as a list is being ordered: what it is called, when it changed, and what it holds.
pub type SortableItem<Values> = (u64, ItemMetadata, Values);

fn compare_items<Values>(
    a: &SortableItem<Values>,
    b: &SortableItem<Values>,
    sort: &Sort,
    comparable: impl Fn(Option<&Values>, &str) -> Option<Comparable>,
) -> Ordering {
    let order = match sort.key() {
        SortKey::Id => a.0.cmp(&b.0),
        SortKey::PublishedAt => a.1.published_at.cmp(&b.1.published_at),
        SortKey::CreatedAt => a.1.created_at.cmp(&b.1.created_at),
        SortKey::UpdatedAt => a.1.updated_at.cmp(&b.1.updated_at),
        SortKey::Field(name) => {
            compare_optional(comparable(Some(&a.2), name), comparable(Some(&b.2), name))
        }
    };
    let order = if sort.is_descending() {
        order.reverse()
    } else {
        order
    };
    order.then_with(|| a.0.cmp(&b.0))
}

fn comparable_delivered(values: Option<&DeliveredItem>, name: &str) -> Option<Comparable> {
    values
        .and_then(|values| values.get(name))
        .and_then(comparable_value)
}

fn comparable_value(value: &DeliveryValue) -> Option<Comparable> {
    match value {
        DeliveryValue::Number(n) => n.map(Comparable::Number),
        DeliveryValue::Boolean(flag) => Some(Comparable::Boolean(*flag)),
        DeliveryValue::Date(date) => date.map(Comparable::Date),
        DeliveryValue::DateTime(stamp) => stamp.map(Comparable::Stamp),
        DeliveryValue::Text(text) | DeliveryValue::Markdown(text) => {
            Some(Comparable::Text(text.to_lowercase()))
        }
        DeliveryValue::TextEnum(options) => {
            Some(Comparable::Text(options.join(",").to_lowercase()))
        }
        _ => None,
    }
}

fn comparable_response(
    values: Option<&HashMap<String, FieldValueResponse>>,
    name: &str,
) -> Option<Comparable> {
    let value = values.and_then(|values| values.get(name))?;
    match value {
        FieldValueResponse::Number(n) => n.map(Comparable::Number),
        FieldValueResponse::Boolean(flag) => Some(Comparable::Boolean(*flag)),
        FieldValueResponse::Date(date) => date.map(Comparable::Date),
        FieldValueResponse::DateTime(stamp) => stamp.map(Comparable::Stamp),
        FieldValueResponse::Text(text) | FieldValueResponse::Markdown(text) => {
            Some(Comparable::Text(text.to_lowercase()))
        }
        FieldValueResponse::TextEnum(options) => {
            Some(Comparable::Text(options.join(",").to_lowercase()))
        }
        _ => None,
    }
}
