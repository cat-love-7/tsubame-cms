//! What the delivery API serves.
//!
//! The management API's shape, with the two things a site needs and an editor does not:
//!
//! * a reference that is not published is **gone** - the site has nothing to point at, and the
//!   rule that a *required* relation has to have a published target is what keeps that from
//!   silently emptying a field (see `repositories::relation_rules`);
//! * a field a client named in `?populate=` carries the target's published values, one level deep
//!   (see `docs/relations-design.md` §5).
//!
//! A separate tree rather than `FieldValueResponse` because a reference here may hold values and
//! there may not: the management shape is a *stored* value, and what a site is served is a
//! rendering of one. Every other kind of field, images included, renders the same way in both.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, FixedOffset, NaiveDate};

use crate::models::error::{HttpError, map_internal_error};
use crate::models::image::{Image, ImageId, ImageResponse};
use crate::models::owner::ItemOwner;
use crate::models::schema::{CompositeFieldId, CompositeFieldSchema, FieldSchema, FieldType};
use crate::models::values::{FieldValue, FieldValueMap, FieldValueResponse};
use crate::repositories::content_reader::ContentReader;
use crate::repositories::relation_repository::RelationRepository;

/// One item's values as the site is served them.
pub type DeliveredItem = HashMap<String, DeliveryValue>;

/// One `?populate=` entry: a field name (or an `inverse_name`), and - when the field declares
/// several targets - which of them to expand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopulateEntry {
    /// The field name, or the `inverse_name` of the relation pointing here.
    pub name: String,
    /// The target named after a dot: `?populate=related.authors`. Needed when the field declares
    /// more than one target, where the bare name is refused; on a field with one target it is
    /// accepted and means the same as the bare name, which already says it.
    pub target: Option<String>,
}

impl PopulateEntry {
    /// How the entry reads in a refusal, with the target it named when it named one.
    pub fn describe(&self) -> String {
        match &self.target {
            None => self.name.clone(),
            Some(target) => format!("{}.{}", self.name, target),
        }
    }
}

/// What `?populate=` asked for: the names to expand, one level deep.
#[derive(Debug, Clone, Default)]
pub struct Populate(Vec<PopulateEntry>);

impl Populate {
    /// Read the query parameter: a comma-separated list, blank entries dropped.
    ///
    /// `field.target` names one target of a field that declares several, which is the only way to
    /// say which one a client meant.
    pub fn parse(raw: Option<&str>) -> Self {
        Populate(
            raw.unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .filter_map(|entry| match entry.split_once('.') {
                    None => Some(PopulateEntry {
                        name: entry.to_string(),
                        target: None,
                    }),
                    Some((name, target)) => {
                        let (name, target) = (name.trim(), target.trim());
                        (!name.is_empty() && !target.is_empty()).then(|| PopulateEntry {
                            name: name.to_string(),
                            target: Some(target.to_string()),
                        })
                    }
                })
                .collect(),
        )
    }

    /// Whether a client asked for this field, with or without naming one of its targets.
    pub fn asks(&self, name: &str) -> bool {
        self.0.iter().any(|entry| entry.name == name)
    }

    /// What the request asks to expand for one field.
    pub fn ask_for(&self, name: &str) -> Ask<'_> {
        Ask {
            entries: self.0.iter().filter(|entry| entry.name == name).collect(),
        }
    }

    /// Nothing asked for: what an expansion inside an expansion uses, because the depth is one.
    pub fn none() -> Self {
        Populate(Vec::new())
    }

    /// The entries asked for, one by one, so a caller can split them itself.
    pub fn iter(&self) -> impl Iterator<Item = &PopulateEntry> {
        self.0.iter()
    }

    /// Just the entries this schema relates through: what a forward expansion is given.
    pub fn forward_only(&self, schema: &[FieldSchema]) -> Populate {
        Populate(
            self.0
                .iter()
                .filter(|entry| relates_through(schema, entry))
                .cloned()
                .collect(),
        )
    }
}

/// Whether this schema relates through the entry: a field (or an array of relations) of that name,
/// declaring the target the entry named - or any target, when it named none.
fn relates_through(schema: &[FieldSchema], entry: &PopulateEntry) -> bool {
    schema.iter().any(|field| {
        field.name == entry.name
            && targets_of(&field.field_type).iter().any(|target| {
                entry
                    .target
                    .as_deref()
                    .is_none_or(|asked| asked == target.name())
            })
    })
}

