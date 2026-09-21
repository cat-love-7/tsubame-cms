use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::models::error::FieldRefusal;
use crate::models::identity::StringId;

use super::values::FieldValue;
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
    /// Whether the collection's list shows this field as a column.
    ///
    /// A list is the screen an editor scans to find one item, and a schema of a dozen fields makes a
    /// table nobody can read. Which fields identify an item is the schema author's business, so it
    /// is a property of the field - like its width, which is about the other screen.
    ///
    /// A schema that marks **no** field is not a table of nothing: the list then shows every field,
    /// which is what it did before this existed (see the frontend's `listColumns`). Only collections
    /// have a list, so the pages' and composites' schemas never set it.
    ///
    /// Omitted from the wire when false, like `unique`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_in_list: bool,
    /// Whether this is the field a reference to this item shows as its name.
    ///
    /// A reference is stored as which item it points at, and an id says nothing to a reader - so a
    /// collection says which of its fields names an item ("the category's name", "the article's
    /// title"), and the screens that show a reference show that. At most **one** field per schema
    /// may be the title, and only a field that reads as one line (a body or an image is not a name).
    ///
    /// Omitted from the wire when false, like `unique`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_title: bool,
    /// in colspan units (1-12)
    pub width: u32,
    /// in rowspan units (1-)
    pub height: u32,
}
/// `#[serde(skip_serializing_if)]` takes a predicate, and `bool` has no `is_false`.
fn is_false(value: &bool) -> bool {
    !*value
}

/// What a schema is told about itself, as opposed to what it says about its values.
///
/// Held apart from the fields on purpose. A schema's fields are read by almost every screen and by
/// the delivery API's composite route, and none of them has any use for these; a separate record
/// also means a setting can be added without rewriting the definition every item's values are
/// checked against, and an adapter that has never been told about one answers the default.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaSettings {
    /// Whether a preview link may be minted for this schema's working copies.
    ///
    /// Off until an administrator turns it on. A preview link is a URL that shows unpublished
    /// content to whoever holds it, and the signature is the only thing guarding it - so a
    /// deployment that renders previews at all still decides schema by schema, rather than every
    /// collection becoming shareable the moment a preview site exists.
    #[serde(default)]
    pub preview: bool,
}

pub type CompositeFieldId = StringId<CompositeFieldSchema>;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CompositeFieldReference {
    pub id: CompositeFieldId,
}

pub type CompositeFieldSchema = Vec<FieldSchema>;

/// What a slug field can be told.
///
/// A slug has no options in the usual sense: the character rule and the length cap belong to the
/// type (`models::slug`), so that every slug in every collection means the same thing. What is left
/// is the editor's convenience - which field to suggest as the source of the value.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SlugOptions {
    /// The field an editor is offered to generate the slug from, usually a title.
    ///
    /// Checked when the schema is saved: it has to name another text field in the same schema.
    /// Nothing is generated without an editor pressing the button - a title that changes should
    /// not silently move a URL that is already published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_from: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, Copy)]
pub struct TextFieldOptions {
    pub max_length: Option<usize>,
    pub min_length: Option<usize>,
}

/// What a relation points at.
///
/// The names are strings rather than the typed name structs so this module stays free of the
/// collection and page modules; the schema save is what checks them against what exists (see
/// [`validate_relation_targets`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RelationTarget {
    Collection { name: String },
    SinglePage { name: String },
}

impl Default for RelationTarget {
    /// A new relation field starts as "points at a collection", with the name still to be chosen.
    fn default() -> Self {
        RelationTarget::Collection {
            name: String::new(),
        }
    }
}

impl RelationTarget {
    /// The collection or page name, whichever this is.
    pub fn name(&self) -> &str {
        match self {
            RelationTarget::Collection { name } | RelationTarget::SinglePage { name } => name,
        }
    }

    /// Whether this points at a single page, which cannot hold several references.
    pub fn is_single_page(&self) -> bool {
        matches!(self, RelationTarget::SinglePage { .. })
    }
}

