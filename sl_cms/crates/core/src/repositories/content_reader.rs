//! Reading one piece of content by the name a reference index uses for it.
//!
//! A relation rule has to look at both ends of a reference, and the ends are not always the same
//! kind of thing: a page may reference an item, an item may reference a page, and what the rule
//! asks - "is this on the site, and does it hold what it must?" - does not depend on which. The
//! two repositories that know this are separate traits, so this is the one place a caller can ask
//! the question about content by name, whoever owns it (see
//! [`relation_rules`](crate::repositories::relation_rules), which is what asks).
//!
//! Nothing here writes, and nothing here decides: it hands over the schema and the copies, and the
//! rule reads them.

use std::future::Future;
use std::pin::Pin;

use crate::models::collection::{CollectionItemId, CollectionName};
use crate::models::owner::{ItemOwner, ItemOwnerKind};
use crate::models::schema::{FieldSchema, RelationTarget};
use crate::models::single_page::SinglePageName;
use crate::models::values::FieldValueMap;
use crate::repositories::collection_repository::{BoxError, CollectionRepository};
use crate::repositories::single_page_repository::SinglePageRepository;

/// What one piece of content declares and holds.
///
/// Both copies are handed over rather than one: publishing asks about the copy going live, and
/// unpublishing asks about the copy the site is currently serving, and each rule wants the one it
/// is about (see [`relation_rules`](crate::repositories::relation_rules)).
pub struct OwnedContent {
    /// The fields it declares, so a rule can tell a required relation from an optional one.
    pub schema: Vec<FieldSchema>,
    /// Whether it is on the site.
    ///
    /// Metadata's answer, not "is there a published copy": unpublishing keeps the published record
    /// and takes the item off the site, so a copy that outlives the status is exactly what an
    /// unpublish leaves behind.
    pub published: bool,
    /// The copy the site serves, absent when nothing was ever published.
    pub published_values: Option<FieldValueMap<Vec<FieldSchema>>>,
    /// The working copy an editor is in the middle of, absent when there is none.
    pub working_values: Option<FieldValueMap<Vec<FieldSchema>>>,
}

pub type ReadContentFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<OwnedContent>, BoxError>> + Send + 'a>>;

pub type DeclaresInverseFuture<'a> =
    Pin<Box<dyn Future<Output = Result<bool, BoxError>> + Send + 'a>>;

pub type DeclaredInversesFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<DeclaredInverse>, BoxError>> + Send + 'a>>;

/// Who declares a relation on the other side: a collection's schema or a page's.
///
/// A schema save is about a *schema*, not about one item, so this is the identity the inverse-name
/// rule compares: the same collection saving again is not a second declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaOwner {
    Collection(String),
    SinglePage(String),
}

impl SchemaOwner {
    /// What to call it in a refusal somebody reads.
    pub fn describe(&self) -> String {
        match self {
            SchemaOwner::Collection(name) => format!("collection '{name}'"),
            SchemaOwner::SinglePage(name) => format!("single page '{name}'"),
        }
    }
}

/// One inverse name a schema gives a relation to a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredInverse {
    pub of: SchemaOwner,
    pub name: String,
}

/// Content, by the name a reference index uses for it.
pub trait ContentReader: Send + Sync + 'static {
    /// What this content declares and holds, or `None` when it is gone.
    ///
    /// Gone is an answer rather than an error: the rules treat it as "not published", because a
    /// reference to something that is not there serves nothing.
    fn read_content<'a>(&'a self, owner: &'a ItemOwner) -> ReadContentFuture<'a>;

    /// Whether anything on the site gives a relation this name on the other side (`inverse_name`).
    ///
    /// The one question that needs the whole site rather than one piece of content: a caller
    /// asking for an inverse expansion of an item nothing references has no referrer to read the
    /// name off, and answering "empty" to a typo is worse than reading the schemas.
    fn declares_inverse<'a>(&'a self, name: &'a str) -> DeclaresInverseFuture<'a>;

    /// Who calls a relation to `target` what, so a second schema cannot give the same target the
    /// same name on the other side.
    ///
    /// The whole site again, for the same reason as `declares_inverse`: an inverse name is how a
    /// relation is addressed from the target, and two names for one relation would make the answer
    /// depend on who asked.
    fn declared_inverses<'a>(&'a self, target: &'a RelationTarget) -> DeclaredInversesFuture<'a>;
}