/// The targets a field may reference: one for a relation, several for an array of them.
pub fn targets_of(field_type: &FieldType) -> Vec<&crate::models::schema::RelationTarget> {
    match field_type {
        FieldType::Relation(options) => vec![&options.target],
        FieldType::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                FieldType::Relation(options) => Some(&options.target),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// What the request asked to expand for one field.
#[derive(Debug, Clone, Default)]
pub struct Ask<'a> {
    entries: Vec<&'a PopulateEntry>,
}

impl Ask<'_> {
    /// Whether the request asked to expand a reference to this target.
    pub fn wants(&self, target: &crate::models::schema::RelationTarget) -> bool {
        self.entries.iter().any(|entry| match &entry.target {
            None => true,
            Some(name) => name == target.name(),
        })
    }

    /// Whether the field was named on its own, which only says which target while the field
    /// declares one.
    pub fn bare(&self) -> bool {
        self.entries.iter().any(|entry| entry.target.is_none())
    }

    /// Whether anything at all was asked for this field.
    pub fn any(&self) -> bool {
        !self.entries.is_empty()
    }
}

/// One value as the delivery API serves it.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum DeliveryValue {
    Text(String),
    Markdown(String),
    Number(Option<f64>),
    Boolean(bool),
    Date(Option<NaiveDate>),
    DateTime(Option<DateTime<FixedOffset>>),
    Image(Option<ImageResponse>),
    CompositeField(Option<DeliveryComposite>),
    /// A `Relation` field: one reference, or `null` for none.
    Relation(Option<DeliveryReference>),
    /// `?populate=<inverse_name>`: the content referring to this, which is a list whatever the
    /// referring field holds.
    RelationList(Vec<DeliveryReference>),
    Array(Vec<DeliveryValue>),
    TextEnum(Vec<String>),
}

#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct DeliveryComposite {
    pub id: CompositeFieldId,
    pub values: HashMap<String, DeliveryValue>,
}

/// What a relation points at, and - when `?populate=` named the field - what it holds.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct DeliveryReference {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
    /// The published values of the target, one level deep: present only when asked for, and only
    /// for a target that is published (which every reference here is).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<DeliveredItem>,
}

/// What this module needs to read while it renders: the site's shape and its images.
struct Delivery<'a> {
    composites: &'a HashMap<CompositeFieldId, CompositeFieldSchema>,
    images: &'a HashMap<ImageId, Image>,
    reader: &'a dyn ContentReader,
}

/// Recursion goes through a boxed future: an expansion renders another item's values, which may
/// hold an array of composites, which the same walker renders (see `deliver_value`).
type DeliveredValuesFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DeliveredItem, HttpError>> + Send + 'a>>;
type DeliveredValueFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DeliveryValue, HttpError>> + Send + 'a>>;

/// The values a site is served: unpublished references dropped, the named fields expanded.
fn deliver_values<'a>(
    schema: &'a [FieldSchema],
    values: &'a HashMap<String, FieldValueResponse>,
    delivery: &'a Delivery<'a>,
    populate: &'a Populate,
) -> DeliveredValuesFuture<'a> {
    Box::pin(async move {
        let mut delivered = HashMap::new();
        for field in schema {
            let Some(value) = values.get(&field.name) else {
                continue;
            };
            let delivered_value = deliver_value(
                &field.field_type,
                value,
                delivery,
                &populate.ask_for(&field.name),
            )
            .await?;
            delivered.insert(field.name.clone(), delivered_value);
        }
        Ok(delivered)
    })
}