/// What a relation field is told: what it points at, whether it may point at several things, and
/// what the other side is called.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct RelationOptions {
    pub target: RelationTarget,
    /// Whether several may be referenced at once.
    ///
    /// A single page is one item whose whole identity is its name, so it can only ever be a
    /// single reference: a field pointing at two pages would be two fields.
    #[serde(default)]
    pub has_many: bool,
    /// What the other side is called in the screens and in `?populate=`.
    ///
    /// A **label**, not a second field: nothing about the relation is stored on the target, and
    /// the inverse direction is always answered from the index. It exists so a category's screen
    /// can say "articles" without the schema having to define a field called that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse_name: Option<String>,
}

impl RelationOptions {
    /// Whether this holds at most one reference, which is what a value is checked against.
    ///
    /// A page target is always one, whatever `has_many` says - which `validate_relation_options`
    /// refuses to store in the first place, so this stays true about a stored schema.
    pub fn is_single(&self) -> bool {
        !self.has_many || self.target.is_single_page()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(EnumIter))]
pub enum FieldType {
    Text(TextFieldOptions),
    /// A URL-safe, normalised, always-unique string (see `models::slug`).
    Slug(SlugOptions),
    Markdown(TextFieldOptions),
    Number,
    Boolean,
    Date,
    DateTime,
    Image,
    CompositeField(CompositeFieldReference),
    /// A reference to another collection's items, or to a single page.
    Relation(RelationOptions),
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
            (FieldType::Text(_) | FieldType::Slug(_), FieldValue::Text(text)) => !text.is_empty(),
            (FieldType::Markdown(_), FieldValue::Markdown(markdown)) => !markdown.is_empty(),
            (FieldType::Number, FieldValue::Number(num_opt)) => num_opt.is_some(),
            (FieldType::Boolean, FieldValue::Boolean(_)) => true,
            (FieldType::Date, FieldValue::Date(date_opt)) => date_opt.is_some(),
            (FieldType::DateTime, FieldValue::DateTime(date_time_opt)) => date_time_opt.is_some(),
            (FieldType::Image, FieldValue::Image(img_opt)) => img_opt.is_some(),
            (FieldType::CompositeField(_), FieldValue::CompositeField(value)) => value.is_some(),
            (FieldType::Relation(_), FieldValue::Relation(refs)) => !refs.is_empty(),
            (FieldType::Array(_), FieldValue::Array(values)) => !values.is_empty(),
            (FieldType::TextEnum(_), FieldValue::TextEnum(vals)) => !vals.is_empty(),
            _ => false,
        }
    }
}
impl FieldSchema {
    /// Whether this field's values are tracked for uniqueness.
    ///
    /// A field that asked for it, or a slug - which is unique by being a slug: `Hello World` and
    /// `hello-world` are one slug, so the index is what keeps them from becoming two items.
    pub fn is_unique(&self) -> bool {
        self.unique || self.is_slug()
    }

    /// Whether this field is a slug (see `models::slug`).
    pub fn is_slug(&self) -> bool {
        matches!(self.field_type, FieldType::Slug(_))
    }

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
            FieldType::Text(_) | FieldType::Slug(_) => FieldValue::Text(String::new()),
            FieldType::Markdown(_) => FieldValue::Markdown(String::new()),
            FieldType::Number => FieldValue::Number(None),
            FieldType::Boolean => FieldValue::Boolean(false),
            FieldType::Date => FieldValue::Date(None),
            FieldType::DateTime => FieldValue::DateTime(None),
            FieldType::Image => FieldValue::Image(None),
            FieldType::CompositeField(_) => FieldValue::CompositeField(None),
            FieldType::Relation(_) => FieldValue::Relation(vec![]),
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
    // The title seen so far, so a second one can name the first.
    let mut title_so_far: Option<String> = None;
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
            if field.is_slug() {
                // Not a mistake worth refusing outright - the flag is simply already true - but
                // saying so beats a schema that reads as if the two were unrelated.
                return Err(format!(
                    "field '{name}': a slug is unique already, so 'unique' is redundant"
                ));
            }
            if !matches!(field.field_type, FieldType::Text(_)) {
                return Err(format!("field '{name}': only a text field can be unique"));
            }
        }
        if field.is_slug() {
            match scope {
                SchemaScope::Collection => {}
                SchemaScope::SinglePage => {
                    return Err(format!(
                        "field '{name}': a single page's address is its name, so it needs no slug"
                    ));
                }
                SchemaScope::CompositeDefinition => {
                    return Err(format!(
                        "field '{name}': a composite's values are stored inside the item that uses                          them, so a slug there would name nothing"
                    ));
                }
            }
            validate_slug_options(fields, field)?;
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
        validate_relation_options(name, &field.field_type)?;
        if field.is_title {
            if !readable_as_a_title(&field.field_type) {
                return Err(format!(
                    "field '{name}': a reference to this item would show this field as its name,                      and a {} cannot be read as one line",
                    field_type_name(&field.field_type)
                ));
            }
            if let Some(other) = title_so_far {
                return Err(format!(
                    "field '{name}': '{other}' is already the title of this schema, and an item has                      one name"
                ));
            }
            title_so_far = Some(name.to_string());
        }
        // A relation inside a composite definition is allowed, and points at a collection or a
        // page of the site - the definition is stored on its own and reused, so there is nothing
        // about the embedder for its target to be relative to. What has to be checked is the
        // target: see `referenced_relation_targets`, which walks into the definitions a schema
        // embeds, and the composite save, which checks its own targets as it stores them.
    }
    Ok(())
}