/// Storage answers both halves, because it is both repositories (see `Storage`).
impl<T: CollectionRepository + SinglePageRepository> ContentReader for T {
    fn declares_inverse<'a>(&'a self, name: &'a str) -> DeclaresInverseFuture<'a> {
        Box::pin(async move {
            for collection in self.list_collection_names().await? {
                if let Some(schema) = self.get_collection_schema(&collection).await? {
                    if schema_declares_inverse(&schema, name) {
                        return Ok(true);
                    }
                }
            }
            for page in self.list_all_page_names().await? {
                if let Some(schema) = self.get_single_page_schema(&page).await? {
                    if schema_declares_inverse(&schema, name) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })
    }

    fn declared_inverses<'a>(&'a self, target: &'a RelationTarget) -> DeclaredInversesFuture<'a> {
        Box::pin(async move {
            let mut declared = Vec::new();
            for collection in self.list_collection_names().await? {
                let Some(schema) = self.get_collection_schema(&collection).await? else {
                    continue;
                };
                for name in inverse_names_for(&schema, target) {
                    declared.push(DeclaredInverse {
                        of: SchemaOwner::Collection(collection.as_str().to_string()),
                        name,
                    });
                }
            }
            for page in self.list_all_page_names().await? {
                let Some(schema) = self.get_single_page_schema(&page).await? else {
                    continue;
                };
                for name in inverse_names_for(&schema, target) {
                    declared.push(DeclaredInverse {
                        of: SchemaOwner::SinglePage(page.as_str().to_string()),
                        name,
                    });
                }
            }
            Ok(declared)
        })
    }

    fn read_content<'a>(&'a self, owner: &'a ItemOwner) -> ReadContentFuture<'a> {
        Box::pin(async move {
            match owner.kind {
                ItemOwnerKind::CollectionItem => {
                    let name = CollectionName::from(owner.name.as_str());
                    let id = CollectionItemId::from_u64(owner.item.unwrap_or_default());
                    let Some(schema) = self.get_collection_schema(&name).await? else {
                        return Ok(None);
                    };
                    let published = self
                        .get_item_metadata(&name, &id)
                        .await?
                        .map(|metadata| metadata.is_published())
                        .unwrap_or(false);
                    Ok(Some(OwnedContent {
                        schema,
                        published,
                        published_values: self.get_collection_item(&name, &id).await?,
                        working_values: self.get_collection_item_draft(&name, &id).await?,
                    }))
                }
                ItemOwnerKind::SinglePage => {
                    let name = SinglePageName::from(owner.name.as_str());
                    let Some(schema) = self.get_single_page_schema(&name).await? else {
                        return Ok(None);
                    };
                    let published = self
                        .get_page_metadata(&name)
                        .await?
                        .map(|metadata| metadata.is_published())
                        .unwrap_or(false);
                    Ok(Some(OwnedContent {
                        schema,
                        published,
                        published_values: self.get_single_page_item(&name).await?,
                        working_values: self.get_single_page_item_draft(&name).await?,
                    }))
                }
            }
        })
    }
}

/// The inverse names a schema gives a relation to `target`.
fn inverse_names_for(schema: &[FieldSchema], target: &RelationTarget) -> Vec<String> {
    schema
        .iter()
        .filter_map(|field| match &field.field_type {
            crate::models::schema::FieldType::Relation(options) => {
                if &options.target == target {
                    options.inverse_name.clone()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

/// Whether a schema gives a relation this name on the other side.
fn schema_declares_inverse(schema: &[FieldSchema], name: &str) -> bool {
    schema.iter().any(|field| match &field.field_type {
        crate::models::schema::FieldType::Relation(options) => {
            options.inverse_name.as_deref() == Some(name)
        }
        _ => false,
    })
}

/// Content that is always gone, for tests that are not about relations.
///
/// A deployment always reads the real storage; this exists so a service test can be built without
/// one, the way `NoRelations` does for the index.
#[cfg(test)]
pub struct NoContent;

#[cfg(test)]
impl ContentReader for NoContent {
    fn read_content<'a>(&'a self, _owner: &'a ItemOwner) -> ReadContentFuture<'a> {
        Box::pin(async { Ok(None) })
    }

    fn declares_inverse<'a>(&'a self, _name: &'a str) -> DeclaresInverseFuture<'a> {
        Box::pin(async { Ok(false) })
    }

    fn declared_inverses<'a>(&'a self, _target: &'a RelationTarget) -> DeclaredInversesFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