/// One value, with the schema that gives it meaning.
fn deliver_value<'a>(
    field_type: &'a FieldType,
    value: &'a FieldValueResponse,
    delivery: &'a Delivery<'a>,
    ask: &'a Ask<'a>,
) -> DeliveredValueFuture<'a> {
    Box::pin(async move {
        Ok(match (field_type, value) {
            (_, FieldValueResponse::Text(text)) => DeliveryValue::Text(text.clone()),
            (_, FieldValueResponse::Markdown(text)) => DeliveryValue::Markdown(text.clone()),
            (_, FieldValueResponse::Number(number)) => DeliveryValue::Number(*number),
            (_, FieldValueResponse::Boolean(flag)) => DeliveryValue::Boolean(*flag),
            (_, FieldValueResponse::Date(date)) => DeliveryValue::Date(*date),
            (_, FieldValueResponse::DateTime(stamp)) => DeliveryValue::DateTime(*stamp),
            (_, FieldValueResponse::Image(image)) => DeliveryValue::Image(image.clone()),
            (_, FieldValueResponse::TextEnum(options)) => DeliveryValue::TextEnum(options.clone()),
            (FieldType::Relation(options), FieldValueResponse::Relation(reference)) => {
                let expand = ask.wants(&options.target);
                DeliveryValue::Relation(
                    deliver_reference(reference.as_ref(), delivery, expand).await?,
                )
            }
            (
                FieldType::CompositeField(definition),
                FieldValueResponse::CompositeField(Some(composite)),
            ) => {
                let Some(nested_schema) = delivery.composites.get(&definition.id) else {
                    return Ok(DeliveryValue::CompositeField(None));
                };
                let values = deliver_values(
                    nested_schema,
                    &composite.values,
                    delivery,
                    &Populate::none(),
                )
                .await?;
                DeliveryValue::CompositeField(Some(DeliveryComposite {
                    id: composite.id.clone(),
                    values,
                }))
            }
            (_, FieldValueResponse::CompositeField(None)) => DeliveryValue::CompositeField(None),
            (FieldType::Array(items), FieldValueResponse::Array(elements)) => {
                // An array of relations is how several references are written, so it is the field
                // the request named - but only the client knows which of its targets it meant when
                // it declares more than one.
                refuse_bare_name_among_several(items, ask)?;
                let mut delivered = Vec::with_capacity(elements.len());
                for element in elements {
                    // The declared item types are untyped on the wire, so an element is rendered
                    // by the type it turned out to be: a composite names its own definition, a
                    // relation names its target, and a scalar does not care which type declared it.
                    let element_type = element_type_of(items, element);
                    delivered.push(deliver_value(&element_type, element, delivery, ask).await?);
                }
                DeliveryValue::Array(delivered)
            }
            // The schema and the stored value disagree: the value is served as it is rather than
            // hidden, which is what the management API does too.
            (_, other) => delivery_of_untyped(other),
        })
    })
}

/// Refuse `?populate=<field>` when the field declares several targets: the name does not say which
/// one the client meant, and guessing would serve a different answer than it asked for.
fn refuse_bare_name_among_several(items: &[FieldType], ask: &Ask<'_>) -> Result<(), HttpError> {
    if !ask.bare() {
        return Ok(());
    }
    let names: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            FieldType::Relation(options) => Some(options.target.name()),
            _ => None,
        })
        .collect();
    if names.len() < 2 {
        return Ok(());
    }
    Err(HttpError::BadRequest(&format!(
        "this field references several targets; name one of them (?populate=field.{})",
        names.join(" or field.")
    )))
}

/// The declared item type an element turned out to be.
fn element_type_of(items: &[FieldType], element: &FieldValueResponse) -> FieldType {
    // A reference names its own target, which is what picks the declaration it belongs to - and
    // with several targets declared, that is the only thing that can.
    if let FieldValueResponse::Relation(reference) = element {
        let wanted = reference
            .as_ref()
            .map(|reference| reference.target.as_str());
        let declared = items.iter().find(|item| match item {
            FieldType::Relation(options) => match wanted {
                Some(target) => options.target.name() == target,
                None => true,
            },
            _ => false,
        });
        if let Some(declared) = declared {
            return declared.clone();
        }
    }
    let FieldValueResponse::CompositeField(Some(composite)) = element else {
        return FieldType::Number;
    };
    items
        .iter()
        .find(|item| match item {
            FieldType::CompositeField(reference) => reference.id == composite.id,
            _ => false,
        })
        .cloned()
        .unwrap_or(FieldType::Number)
}

/// One reference a site can follow, or `None` when there is none or its target is not published.
async fn deliver_reference(
    reference: Option<&crate::models::values::RelationRef>,
    delivery: &Delivery<'_>,
    expand: bool,
) -> Result<Option<DeliveryReference>, HttpError> {
    let Some(reference) = reference else {
        return Ok(None);
    };
    let delivered = deliver_references(std::slice::from_ref(reference), delivery, expand).await?;
    Ok(delivered.into_iter().next())
}

/// The references a site can follow: the target has to be published, and its values come along
/// only when the field was named in `?populate=`.
async fn deliver_references(
    references: &[crate::models::values::RelationRef],
    delivery: &Delivery<'_>,
    expand: bool,
) -> Result<Vec<DeliveryReference>, HttpError> {
    let mut delivered = Vec::with_capacity(references.len());
    for reference in references {
        let Some(content) = delivery
            .reader
            .read_content(&reference.target_owner())
            .await
            .map_err(map_internal_error)?
        else {
            continue;
        };
        if !content.published {
            continue;
        }
        let values = match (expand, content.published_values.as_ref()) {
            (true, Some(published)) => {
                // The target's own schema names its fields, and one level is the depth: nothing
                // inside the expansion is expanded again.
                let formatted = published
                    .format_to_schema(delivery.composites, &content.schema)
                    .to_response(delivery.images);
                Some(
                    deliver_values(&content.schema, &formatted, delivery, &Populate::none())
                        .await?,
                )
            }
            _ => None,
        };
        delivered.push(DeliveryReference {
            target: reference.target.clone(),
            item: reference.item,
            values,
        });
    }
    Ok(delivered)
}