/// A relation has to say what it points at, and a single page can only be a single reference.
///
/// Whether that target *exists* is checked where the collections and pages are known (the schema
/// save in the API), because this module deliberately knows nothing about them.
fn validate_relation_options(name: &str, field_type: &FieldType) -> Result<(), String> {
    match field_type {
        FieldType::Relation(options) => {
            if options.target.name().trim().is_empty() {
                return Err(format!(
                    "field '{name}': a relation must name the collection or single page it points at"
                ));
            }
            if options
                .inverse_name
                .as_deref()
                .is_some_and(|inverse| inverse.trim().is_empty())
            {
                return Err(format!(
                    "field '{name}': the inverse name must not be blank - leave it out instead"
                ));
            }
            if options.target.is_single_page() && options.has_many {
                return Err(format!(
                    "field '{name}': a single page is one item, so a relation to one can only be a \
                     single reference"
                ));
            }
            Ok(())
        }
        FieldType::Array(items) => {
            // An array cannot hold one (see `validate_field_type`), for the same reason the walk
            // above looks inside them.
            for item in items {
                validate_relation_options(name, item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Whether a value of this type reads as a name: one line, in a list or next to a reference.
///
/// A Markdown body is allowed because a short one is a name and the rendering takes the text as it
/// is; an image, a composite, a relation and an array are not, because there is no one line that
/// stands for them.
fn readable_as_a_title(field_type: &FieldType) -> bool {
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

/// What a field's type is called when a refusal has to name it.
fn field_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::Text(_) => "text field",
        FieldType::Slug(_) => "slug",
        FieldType::Markdown(_) => "Markdown field",
        FieldType::Number => "number",
        FieldType::Boolean => "boolean",
        FieldType::Date => "date",
        FieldType::DateTime => "date and time",
        FieldType::Image => "image",
        FieldType::CompositeField(_) => "composite field",
        FieldType::Relation(_) => "relation",
        FieldType::Array(_) => "array",
        FieldType::TextEnum(_) => "enum",
    }
}

/// Every relation target a schema names, including the ones inside the composite definitions it
/// embeds.
///
/// Arrays and composite definitions are both walked: `validate_field_type` refuses to store a
/// relation as an array item, and this walk is what would still find the target if that rule ever
/// changed. A definition is reused by whatever embeds it, and one may hold an array of itself, so
/// each definition is walked once.
pub fn referenced_relation_targets(
    fields: &[FieldSchema],
    composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
) -> Vec<RelationTarget> {
    let mut targets = Vec::new();
    let mut walked = HashSet::new();
    collect_relation_targets(fields, composites, &mut walked, &mut targets);
    targets
}

fn collect_relation_targets(
    fields: &[FieldSchema],
    composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    walked: &mut HashSet<CompositeFieldId>,
    into: &mut Vec<RelationTarget>,
) {
    for field in fields {
        collect_field_relation_targets(&field.field_type, composites, walked, into);
    }
}

fn collect_field_relation_targets(
    field_type: &FieldType,
    composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    walked: &mut HashSet<CompositeFieldId>,
    into: &mut Vec<RelationTarget>,
) {
    match field_type {
        FieldType::Relation(options) => into.push(options.target.clone()),
        FieldType::Array(items) => {
            for item in items {
                collect_field_relation_targets(item, composites, walked, into);
            }
        }
        FieldType::CompositeField(reference) => {
            // Each definition once: which fields reach it changes nothing about what it names, and
            // a definition that holds an array of itself would otherwise be walked for ever.
            if !walked.insert(reference.id.clone()) {
                return;
            }
            if let Some(schema) = composites.get(&reference.id) {
                collect_relation_targets(schema, composites, walked, into);
            }
        }
        _ => {}
    }
}

/// Every relation a schema declares has to point at something that exists.
///
/// `composites` is every composite definition of the site, which is what lets a target declared
/// inside one be found from the schema that embeds it, and `available` is what the site has - the
/// collections and the single pages - because this module knows neither (see
/// `RelationTargetSource`, which is what hands them over). A typo here would
/// otherwise leave a field that can never hold anything, refused only when somebody tries to fill
/// it in.
pub fn validate_relation_targets(
    fields: &[FieldSchema],
    composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    available: &[RelationTarget],
) -> Result<(), String> {
    for target in referenced_relation_targets(fields, composites) {
        if !available.contains(&target) {
            let kind = match target {
                RelationTarget::Collection { .. } => "collection",
                RelationTarget::SinglePage { .. } => "single page",
            };
            return Err(format!(
                "relation target '{}' is not a {kind} of this site",
                target.name()
            ));
        }
    }
    Ok(())
}

/// A slug's `generate_from` has to name another field an editor could take a value from.
///
/// Checked here because nothing about the schema editor would notice: a name that matches nothing
/// (or matches itself) leaves a button that does not work, and a field of a kind that is not text
/// gives a value with nothing to make a slug out of.
fn validate_slug_options(fields: &[FieldSchema], field: &FieldSchema) -> Result<(), String> {
    let FieldType::Slug(options) = &field.field_type else {
        return Ok(());
    };
    let Some(source) = options.generate_from.as_deref().map(str::trim) else {
        return Ok(());
    };
    if source.is_empty() {
        // Same as not naming one: the editor simply offers no button.
        return Ok(());
    }
    if source == field.name.trim() {
        return Err(format!(
            "field '{}': a slug cannot be generated from itself",
            field.name
        ));
    }
    let Some(candidate) = fields.iter().find(|other| other.name.trim() == source) else {
        return Err(format!(
            "field '{}': generate_from names '{source}', which is not a field of this schema",
            field.name
        ));
    };
    if !matches!(
        candidate.field_type,
        FieldType::Text(_) | FieldType::Markdown(_)
    ) {
        return Err(format!(
            "field '{}': a slug can only be generated from a text field, and '{source}' is not one",
            field.name
        ));
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
        // A relation is a set already, and how many it holds is its own `has_many`: an array of
        // relations would be a second way to say "several references", with nothing that could
        // edit or deliver it.
        if matches!(item, FieldType::Relation(_)) {
            return Err(format!(
                "field '{name}': a relation cannot be an array item; a relation already holds \
                 several references when it asks for them"
            ));
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
    item: &crate::models::values::FieldValueMap<Vec<FieldSchema>>,
) -> Vec<UniqueValue> {
    let mut values = Vec::new();
    for field in fields {
        if !field.is_unique() {
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
    fields.iter().any(|field| field.is_unique())
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
            is_title: false,
            show_in_list: false,
            name: name.to_string(),
            field_type,
            required: false,
            width: 12,
            height: 1,
            unique: false,
        }
    }

    /// The list setting travels with the field, and a schema that does not mention it reads as
    /// "not in the list" - which is what every schema stored before it existed says.
    #[test]
    fn the_list_setting_travels_with_the_field() {
        let mut marked = field("title", FieldType::Text(TextFieldOptions::default()));
        marked.show_in_list = true;
        let json = serde_json::to_value(&marked).expect("a field");
        assert_eq!(json["show_in_list"], serde_json::json!(true));
        assert_eq!(
            serde_json::from_value::<FieldSchema>(json)
                .expect("the field back")
                .show_in_list,
            true
        );

        // Omitted when false, like `unique`, so a schema that uses neither is unchanged on the wire.
        let plain = field("title", FieldType::Text(TextFieldOptions::default()));
        let json = serde_json::to_value(&plain).expect("a field");
        assert!(json.get("show_in_list").is_none());
        let without_the_key: FieldSchema = serde_json::from_value(serde_json::json!({
            "name": "title",
            "field_type": { "Text": {} },
            "required": false,
            "width": 12,
            "height": 1
        }))
        .expect("a field written before this existed");
        assert!(!without_the_key.show_in_list);
    }

    /// One name per item, and it has to be something that reads as one.
    #[test]
    fn a_schema_has_at_most_one_title_and_it_has_to_be_readable() {
        let named = |name: &str, field_type: FieldType| {
            let mut field = field(name, field_type);
            field.is_title = true;
            field
        };

        assert!(
            validate_schema(
                &[named("name", FieldType::Text(TextFieldOptions::default()))],
                SchemaScope::Collection
            )
            .is_ok()
        );
        // A short Markdown field is a name too: the text is taken as it is.
        assert!(
            validate_schema(
                &[named(
                    "name",
                    FieldType::Markdown(TextFieldOptions::default())
                )],
                SchemaScope::Collection
            )
            .is_ok()
        );

        // Two titles is two names for one item, and the second one names the first.
        let error = validate_schema(
            &[
                named("name", FieldType::Text(TextFieldOptions::default())),
                named("also", FieldType::Text(TextFieldOptions::default())),
            ],
            SchemaScope::Collection,
        )
        .unwrap_err();
        assert!(error.contains("'name' is already the title"), "{error}");

        // A reference shows the title as the name of an item, so it has to be one line.
        for (field_type, kind) in [
            (FieldType::Image, "image"),
            (
                FieldType::Array(vec![FieldType::Text(TextFieldOptions::default())]),
                "array",
            ),
            (
                FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::from("seo"),
                }),
                "composite field",
            ),
            (relation(collection_target("authors"), true), "relation"),
        ] {
            let error =
                validate_schema(&[named("bad", field_type)], SchemaScope::Collection).unwrap_err();
            assert!(
                error.contains(&format!("a {kind} cannot be read as one line")),
                "{error}"
            );
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
        assert!(
            field
                .validate_text_length(&"あ".repeat(20), &options, "title")
                .is_ok()
        );
        assert!(
            field
                .validate_text_length(&"あ".repeat(21), &options, "title")
                .is_err()
        );
        assert!(
            field
                .validate_text_length("短い", &options, "title")
                .is_err()
        );

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

    /// A slug is unique by being a slug, so the bookkeeping has to see it without the flag.
    #[test]
    fn a_slug_counts_as_a_unique_field() {
        let schema = vec![field("address", FieldType::Slug(SlugOptions::default()))];

        assert!(has_unique_fields(&schema));
        assert!(schema[0].is_unique() && schema[0].is_slug());
        assert!(!schema[0].unique);
    }

    #[test]
    fn accepts_a_slug_generated_from_another_text_field() {
        let schema = vec![
            field("title", FieldType::Text(TextFieldOptions::default())),
            field(
                "address",
                FieldType::Slug(SlugOptions {
                    generate_from: Some("title".to_string()),
                }),
            ),
        ];

        assert!(validate_schema(&schema, SchemaScope::Collection).is_ok());
    }

    #[test]
    fn rejects_a_slug_that_cannot_be_generated_or_that_would_compare_nothing() {
        let text = || field("title", FieldType::Text(TextFieldOptions::default()));
        let slug = |from: Option<&str>| {
            field(
                "address",
                FieldType::Slug(SlugOptions {
                    generate_from: from.map(str::to_string),
                }),
            )
        };

        // Naming a field that is not there, itself, or something that is not text.
        let missing = vec![text(), slug(Some("subtitle"))];
        assert!(
            validate_schema(&missing, SchemaScope::Collection)
                .unwrap_err()
                .contains("is not a field of this schema")
        );
        let itself = vec![text(), slug(Some("address"))];
        assert!(
            validate_schema(&itself, SchemaScope::Collection)
                .unwrap_err()
                .contains("cannot be generated from itself")
        );
        let number = vec![field("count", FieldType::Number), slug(Some("count"))];
        assert!(
            validate_schema(&number, SchemaScope::Collection)
                .unwrap_err()
                .contains("only be generated from a text field")
        );

        // A page's address is its name, and a composite's values live inside the item using it.
        let page = vec![slug(None)];
        assert!(
            validate_schema(&page, SchemaScope::SinglePage)
                .unwrap_err()
                .contains("address is its name")
        );
        assert!(
            validate_schema(&page, SchemaScope::CompositeDefinition)
                .unwrap_err()
                .contains("a slug there would name nothing")
        );

        // And the flag it does not need is refused rather than ignored.
        let mut flagged = slug(None);
        flagged.unique = true;
        assert!(
            validate_schema(&[flagged], SchemaScope::Collection)
                .unwrap_err()
                .contains("a slug is unique already")
        );
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
        assert!(
            validate_schema(&[field("  ", FieldType::Number)], SchemaScope::Collection)
                .unwrap_err()
                .contains("must not be empty")
        );

        let duplicate = vec![
            field("title", FieldType::Number),
            field("title", FieldType::Boolean),
        ];
        assert!(
            validate_schema(&duplicate, SchemaScope::Collection)
                .unwrap_err()
                .contains("duplicate field name")
        );
    }

    #[test]
    fn rejects_out_of_range_layout() {
        let mut too_wide = field("a", FieldType::Number);
        too_wide.width = 13;
        assert!(
            validate_schema(&[too_wide], SchemaScope::Collection)
                .unwrap_err()
                .contains("width")
        );

        let mut too_narrow = field("a", FieldType::Number);
        too_narrow.width = 0;
        assert!(
            validate_schema(&[too_narrow], SchemaScope::Collection)
                .unwrap_err()
                .contains("width")
        );

        let mut no_height = field("a", FieldType::Number);
        no_height.height = 0;
        assert!(
            validate_schema(&[no_height], SchemaScope::Collection)
                .unwrap_err()
                .contains("height")
        );
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
        assert!(
            error.contains("both Number and Image"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_empty_nested_and_composite_array_items() {
        assert!(
            validate_schema(
                &[field("a", FieldType::Array(vec![]))],
                SchemaScope::Collection
            )
            .unwrap_err()
            .contains("at least one item type")
        );

        assert!(
            validate_schema(
                &[field(
                    "a",
                    FieldType::Array(vec![FieldType::Array(vec![FieldType::Number])])
                )],
                SchemaScope::Collection
            )
            .unwrap_err()
            .contains("nested arrays")
        );

        // A composite as an array item is allowed: it is an object on the wire, so it cannot be
        // confused with a scalar, and the reference is checked like any other.
        validate_schema(
            &[field(
                "a",
                FieldType::Array(vec![FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::from("seo"),
                })]),
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

        assert!(
            validate_composite_references(&[field("seo", composite_ref("seo"))], &available)
                .is_ok()
        );
        assert!(
            validate_composite_references(&[field("nope", composite_ref("nope"))], &available)
                .unwrap_err()
                .contains("composite field 'nope' does not exist")
        );
    }

    fn collection_target(name: &str) -> RelationTarget {
        RelationTarget::Collection {
            name: name.to_string(),
        }
    }

    fn page_target(name: &str) -> RelationTarget {
        RelationTarget::SinglePage {
            name: name.to_string(),
        }
    }

    fn relation(target: RelationTarget, has_many: bool) -> FieldType {
        FieldType::Relation(RelationOptions {
            target,
            has_many,
            inverse_name: None,
        })
    }

    #[test]
    fn a_relation_names_what_it_points_at() {
        assert!(
            validate_schema(
                &[field(
                    "author",
                    relation(collection_target("authors"), true)
                )],
                SchemaScope::Collection
            )
            .is_ok()
        );
        assert!(
            validate_schema(
                &[field("home", relation(page_target("home"), false))],
                SchemaScope::Collection
            )
            .is_ok()
        );

        // A relation with no target at all is a field that could never be filled in.
        assert!(
            validate_schema(
                &[field("author", relation(collection_target("  "), false))],
                SchemaScope::Collection
            )
            .unwrap_err()
            .contains("must name the collection")
        );

        // A page is one item whose identity is its name, so asking for several is asking for
        // something that cannot exist.
        assert!(
            validate_schema(
                &[field("homes", relation(page_target("home"), true))],
                SchemaScope::Collection
            )
            .unwrap_err()
            .contains("can only be a single reference")
        );

        // A blank inverse name is a label with nothing in it; leaving it out says more.
        let blank_inverse = FieldType::Relation(RelationOptions {
            target: collection_target("authors"),
            has_many: false,
            inverse_name: Some("   ".to_string()),
        });
        assert!(
            validate_schema(&[field("author", blank_inverse)], SchemaScope::Collection)
                .unwrap_err()
                .contains("must not be blank")
        );
    }

    #[test]
    fn a_relation_is_never_an_array_item() {
        let array = FieldType::Array(vec![relation(collection_target("authors"), true)]);
        assert!(
            validate_schema(&[field("related", array)], SchemaScope::Collection)
                .unwrap_err()
                .contains("cannot be an array item")
        );
    }

    #[test]
    fn collects_every_relation_target_a_schema_names() {
        let mut composites = HashMap::new();
        composites.insert(
            CompositeFieldId::from("cta"),
            vec![field("home", relation(page_target("home"), false))],
        );
        let fields = vec![
            field("author", relation(collection_target("authors"), false)),
            // Walking arrays too, so a target is never missed wherever it was written.
            field(
                "related",
                FieldType::Array(vec![relation(collection_target("writers"), false)]),
            ),
            // And into the composite definitions it embeds, which is where a relation may sit.
            field(
                "cta",
                FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::from("cta"),
                }),
            ),
            field("plain", FieldType::Number),
        ];

        assert_eq!(
            referenced_relation_targets(&fields, &composites),
            vec![
                collection_target("authors"),
                collection_target("writers"),
                page_target("home")
            ]
        );
        assert!(
            referenced_relation_targets(&[field("plain", FieldType::Number)], &composites)
                .is_empty()
        );
    }

    #[test]
    fn walks_a_composite_definition_that_holds_an_array_of_itself() {
        // Allowed by `validate_no_composite_cycles`, because the array's elements come from the
        // value: the walk has to survive it, and still name what the definition holds.
        let mut composites = HashMap::new();
        composites.insert(
            CompositeFieldId::from("block"),
            vec![
                field("label", FieldType::Text(TextFieldOptions::default())),
                field("author", relation(collection_target("authors"), false)),
                field(
                    "blocks",
                    FieldType::Array(vec![FieldType::CompositeField(CompositeFieldReference {
                        id: CompositeFieldId::from("block"),
                    })]),
                ),
            ],
        );
        let fields = vec![
            field(
                "blocks",
                FieldType::Array(vec![FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::from("block"),
                })]),
            ),
            field(
                "other",
                FieldType::CompositeField(CompositeFieldReference {
                    id: CompositeFieldId::from("block"),
                }),
            ),
        ];

        assert_eq!(
            referenced_relation_targets(&fields, &composites),
            vec![collection_target("authors")]
        );
    }

    #[test]
    fn a_composite_definition_may_hold_a_relation() {
        // Its target is a collection or a page of the site, not of whatever embeds it, so nothing
        // about the embedder has to be known here.
        assert!(
            validate_schema(
                &[field(
                    "author",
                    relation(collection_target("authors"), false)
                )],
                SchemaScope::CompositeDefinition
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_relations_to_targets_that_do_not_exist() {
        let available = vec![collection_target("authors"), page_target("home")];

        assert!(
            validate_relation_targets(
                &[field(
                    "author",
                    relation(collection_target("authors"), false)
                )],
                &HashMap::new(),
                &available
            )
            .is_ok()
        );
        assert!(
            validate_relation_targets(
                &[field("home", relation(page_target("home"), false))],
                &HashMap::new(),
                &available
            )
            .is_ok()
        );

        let error = validate_relation_targets(
            &[field(
                "author",
                relation(collection_target("writers"), false),
            )],
            &HashMap::new(),
            &available,
        )
        .unwrap_err();
        assert!(error.contains("'writers' is not a collection"), "{error}");

        // The same name of the other kind is a different target, so it is still missing.
        let error = validate_relation_targets(
            &[field("home", relation(collection_target("home"), false))],
            &HashMap::new(),
            &available,
        )
        .unwrap_err();
        assert!(error.contains("'home' is not a collection"), "{error}");

        let error = validate_relation_targets(
            &[field("about", relation(page_target("about"), false))],
            &HashMap::new(),
            &available,
        )
        .unwrap_err();
        assert!(error.contains("'about' is not a single page"), "{error}");
    }

    #[test]
    fn only_a_multiple_reference_to_a_collection_is_not_single() {
        let single = |target: RelationTarget, has_many: bool| RelationOptions {
            target,
            has_many,
            inverse_name: None,
        };

        assert!(single(collection_target("authors"), false).is_single());
        // A page is a single reference whatever the flag says, which the schema refuses anyway.
        assert!(single(page_target("home"), true).is_single());
        assert!(!single(collection_target("authors"), true).is_single());
    }

    #[test]
    fn rejects_composites_that_reference_themselves() {
        let id = CompositeFieldId::from("seo");

        // Directly.
        let direct = vec![field("self", composite_ref("seo"))];
        assert!(
            validate_no_composite_cycles(&id, &direct, &HashMap::new())
                .unwrap_err()
                .contains("cannot reference itself")
        );

        // Through another composite: seo -> other -> seo.
        let indirect = vec![field("other", composite_ref("other"))];
        let mut all: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        all.insert(
            CompositeFieldId::from("other"),
            vec![field("back", composite_ref("seo"))],
        );
        assert!(
            validate_no_composite_cycles(&id, &indirect, &all)
                .unwrap_err()
                .contains("cannot reference itself")
        );

        // A plain chain is fine, and a missing target must not loop forever.
        let chain = vec![
            field("a", composite_ref("a")),
            field("b", composite_ref("b")),
        ];
        let mut all: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        all.insert(
            CompositeFieldId::from("a"),
            vec![field("b", composite_ref("b"))],
        );
        all.insert(CompositeFieldId::from("b"), vec![]);
        assert!(validate_no_composite_cycles(&id, &chain, &all).is_ok());
        assert!(
            validate_no_composite_cycles(&id, &vec![field("gone", composite_ref("gone"))], &all)
                .is_ok()
        );
    }

    /// A block that holds a list of blocks is the point of an array of composites, and it is a
    /// loop in the reference graph. It is allowed because every edge of this loop goes through an
    /// array: the editor draws array elements from the value, so an empty array draws nothing.
    #[test]
    fn allows_a_composite_that_reaches_itself_through_an_array() {
        let id = CompositeFieldId::from("tree");
        let tree = vec![
            field("line", FieldType::Text(TextFieldOptions::default())),
            field("children", FieldType::Array(vec![composite_ref("tree")])),
        ];
        assert!(validate_no_composite_cycles(&id, &tree, &HashMap::new()).is_ok());

        // ...including when the loop takes a detour: tree -> other -> array of tree.
        let mut all: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        all.insert(
            CompositeFieldId::from("other"),
            vec![field(
                "again",
                FieldType::Array(vec![composite_ref("tree")]),
            )],
        );
        let detour = vec![field("other", composite_ref("other"))];
        assert!(validate_no_composite_cycles(&id, &detour, &all).is_ok());

        // But a route back that never enters an array is still refused.
        let mut back: HashMap<CompositeFieldId, CompositeFieldSchema> = HashMap::new();
        back.insert(
            CompositeFieldId::from("other"),
            vec![field("back", composite_ref("tree"))],
        );
        assert!(
            validate_no_composite_cycles(&id, &detour, &back)
                .unwrap_err()
                .contains("cannot reference itself")
        );
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