/// A value whose schema does not describe it: rendered the way the management API renders it.
fn delivery_of_untyped(value: &FieldValueResponse) -> DeliveryValue {
    match value {
        FieldValueResponse::Text(text) => DeliveryValue::Text(text.clone()),
        FieldValueResponse::Markdown(text) => DeliveryValue::Markdown(text.clone()),
        FieldValueResponse::Number(number) => DeliveryValue::Number(*number),
        FieldValueResponse::Boolean(flag) => DeliveryValue::Boolean(*flag),
        FieldValueResponse::Date(date) => DeliveryValue::Date(*date),
        FieldValueResponse::DateTime(stamp) => DeliveryValue::DateTime(*stamp),
        FieldValueResponse::Image(image) => DeliveryValue::Image(image.clone()),
        FieldValueResponse::TextEnum(options) => DeliveryValue::TextEnum(options.clone()),
        FieldValueResponse::Relation(reference) => {
            DeliveryValue::Relation(reference.as_ref().map(|reference| DeliveryReference {
                target: reference.target.clone(),
                item: reference.item,
                values: None,
            }))
        }
        FieldValueResponse::CompositeField(Some(composite)) => {
            DeliveryValue::CompositeField(Some(DeliveryComposite {
                id: composite.id.clone(),
                values: composite
                    .values
                    .iter()
                    .map(|(name, value)| (name.clone(), delivery_of_untyped(value)))
                    .collect(),
            }))
        }
        FieldValueResponse::CompositeField(None) => DeliveryValue::CompositeField(None),
        FieldValueResponse::Array(elements) => {
            DeliveryValue::Array(elements.iter().map(delivery_of_untyped).collect())
        }
    }
}

/// The published content that points at `owner`, under the name the other side calls the
/// relation (`inverse_name`).
///
/// `None` when nothing calls the relation `name`: the caller turns that into a refusal, because a
/// name nothing answers is a typo rather than an empty set. A name the site does declare is an
/// empty list when nothing references the item yet, which is not an error - a build asking about a
/// category with no articles is a normal question.
///
/// The index names the candidates and their schemas say what they call the relation; the referrer
/// has to be published *and* its published copy has to hold the reference, which is the same
/// "what the site serves" rule the forward direction follows.
pub async fn inverse_references(
    owner: &ItemOwner,
    name: &str,
    limit: usize,
    delivery: &DeliveryMaps,
    reader: &dyn ContentReader,
    relations: &dyn RelationRepository,
) -> Result<Option<Vec<DeliveryReference>>, HttpError> {
    let referrers = relations
        .get_relation_references(owner)
        .await
        .map_err(map_internal_error)?;
    let mut named = false;
    let mut delivered = Vec::new();
    for referrer in referrers {
        let Some(content) = reader
            .read_content(&referrer)
            .await
            .map_err(map_internal_error)?
        else {
            continue;
        };
        let calls_it = content
            .schema
            .iter()
            .any(|field| declares_inverse_of(&field.field_type, name, owner));
        if !calls_it {
            continue;
        }
        named = true;
        if !content.published {
            continue;
        }
        let Some(published) = content.published_values.as_ref() else {
            continue;
        };
        // The published copy is what decides: an editor may have already taken the reference out.
        if !holds_reference(published, owner) {
            continue;
        }
        let formatted = published
            .format_to_schema(&delivery.composites, &content.schema)
            .to_response(&delivery.images);
        let values = delivery
            .deliver(&content.schema, &formatted, reader, &Populate::none())
            .await?;
        if delivered.len() >= limit {
            break;
        }
        delivered.push(DeliveryReference {
            target: referrer.name.clone(),
            item: referrer.item,
            values: Some(values),
        });
    }
    if !named {
        // No referrer declares it, and an item nothing references cannot answer whether the name
        // exists at all: ask the site, because answering "empty" to a typo is the failure this
        // refusal exists to prevent.
        if !reader
            .declares_inverse(name)
            .await
            .map_err(map_internal_error)?
        {
            return Ok(None);
        }
    }
    Ok(Some(delivered))
}

/// Whether a field type answers to this inverse name for a reference to `owner`.
///
/// The item types of an array are relations like any other, so an array of relations calls the
/// relation whatever its own item types say - which is what makes `?populate=<inverse_name>` work
/// for several references the way it does for one (see `docs/relations-design.md` §3).
fn declares_inverse_of(field_type: &FieldType, name: &str, owner: &ItemOwner) -> bool {
    match field_type {
        FieldType::Relation(options) => {
            options.inverse_name.as_deref() == Some(name)
                && options.target.name() == owner.name
                && options.target.is_single_page() == owner.is_single_page()
        }
        FieldType::Array(items) => items
            .iter()
            .any(|item| declares_inverse_of(item, name, owner)),
        _ => false,
    }
}

/// Whether a set of values points at this content anywhere, composites and arrays included.
fn holds_reference(values: &FieldValueMap<Vec<FieldSchema>>, owner: &ItemOwner) -> bool {
    values
        .0
        .values()
        .any(|value| holds_reference_in(value, owner))
}

fn holds_reference_in(value: &FieldValue, owner: &ItemOwner) -> bool {
    match value {
        FieldValue::Relation(references) => references
            .iter()
            .any(|reference| &reference.target_owner() == owner),
        FieldValue::CompositeField(Some(composite)) => composite
            .values
            .0
            .values()
            .any(|nested| holds_reference_in(nested, owner)),
        FieldValue::Array(elements) => elements
            .iter()
            .any(|element| holds_reference_in(element, owner)),
        _ => false,
    }
}

/// How many pieces of content an inverse expansion may bring back when the request does not say
/// (see `docs/relations-design.md` §5: a big set is read through the filter, which pages).
pub const DEFAULT_INVERSE_LIMIT: usize = 25;

/// What a delivery request asked for.
#[derive(Debug, Clone)]
pub struct Expansion {
    pub populate: Populate,
    /// The cap on one inverse expansion. A route with a page of its own takes `?limit=` for the
    /// page, so its inverse expansions use the default.
    pub inverse_limit: usize,
}

impl Default for Expansion {
    fn default() -> Self {
        Expansion {
            populate: Populate::none(),
            inverse_limit: DEFAULT_INVERSE_LIMIT,
        }
    }
}

impl Expansion {
    /// A request that asked for `populate` names, with the default inverse cap.
    pub fn of(populate: Populate) -> Self {
        Expansion {
            populate,
            inverse_limit: DEFAULT_INVERSE_LIMIT,
        }
    }

    /// The same, with the cap the request asked for.
    pub fn capped_at(mut self, limit: Option<usize>) -> Self {
        if let Some(limit) = limit {
            self.inverse_limit = limit;
        }
        self
    }
}

/// One relation of a collection's schema, and the content it has to point at: what `?where=` says.
#[derive(Debug, Clone)]
pub struct RelationFilter {
    pub field: String,
    pub owner: ItemOwner,
}

/// Whether the relation field `field` of these values points at `owner`.
///
/// Both shapes a relation field takes are walked: one reference, or several as an `Array` of them
/// (`Array([Relation(…)])`). Which of the field's targets counts was settled when the filter was
/// parsed, so an element is compared by the owner it names.
pub fn field_holds(
    values: &FieldValueMap<Vec<FieldSchema>>,
    field: &str,
    owner: &ItemOwner,
) -> bool {
    match values.0.get(field) {
        Some(FieldValue::Relation(references)) => references
            .iter()
            .any(|reference| &reference.target_owner() == owner),
        Some(FieldValue::Array(elements)) => elements.iter().any(|element| match element {
            FieldValue::Relation(references) => references
                .iter()
                .any(|reference| &reference.target_owner() == owner),
            _ => false,
        }),
        _ => false,
    }
}

/// What the delivery walker needs, built where the maps are already read.
///
/// Public because the services own those reads (one image query per request, one list of
/// composite definitions), and private would mean reading them again per value.
pub struct DeliveryMaps {
    pub composites: HashMap<CompositeFieldId, CompositeFieldSchema>,
    pub images: HashMap<ImageId, Image>,
}

impl DeliveryMaps {
    /// The values of one item, ready to be served.
    pub async fn deliver<'a>(
        &'a self,
        schema: &[FieldSchema],
        values: &HashMap<String, FieldValueResponse>,
        reader: &'a dyn ContentReader,
        populate: &Populate,
    ) -> Result<DeliveredItem, HttpError> {
        let delivery = Delivery {
            composites: &self.composites,
            images: &self.images,
            reader,
        };
        deliver_values(schema, values, &delivery, populate).await
    }
}
