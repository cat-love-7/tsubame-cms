use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;

use crate::models::collection::{
    CollectionItem, CollectionItemId, CollectionItemResponse, CollectionName, CollectionSchema,
};
use crate::models::error::{FieldRefusal, HttpError, map_internal_error};
use crate::models::image::{Image, ImageId};
use crate::models::item_status::{
    ItemDates, ItemMetadata, ItemStatus, ItemStatusOutcome, PublishedBy,
};
use crate::models::owner::ItemOwner;
use crate::models::pagination::{Page, Pagination};
use crate::models::schema::{
    CompositeFieldId, RelationTarget, SchemaScope, has_unique_fields, referenced_relation_targets,
    unique_values, validate_composite_references, validate_relation_targets, validate_schema,
};
use crate::models::values::{FieldValue, FieldValueMap};
use crate::repositories::collection_repository::ApplyStatusError;
use crate::repositories::collection_repository::{CollectionRepository, Reservation, UniqueValue};
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::ImageRepository;
use crate::repositories::relation_repository::RelationRepository;
use crate::repositories::relation_targets::RelationTargetSource;
use crate::webhook::{ContentEvent, Notifier};

pub struct CollectionService<
    CR: CollectionRepository,
    CFR: CompositeFieldRepository,
    IR: ImageRepository,
> {
    collection_repository: Arc<CR>,
    composite_field_repository: Arc<CFR>,
    image_repository: Arc<IR>,
    /// What a relation field may point at, which only the schema save asks for.
    relation_targets: Arc<dyn RelationTargetSource>,
    /// The relation index, which answers who references what.
    relations: Arc<dyn RelationRepository>,
    /// Told about every publish/unpublish so a site build can be triggered.
    notifier: Arc<dyn Notifier>,
}

impl<CR: CollectionRepository, CFR: CompositeFieldRepository, IR: ImageRepository>
    CollectionService<CR, CFR, IR>
{
    pub fn new(
        collection_repository: Arc<CR>,
        composite_field_repository: Arc<CFR>,
        image_repository: Arc<IR>,
        relation_targets: Arc<dyn RelationTargetSource>,
        relations: Arc<dyn RelationRepository>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        CollectionService {
            collection_repository,
            composite_field_repository,
            image_repository,
            relation_targets,
            relations,
            notifier,
        }
    }
    pub async fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<CollectionSchema, HttpError> {
        let collection = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?;
        match collection {
            Some(schema) => Ok(schema),
            None => Err(HttpError::NotFound("Collection not found")),
        }
    }
    pub async fn update_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::Collection).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema).await?;
        self.ensure_relation_targets_exist(schema).await?;
        let Some(previous) = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
        else {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        };
        // A schema that declares a unique field has to bring the index up to date with the
        // items that are already stored, and refuse to be saved while they share a value.
        let claimed = self.backfill_unique_values(collection_name, schema).await?;
        if let Err(e) = self
            .collection_repository
            .add_collection_schema(collection_name, schema)
            .await
        {
            // The field is not unique yet, so nothing should be held for it.
            for (id, value) in &claimed {
                self.release_unique_values(collection_name, id, std::slice::from_ref(value))
                    .await;
            }
            return Err(map_internal_error(e));
        }
        // The schema is what is stored now, so anything it no longer makes unique has to give its
        // values back: they are held for items that may not hold them any more, and a later
        // "unique again" would refuse a value nothing is using.
        self.release_fields_no_longer_unique(collection_name, &previous, schema)
            .await;
        Ok(())
    }

    /// Give back the index entries of the fields a schema stops declaring unique.
    ///
    /// A failure here leaves a value claimed that nothing holds, which the next schema save puts
    /// right; it must not report the save that did happen as an error (the same reason a failed
    /// release after an item write is only a warning).
    async fn release_fields_no_longer_unique(
        &self,
        collection_name: &CollectionName,
        before: &CollectionSchema,
        after: &CollectionSchema,
    ) {
        for field in before {
            let still_unique = after
                .iter()
                .any(|candidate| candidate.name == field.name && candidate.is_unique());
            if !field.is_unique() || still_unique {
                continue;
            }
            match self
                .collection_repository
                .list_unique_values(collection_name, &field.name)
                .await
            {
                Ok(held) => {
                    for (item_id, value) in held {
                        self.release_unique_values(
                            collection_name,
                            &item_id,
                            std::slice::from_ref(&value),
                        )
                        .await;
                    }
                }
                Err(e) => tracing::warn!("failed to list the values held for {}: {e}", field.name),
            }
        }
    }

    pub async fn list_collections(&self) -> Result<Vec<CollectionName>, HttpError> {
        self.collection_repository
            .list_collection_names()
            .await
            .map_err(map_internal_error)
    }

    /// Every composite a schema references must exist, otherwise the schema can be stored
    /// but never used to read or write values.
    async fn ensure_composites_exist(&self, schema: &CollectionSchema) -> Result<(), HttpError> {
        let available: HashSet<CompositeFieldId> = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?
            .into_keys()
            .collect();
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))
    }

    /// The same for relations: a target that does not exist is a field that can never hold
    /// anything, so the schema that names it is refused rather than stored.
    ///
    /// Most schemas name no relation at all, and answering the question costs two lists of names,
    /// so a schema without one asks for nothing.
    async fn ensure_relation_targets_exist(
        &self,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        if referenced_relation_targets(schema).is_empty() {
            return Ok(());
        }
        let available: Vec<RelationTarget> = self
            .relation_targets
            .relation_targets()
            .await
            .map_err(map_internal_error)?;
        validate_relation_targets(schema, &available).map_err(|e| HttpError::BadRequest(&e))
    }
    pub async fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::Collection).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema).await?;
        self.ensure_relation_targets_exist(schema).await?;
        if self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
            .is_some()
        {
            return Err(HttpError::Conflict(&format!(
                "Collection with id '{}' already exists",
                collection_name
            )));
        }
        self.collection_repository
            .add_collection_schema(collection_name, schema)
            .await
            .map_err(map_internal_error)
    }
    /// The content that references one of this collection's items.
    ///
    /// An item that is not there has no references to report, so the answer is a 404 rather than an
    /// empty list: the caller asked about something that does not exist.
    pub async fn get_item_references(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<Vec<ItemOwner>, HttpError> {
        self.require_item(collection_name, &item_id).await?;
        self.relations
            .get_relation_references(&ItemOwner::collection_item(
                collection_name.as_str(),
                *item_id,
            ))
            .await
            .map_err(map_internal_error)
    }

    pub async fn delete_collection(
        &self,
        collection_name: &CollectionName,
    ) -> Result<(), HttpError> {
        if self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        self.collection_repository
            .delete_collection(collection_name)
            .await
            .map_err(map_internal_error)
    }
    pub async fn get_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?;
        match schema {
            None => Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            ))),
            Some(schema) => {
                // The working copies: this is the screen an editor saves from.
                let items = self.working_items(collection_name).await?;
                self.format_items(&schema, items).await
            }
        }
    }
    pub async fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<CollectionItemResponse, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?;
        if schema.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        let item = self.working_item(collection_name, &item_id).await?;
        match item {
            Some(item) => self.format_item(collection_name, &item).await,
            None => Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id.to_string(),
                collection_name
            ))),
        }
    }
    /// Publish or unpublish a batch of items, and say what happened to each one.
    ///
    /// One item's refusal is not another's: a batch is a convenience for the editor, and the
    /// alternative - an all-or-nothing operation - would either publish items nobody asked about
    /// after a failure or leave a half-applied change with no way to tell which half. Each item
    /// goes through [`CollectionService::set_item_status`], so a batch has exactly the guarantees a
    /// single publish has, including its refusals.
    pub async fn set_items_status(
        &self,
        collection_name: &CollectionName,
        item_ids: &[CollectionItemId],
        status: ItemStatus,
        actor: PublishedBy,
    ) -> Vec<ItemStatusOutcome> {
        let mut outcomes = Vec::with_capacity(item_ids.len());
        for item_id in item_ids {
            let outcome = match self
                .set_item_status(collection_name, item_id.clone(), status, actor.clone())
                .await
            {
                Ok(metadata) => ItemStatusOutcome::Changed {
                    id: item_id.clone(),
                    metadata,
                },
                Err(error) => ItemStatusOutcome::Refused {
                    id: item_id.clone(),
                    code: error.code.to_string(),
                    message: error.message.clone(),
                },
            };
            outcomes.push(outcome);
        }
        outcomes
    }

    /// Copy an item, and answer with the id of the copy.
    ///
    /// The copy is what an editor is looking at (the working copy if there is one, the published
    /// one otherwise), it starts as a **draft** whatever the original was, and its **unique fields
    /// are emptied**: two items cannot hold the same value, and a copy that reused the title or the
    /// slug would be refused rather than made. Everything else comes across, so the editor only
    /// has to fill in the parts that have to be new - which is also what makes the slug's
    /// "generate from the title" button the natural next step.
    pub async fn duplicate_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<u64, HttpError> {
        self.require_item(collection_name, &item_id).await?;
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default();
        let source = match self.working_item(collection_name, &item_id).await? {
            Some(item) => item,
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Item with id '{}' has no content to copy",
                    item_id
                )));
            }
        };

        let mut copy = source;
        for field in &schema {
            if field.is_unique() {
                copy.0.insert(field.name.clone(), field.get_default_value());
            }
        }
        self.create_collection_item(collection_name, &copy).await
    }

    pub async fn create_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &CollectionItem,
    ) -> Result<u64, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?;
        let (schema, item_data) = match schema {
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                )));
            }
            Some(schema) => {
                let composite_schema_map = self
                    .composite_field_repository
                    .list_composite_field_schemas()
                    .await
                    .map_err(map_internal_error)?;
                // Before validation, and before anything is reserved: what is stored, indexed and
                // looked up later is the canonical slug, not what was typed.
                let item_data = self.normalise_slugs(&schema, item_data)?;
                // A working copy may be incomplete: it is a draft, and publishing is what asks
                // for the required fields (see `FieldValueMap::validate_draft`).
                item_data
                    .validate_draft(&composite_schema_map, &schema)
                    .map_err(|e| e.into_http_error())?;
                (schema, item_data)
            }
        };
        let item_data = &item_data;
        let item_id = CollectionItemId::from_u64(
            self.collection_repository
                .add_collection_item(collection_name, item_data)
                .await
                .map_err(map_internal_error)?,
        );
        // The id only exists once the item does, so this is claimed immediately after: a value
        // someone else took in the meantime takes the new item with it rather than leaving a
        // half-created one behind.
        let held = Self::held_unique_values(&schema, [Some(item_data)]);
        if let Err(refusal) = self
            .reserve_unique_values(collection_name, &item_id, &held)
            .await
        {
            let _ = self
                .collection_repository
                .delete_collection_item(collection_name, &item_id)
                .await;
            let _ = self
                .collection_repository
                .set_item_metadata(collection_name, &item_id, &ItemMetadata::default())
                .await;
            return Err(refusal);
        }
        // The new item starts as a working copy; publishing is what puts it in front of
        // the delivery API.
        self.collection_repository
            .set_collection_item_draft(collection_name, &item_id, item_data)
            .await
            .map_err(map_internal_error)?;
        self.record_image_references(collection_name, &item_id, item_data, None)
            .await;
        self.stamp_item(collection_name, item_id, true).await?;
        Ok(*item_id)
    }
    pub async fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        item_data: &CollectionItem,
    ) -> Result<(), HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?;
        match schema {
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                )));
            }
            Some(schema) => {
                if self
                    .collection_repository
                    .get_collection_item(collection_name, &item_id)
                    .await
                    .map_err(map_internal_error)?
                    .is_none()
                {
                    return Err(HttpError::NotFound(&format!(
                        "Item with id '{}' not found in collection '{}'",
                        item_id.to_string(),
                        collection_name
                    )));
                }
                // The same normalisation as a create: a save stores the canonical slug, so the
                // index keeps holding what a URL will ask for.
                let item_data = self.normalise_slugs(&schema, item_data)?;
                item_data
                    .validate_draft(
                        &self
                            .composite_field_repository
                            .list_composite_field_schemas()
                            .await
                            .map_err(map_internal_error)?,
                        &schema,
                    )
                    .map_err(|e| e.into_http_error())?;
                // Everything below works from the normalised copy.
                let item_data = &item_data;

                // The index has to follow both copies: the published one still holds its value
                // while the working copy holds the new one, and a save only gives up a value
                // when neither copy uses it any more.
                let published = self
                    .collection_repository
                    .get_collection_item(collection_name, &item_id)
                    .await
                    .map_err(map_internal_error)?;
                let previous_draft = self
                    .collection_repository
                    .get_collection_item_draft(collection_name, &item_id)
                    .await
                    .map_err(map_internal_error)?;
                // A save never changes the status, so one status covers both sides.
                let is_published = self
                    .collection_repository
                    .get_item_metadata(collection_name, &item_id)
                    .await
                    .map_err(map_internal_error)?
                    .unwrap_or_default()
                    .is_published();
                let before = Self::held_unique_values_for_status(
                    &schema,
                    is_published,
                    published.as_ref(),
                    previous_draft.as_ref(),
                );
                let after = Self::held_unique_values_for_status(
                    &schema,
                    is_published,
                    published.as_ref(),
                    Some(item_data),
                );
                let (reserve, release) = Self::unique_difference(&before, &after);
                self.reserve_unique_values(collection_name, &item_id, &reserve)
                    .await?;

                // Saved into the working copy: the published item keeps serving the live
                // site until this version is published.
                if let Err(e) = self
                    .collection_repository
                    .set_collection_item_draft(collection_name, &item_id, item_data)
                    .await
                {
                    // Nothing was made visible, so give back what this call claimed.
                    self.release_unique_values(collection_name, &item_id, &reserve)
                        .await;
                    return Err(map_internal_error(e));
                }
                self.release_unique_values(collection_name, &item_id, &release)
                    .await;
                // Both copies while the item is published: the site is still serving the published
                // one, and this save only replaced the working copy.
                self.record_image_references(
                    collection_name,
                    &item_id,
                    item_data,
                    if is_published {
                        published.as_ref()
                    } else {
                        None
                    },
                )
                .await;
                self.stamp_item(collection_name, item_id, false).await?;
                Ok(())
            }
        }
    }

    /// The item holding `value` in `field`, which the schema must declare unique.
    ///
    /// The index is what is asked, so this is a point read rather than a scan - and it is the
    /// same answer a save would get, so "is this value free?" and "who has it?" cannot disagree.
    /// What comes back is the item an editor sees (its working copy when it has one).
    pub async fn get_item_by_unique_value(
        &self,
        collection_name: &CollectionName,
        field: &str,
        value: &str,
    ) -> Result<(CollectionItemId, CollectionItemResponse), HttpError> {
        let schema = self.get_collection_schema(collection_name).await?;
        let known = schema
            .iter()
            .any(|candidate| candidate.name == field && candidate.is_unique());
        if !known {
            return Err(HttpError::BadRequest(&format!(
                "field '{field}' is not unique, so a value does not name one item"
            )));
        }
        let value = Self::lookup_value(&schema, field, value);
        if value.is_empty() {
            return Err(HttpError::BadRequest("a value to look up is required"));
        }
        let unique = crate::repositories::collection_repository::UniqueValue {
            field: field.to_string(),
            value: value.clone(),
        };
        let item_id = self
            .collection_repository
            .find_unique_value(collection_name, &unique)
            .await
            .map_err(map_internal_error)?
            .ok_or_else(|| HttpError::NotFound(&format!("no item holds {field} = '{value}'")))?;
        let item = self
            .working_item(collection_name, &item_id)
            .await?
            .ok_or_else(|| HttpError::NotFound("the item holding that value is gone"))?;
        let values = self.format_item(collection_name, &item).await?;
        Ok((item_id, values))
    }

    /// Claim the unique values a write is about to make visible.
    ///
    /// Claimed before the write, so a failure leaves a value reserved that nothing holds yet -
    /// which refuses one value too early - rather than two items holding the same value. The
    /// caller releases what this claimed if the write itself fails.
    ///
    /// A refusal gives back what this call claimed before it: an item with two unique fields that
    /// collides on the second must not leave the first reserved, because the save that would have
    /// held it is not happening, and the value would be blocked for everyone from then on.
    async fn reserve_unique_values(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        values: &[UniqueValue],
    ) -> Result<(), HttpError> {
        let mut claimed: Vec<UniqueValue> = Vec::new();
        for value in values {
            let reservation = match self
                .collection_repository
                .reserve_unique_value(collection_name, item_id, value)
                .await
            {
                Ok(reservation) => reservation,
                Err(e) => {
                    // A storage failure is not a conflict, but it leaves the same half-claim
                    // behind: the values claimed before it are held by an item that is not going
                    // to be saved. Give them back before reporting the failure.
                    self.release_unique_values(collection_name, item_id, &claimed)
                        .await;
                    return Err(map_internal_error(e));
                }
            };
            match reservation {
                Reservation::Claimed => claimed.push(value.clone()),
                // Holding it already is not this call's doing, so a refusal must not take it away.
                Reservation::AlreadyHeld => {}
                Reservation::Taken { owner } => {
                    self.release_unique_values(collection_name, item_id, &claimed)
                        .await;
                    return Err(HttpError::Conflict(&format!(
                        "field '{}': the value '{}' is already used by item {}",
                        value.field, value.value, owner
                    ))
                    .with_code("value_taken")
                    .with_field(&value.field));
                }
            }
        }
        Ok(())
    }

    /// Give up values the item no longer holds, after the write that stopped holding them.
    async fn release_unique_values(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        values: &[UniqueValue],
    ) {
        for value in values {
            // A release that fails only leaves a value blocked, which is visible and fixable;
            // failing the request here would report a save that did happen as an error.
            if let Err(e) = self
                .collection_repository
                .release_unique_value(collection_name, item_id, value)
                .await
            {
                tracing::warn!(
                    "failed to release unique value {}={:?}: {e}",
                    value.field,
                    value.value
                );
            }
        }
    }

    /// Claim what `before` did not hold and give up what `after` does not hold.
    ///
    /// The two halves are split across the write by the caller: claiming happens before it (so
    /// the value is never twice-held), releasing after (so it is never unheld while in use).
    fn unique_difference(
        before: &[UniqueValue],
        after: &[UniqueValue],
    ) -> (Vec<UniqueValue>, Vec<UniqueValue>) {
        let reserve = after
            .iter()
            .filter(|value| !before.contains(value))
            .cloned()
            .collect();
        let release = before
            .iter()
            .filter(|value| !after.contains(value))
            .cloned()
            .collect();
        (reserve, release)
    }

    /// The unique values an item holds, given its status and its two copies.
    ///
    /// The working copy is what an editor sees and what a publish would release, so it always
    /// counts. The published copy counts only while the item is published: a draft keeps the
    /// copy it was created from, and holding that value would block one that nothing serves.
    fn held_unique_values_for_status(
        schema: &CollectionSchema,
        published_status: bool,
        published: Option<&CollectionItem>,
        working: Option<&CollectionItem>,
    ) -> Vec<UniqueValue> {
        let effective = working.or(published);
        if published_status {
            Self::held_unique_values(schema, [published, effective])
        } else {
            Self::held_unique_values(schema, [effective])
        }
    }

    /// Every unique value the given copies of one item hold, counted once.
    fn held_unique_values<'a>(
        schema: &CollectionSchema,
        copies: impl IntoIterator<Item = Option<&'a CollectionItem>>,
    ) -> Vec<UniqueValue> {
        let mut held: Vec<UniqueValue> = Vec::new();
        for copy in copies.into_iter().flatten() {
            for value in unique_values(schema, copy) {
                if !held.contains(&value) {
                    held.push(value);
                }
            }
        }
        held
    }

    /// Refuse to make a field unique while items already share a value, and index what is there
    /// when they do not.
    ///
    /// Called when a schema with a unique field is saved: the collection is scanned once, which
    /// is the honest moment to pay for it - the alternative is an index that is silently missing
    /// the items that were stored before the constraint existed.
    async fn backfill_unique_values(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<Vec<(CollectionItemId, UniqueValue)>, HttpError> {
        if !has_unique_fields(schema) {
            return Ok(Vec::new());
        }
        let items = self
            .collection_repository
            .list_collection_items(collection_name)
            .await
            .map_err(map_internal_error)?;
        let drafts: HashMap<CollectionItemId, CollectionItem> = self
            .collection_repository
            .list_collection_item_drafts(collection_name)
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .collect();

        // What each item *holds*, which depends on whether it is published - the same rule a save
        // uses, so the index this builds is the index those saves maintain.
        let statuses: HashMap<CollectionItemId, crate::models::item_status::ItemMetadata> = self
            .collection_repository
            .list_item_metadata(collection_name)
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .collect();

        // Every early exit below has to give back what this scan claimed: the schema is not saved
        // yet, so a claim left behind is an entry for a constraint that does not exist - a value
        // nobody holds, blocked for everyone. The scan therefore fills this list and never releases
        // anything itself; the wrapper does, whichever way it ends.
        let mut claimed: Vec<(CollectionItemId, UniqueValue)> = Vec::new();
        let outcome = self
            .scan_for_unique_values(
                collection_name,
                schema,
                &items,
                &drafts,
                &statuses,
                &mut claimed,
            )
            .await;
        if let Err(e) = outcome {
            for (id, value) in &claimed {
                self.release_unique_values(collection_name, id, std::slice::from_ref(value))
                    .await;
            }
            return Err(e);
        }
        Ok(claimed)
    }

    /// Claim every unique value the stored items hold, for a schema that is about to replace the
    /// one they were written under.
    ///
    /// Appends to `claimed` and reports the first refusal; releasing what it claimed is the
    /// caller's business (see `backfill_unique_values`).
    #[allow(clippy::too_many_arguments)]
    async fn scan_for_unique_values(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
        items: &[(CollectionItemId, CollectionItem)],
        drafts: &HashMap<CollectionItemId, CollectionItem>,
        statuses: &HashMap<CollectionItemId, crate::models::item_status::ItemMetadata>,
        claimed: &mut Vec<(CollectionItemId, UniqueValue)>,
    ) -> Result<(), HttpError> {
        for (item_id, published) in items {
            let working = drafts.get(&item_id);
            let is_published = statuses
                .get(&item_id)
                .map(|metadata| metadata.is_published())
                .unwrap_or(false);
            let held = Self::held_unique_values_for_status(
                schema,
                is_published,
                Some(&published),
                working,
            );

            // A slug is only a slug if what is held is canonical: the index would otherwise carry
            // a spelling no lookup asks for, and the item would be invisible under its own address.
            // Making a text field a slug, or adding one beside values that were typed freely, is
            // refused here, and the refusal names the item to fix.
            for value in &held {
                let is_slug = schema
                    .iter()
                    .any(|candidate| candidate.name == value.field && candidate.is_slug());
                if is_slug && crate::models::slug::normalise(&value.value) != value.value {
                    return Err(HttpError::Conflict(&format!(
                        "field '{}': item {} holds '{}', which is not a slug yet - make it one \
                         (lower-case letters, digits and hyphens) before the field becomes a slug",
                        value.field, item_id, value.value
                    ))
                    .with_code("invalid_slug")
                    .with_field(&value.field));
                }
            }
            for value in held {
                match self
                    .collection_repository
                    .reserve_unique_value(collection_name, &item_id, &value)
                    .await
                    .map_err(map_internal_error)?
                {
                    // Only what this attempt claimed: a value the item held before the schema
                    // was saved has to survive a rollback untouched.
                    Reservation::Claimed => claimed.push((item_id.clone(), value)),
                    Reservation::AlreadyHeld => {}
                    Reservation::Taken { owner } => {
                        return Err(HttpError::Conflict(&format!(
                            "field '{}': the value '{}' is already used by item {}",
                            value.field, value.value, owner
                        ))
                        .with_code("value_taken")
                        .with_field(&value.field));
                    }
                }
            }
        }
        Ok(())
    }

    /// Record when the item's values were last saved.
    ///
    /// `created` distinguishes the first save from a later one: a new item starts as a
    /// draft with both timestamps, while an edit only moves `updated_at` and leaves the
    /// status and the published dates exactly as they were.
    async fn stamp_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        created: bool,
    ) -> Result<(), HttpError> {
        if created {
            // Nothing else can be on the record of an item that was just created, so this write
            // has nothing to lose.
            let now = Utc::now();
            let metadata = ItemMetadata {
                created_at: Some(now),
                updated_at: Some(now),
                ..ItemMetadata::default()
            };
            return self
                .collection_repository
                .set_item_metadata(collection_name, &item_id, &metadata)
                .await
                .map_err(map_internal_error);
        }

        // A save and a publish write the same record, and this one only has `updated_at` to say.
        // Reading it here and writing it back whole would undo a publish that landed in between,
        // so the repository does the read and the write as one step.
        self.collection_repository
            .touch_item_metadata(collection_name, &item_id, Utc::now())
            .await
            .map_err(map_internal_error)
    }

    /// Delete one item, refusing while other content still points at it.
    ///
    /// `detach` is the caller saying "remove those references and go ahead" (`?detach=true`): every
    /// reference to the item is dropped from the content that holds one - both copies of it - and
    /// then the item goes. All of them first, so a failure part way through leaves the item in place
    /// with fewer references rather than gone with references to nothing.
    pub async fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        detach: bool,
    ) -> Result<(), HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?;
        if schema.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        let published = self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?;
        if published.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id.to_string(),
                collection_name
            )));
        }
        let owner = ItemOwner::collection_item(collection_name.as_str(), *item_id);
        let references = self
            .relations
            .get_relation_references(&owner)
            .await
            .map_err(map_internal_error)?;
        if !references.is_empty() {
            if !detach {
                return Err(HttpError::still_referenced(&references));
            }
            self.relations
                .detach_references(&owner)
                .await
                .map_err(map_internal_error)?;
        }

        // Read what the item holds before removing it: afterwards there is nothing left to say
        // which values it was using.
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default();
        let working = self
            .collection_repository
            .get_collection_item_draft(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?;
        let is_published = self
            .collection_repository
            .get_item_metadata(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default()
            .is_published();
        let held = Self::held_unique_values_for_status(
            &schema,
            is_published,
            published.as_ref(),
            working.as_ref(),
        );

        self.collection_repository
            .delete_collection_item(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?;
        // After the delete: a value is never given up while a copy still holds it.
        self.release_unique_values(collection_name, &item_id, &held)
            .await;
        // And nothing uses this item's images any more.
        self.set_image_references(&owner, &[]).await;
        Ok(())
    }

    /// Create an item from an **untagged** JSON body.
    ///
    /// This is the entry point the HTTP layer uses: values arrive without type tags, so
    /// the collection schema is what gives them meaning. Types that do not match are
    /// rejected with 400 rather than coerced.
    pub async fn create_collection_item_from_json(
        &self,
        collection_name: &CollectionName,
        body: &serde_json::Value,
    ) -> Result<u64, HttpError> {
        let item = self.parse_item(collection_name, body).await?;
        self.create_collection_item(collection_name, &item).await
    }

    /// Update an item from an **untagged** JSON body. See
    /// [`CollectionService::create_collection_item_from_json`].
    pub async fn update_collection_item_from_json(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        body: &serde_json::Value,
    ) -> Result<(), HttpError> {
        let item = self.parse_item(collection_name, body).await?;
        self.update_collection_item(collection_name, item_id, &item)
            .await
    }

    /// Read an untagged body into a [`CollectionItem`] using the collection schema.
    async fn parse_item(
        &self,
        collection_name: &CollectionName,
        body: &serde_json::Value,
    ) -> Result<CollectionItem, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
            .ok_or_else(|| {
                HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                ))
            })?;
        let composite_schemas = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?;
        CollectionItem::from_untyped(body, &composite_schemas, &schema)
            .map_err(|e| HttpError::BadRequest(&e))
    }

    // ---- draft / published ---------------------------------------------------

    /// The item as an editor sees it: the working copy if there is one, the published
    /// copy otherwise.
    async fn working_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<CollectionItem>, HttpError> {
        match self
            .collection_repository
            .get_collection_item_draft(collection_name, item_id)
            .await
            .map_err(map_internal_error)?
        {
            Some(draft) => Ok(Some(draft)),
            None => self
                .collection_repository
                .get_collection_item(collection_name, item_id)
                .await
                .map_err(map_internal_error),
        }
    }

    /// Every item as an editor sees it, ordered by id.
    ///
    /// The working copies are read in one scan and laid over the published ones, so a list
    /// of a collection costs two reads rather than one per item.
    async fn working_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItem)>, HttpError> {
        let mut drafts: HashMap<CollectionItemId, CollectionItem> = self
            .collection_repository
            .list_collection_item_drafts(collection_name)
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .collect();

        let mut items = self
            .collection_repository
            .list_collection_items(collection_name)
            .await
            .map_err(map_internal_error)?;
        // Ordered by id: it is what makes offset paging stable, and it keeps the admin list
        // from depending on the adapter's iteration order.
        items.sort_by_key(|(id, _)| **id);

        Ok(items
            .into_iter()
            .map(|(id, published)| {
                let working = drafts.remove(&id).unwrap_or(published);
                (id, working)
            })
            .collect())
    }

    /// Format stored items the way the HTTP layer reports them.
    async fn format_items(
        &self,
        schema: &CollectionSchema,
        items: Vec<(CollectionItemId, CollectionItem)>,
    ) -> Result<Vec<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        let composite_schema_map = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?;
        // The image library once per request: a field value may point at an image, and the
        // cheap way to answer that is one query rather than one per value.
        let images: HashMap<ImageId, Image> = self
            .image_repository
            .list_images()
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .collect();
        Ok(items
            .into_iter()
            .map(|(id, item)| {
                let formatted = item.format_to_schema(&composite_schema_map, schema);
                (id, formatted.to_response(&images))
            })
            .collect())
    }

    /// Format one stored item the way the HTTP layer reports it.
    async fn format_item(
        &self,
        collection_name: &CollectionName,
        item: &CollectionItem,
    ) -> Result<CollectionItemResponse, HttpError> {
        let schema = self.get_collection_schema(collection_name).await?;
        let composite_schema_map = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?;
        // The image library once per request: a field value may point at an image, and the
        // cheap way to answer that is one query rather than one per value.
        let images: HashMap<ImageId, Image> = self
            .image_repository
            .list_images()
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .collect();
        Ok(item
            .format_to_schema(&composite_schema_map, &schema)
            .to_response(&images))
    }

    /// Draft/published state of one item.
    ///
    /// Absent metadata means "draft": an item that was never published is not an error.
    pub async fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<ItemMetadata, HttpError> {
        self.require_item(collection_name, &item_id).await?;
        Ok(self
            .collection_repository
            .get_item_metadata(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default())
    }

    /// State when the content was created, changed or published - which is what a migration from
    /// another CMS has to be able to say.
    ///
    /// The record is read first, for two reasons: a patch that states one date has to be judged
    /// against the ones already there, and the caller is answered with what the record now says
    /// rather than with what it asked for. The write is a patch, so a publish landing while this
    /// runs keeps its publication time.
    pub async fn set_collection_item_dates(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        dates: &ItemDates,
    ) -> Result<ItemMetadata, HttpError> {
        // 404s for an item that is not there, which is also what stops a stray id from leaving a
        // metadata record behind with no item under it.
        let existing = self.get_item_metadata(collection_name, item_id).await?;
        dates.check(&existing, Utc::now()).map_err(|reason| {
            HttpError::BadRequest(&format!("those dates cannot be right: {reason}"))
        })?;
        if !dates.is_empty() {
            self.collection_repository
                .set_item_dates(collection_name, &item_id, dates)
                .await
                .map_err(map_internal_error)?;
        }
        self.get_item_metadata(collection_name, item_id).await
    }

    /// Record which images this item uses, so a delete can say what it would break.
    ///
    /// The **published** copy counts only while the item is published, which is the same rule the
    /// unique index uses: a draft keeps the record it was created from, nobody serves it, and
    /// counting it would name an item whose image fields are empty. A failure here is logged rather
    /// than returned - the content *was* saved, and refusing it over bookkeeping would be worse
    /// than a warning that misses.
    async fn record_image_references(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        working: &CollectionItem,
        published: Option<&CollectionItem>,
    ) {
        let mut images = crate::models::image::referenced_images(working);
        if let Some(published) = published {
            images.extend(crate::models::image::referenced_images(published));
            images.sort();
            images.dedup();
        }
        self.set_image_references(
            &crate::models::owner::ItemOwner::collection_item(collection_name.as_str(), **item_id),
            &images,
        )
        .await;
    }

    /// Write the reference index, and say so in the log when it cannot be written.
    async fn set_image_references(
        &self,
        owner: &crate::models::owner::ItemOwner,
        images: &[crate::models::image::ImageId],
    ) {
        if let Err(e) = self
            .image_repository
            .set_image_references(owner, images)
            .await
        {
            tracing::warn!("could not record image references for {owner:?}: {e}");
        }
    }

    /// The value a unique lookup is asking about, in the form the index holds it.
    ///
    /// A slug is stored canonical, so the question has to be canonical too; everything else is
    /// compared as it was typed, trimmed. A field that is not there, or not unique, answers with
    /// the trimmed value and the caller refuses it a moment later.
    fn lookup_value(schema: &CollectionSchema, field: &str, value: &str) -> String {
        let is_slug = schema
            .iter()
            .any(|candidate| candidate.name == field && candidate.is_slug());
        if is_slug {
            crate::models::slug::normalise(value)
        } else {
            value.trim().to_string()
        }
    }

    /// The item with its slug fields in canonical form.
    ///
    /// A slug is normalised when it is *written*, which is what makes the rest work: what is stored
    /// is what the unique index holds and what a URL resolves. A value that leaves nothing usable -
    /// a title in a script the rule cannot keep - is refused rather than stored as an empty slug;
    /// silently turning content into nothing is the one outcome worse than a refusal.
    fn normalise_slugs(
        &self,
        schema: &CollectionSchema,
        item: &CollectionItem,
    ) -> Result<CollectionItem, HttpError> {
        let mut values = item.0.clone();
        for field in schema {
            if !field.is_slug() {
                continue;
            }
            let Some(FieldValue::Text(text)) = values.get(&field.name) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let slug = crate::models::slug::normalise(text);
            if slug.is_empty() {
                return Err(FieldRefusal::invalid_slug(&field.name).into_http_error());
            }
            values.insert(field.name.clone(), FieldValue::Text(slug));
        }
        Ok(FieldValueMap(values, std::marker::PhantomData))
    }

    /// Metadata for every item, so the admin list can show a status for items that were
    /// never published (those have no stored record).
    pub async fn list_item_metadata(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, HttpError> {
        let stored: HashMap<CollectionItemId, ItemMetadata> = self
            .collection_repository
            .list_item_metadata(collection_name)
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .collect();
        let items = self
            .collection_repository
            .list_collection_items(collection_name)
            .await
            .map_err(map_internal_error)?;
        Ok(items
            .into_iter()
            .map(|(id, _)| {
                let metadata = stored.get(&id).cloned().unwrap_or_default();
                (id, metadata)
            })
            .collect())
    }

    /// Whether the item has an unpublished working copy.
    pub async fn has_draft(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<bool, HttpError> {
        self.require_item(collection_name, &item_id).await?;
        Ok(self
            .collection_repository
            .get_collection_item_draft(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?
            .is_some())
    }

    /// Ids of the items with an unpublished working copy, for the admin list.
    pub async fn draft_item_ids(
        &self,
        collection_name: &CollectionName,
    ) -> Result<HashSet<CollectionItemId>, HttpError> {
        Ok(self
            .collection_repository
            .list_collection_item_drafts(collection_name)
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// Publish or unpublish one item, recording when it happened and who did it.
    ///
    /// `actor` is required rather than optional: an audit trail with holes in it is worse
    /// than none, and every caller in the HTTP layer already has the authenticated user.
    pub async fn set_item_status(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        status: ItemStatus,
        actor: PublishedBy,
    ) -> Result<ItemMetadata, HttpError> {
        self.require_item(collection_name, &item_id).await?;
        // Built from the stored record so publishing keeps the first publication date and the
        // content timestamps.
        let stored = self
            .collection_repository
            .get_item_metadata(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default();
        let was_published = stored.is_published();
        let mut metadata = stored.with_status(status, Some(actor));

        // Publishing *is* the copy: whatever the editor has been working on replaces the
        // published item and stops being a separate draft. With nothing pending, publishing only
        // records who did it.
        //
        // Read first, then hand the whole change over in one call: the storage adapter applies
        // the copy, the removal and the status as one step, so no reader catches the item
        // between them (see `CollectionRepository::apply_item_status`).
        let pending = if metadata.is_published() {
            self.collection_repository
                .get_collection_item_draft(collection_name, &item_id)
                .await
                .map_err(map_internal_error)?
        } else {
            None
        };
        if pending.is_some() {
            // The content the site serves just changed, so the release moves `updated_at`.
            metadata = metadata.released(chrono::Utc::now());
        }
        // The index follows the copies. Publishing promotes the working copy, so the value the
        // *old* published copy held is given up; unpublishing gives it up too, because the item
        // is off the site and what is left is the working copy.
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default();
        // Read once, for the index and for the image references below: what the item holds in each
        // copy is what both of them are about.
        let published = self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?;
        let working = match &pending {
            Some(pending) => Some(pending.clone()),
            None => self
                .collection_repository
                .get_collection_item_draft(collection_name, &item_id)
                .await
                .map_err(map_internal_error)?,
        };
        // Publishing is where completeness is asked for. A working copy may be missing a required
        // field (a draft is what an editor is in the middle of), and the site must not be served
        // one: the refusal names the field, so the editor knows which one to fill in.
        if metadata.is_published() {
            if let Some(copy) = working.as_ref() {
                let composite_schemas = self
                    .composite_field_repository
                    .list_composite_field_schemas()
                    .await
                    .map_err(map_internal_error)?;
                copy.validate_to_schema(&composite_schemas, &schema)
                    .map_err(|e| e.into_http_error())?;
            }
        }
        let (reserve, release) = if has_unique_fields(&schema) {
            let before = Self::held_unique_values_for_status(
                &schema,
                was_published,
                published.as_ref(),
                working.as_ref(),
            );
            let after_published = pending.as_ref().or(published.as_ref());
            let after_working = if metadata.is_published() {
                None
            } else {
                working.as_ref()
            };
            let after = Self::held_unique_values_for_status(
                &schema,
                metadata.is_published(),
                after_published,
                after_working,
            );
            Self::unique_difference(&before, &after)
        } else {
            (Vec::new(), Vec::new())
        };
        self.reserve_unique_values(collection_name, &item_id, &reserve)
            .await?;
        if let Err(e) = self
            .collection_repository
            .apply_item_status(collection_name, &item_id, pending.as_ref(), &metadata)
            .await
        {
            self.release_unique_values(collection_name, &item_id, &reserve)
                .await;
            // The content moved between the read and the promotion, so this is the editor's turn:
            // what it has on screen is not what is stored, and only it knows whether to save again
            // or to publish what is there now.
            if e.downcast_ref::<ApplyStatusError>() == Some(&ApplyStatusError::DraftChanged) {
                return Err(HttpError::Conflict(
                    "the item was saved again while it was being published",
                )
                .with_code("draft_changed"));
            }
            return Err(map_internal_error(e));
        }
        self.release_unique_values(collection_name, &item_id, &release)
            .await;
        // What the item holds now: publishing promoted the working copy, unpublishing took it off
        // the site but kept the working copy, and a draft's old published record is not served.
        let held_now = pending.as_ref().or(working.as_ref()).or(published.as_ref());
        match held_now {
            Some(held_now) => {
                self.record_image_references(collection_name, &item_id, held_now, None)
                    .await
            }
            None => {
                self.set_image_references(
                    &crate::models::owner::ItemOwner::collection_item(
                        collection_name.as_str(),
                        *item_id,
                    ),
                    &[],
                )
                .await
            }
        }
        // Only after the status is stored: a receiver that reacts by reading the delivery
        // API must not see the previous state.
        self.notifier
            .notify(ContentEvent::collection_item(
                collection_name,
                item_id,
                &metadata,
            ))
            .await;
        Ok(metadata)
    }

    /// Items with the window the caller asked for. The total travels alongside, because
    /// `X-Total-Count` is what tells an admin caller there is more to fetch.
    pub async fn get_collection_items_page(
        &self,
        collection_name: &CollectionName,
        pagination: &Pagination,
    ) -> Result<Page<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        Ok(pagination.apply(self.get_collection_items(collection_name).await?))
    }

    /// Items visible to the public delivery API, with the metadata it reports.
    ///
    /// Ordered by id, which is the order the pages have to be walked in.
    pub async fn list_published_items(
        &self,
        collection_name: &CollectionName,
        pagination: &Pagination,
    ) -> Result<Page<(CollectionItemId, ItemMetadata, CollectionItemResponse)>, HttpError> {
        // The window goes to the storage, which can read a page of the *published* set
        // without materialising it (see `CollectionRepository::list_published_items_page`).
        // The delivery API is the one caller that walks a list it did not bound itself, so
        // this is where reading everything would actually hurt.
        let (offset, limit) = pagination.window();
        let (page, total) = self
            .collection_repository
            .list_published_items_page(collection_name, offset, limit)
            .await
            .map_err(map_internal_error)?;

        let schema = self.get_collection_schema(collection_name).await?;
        let mut stored = Vec::with_capacity(page.len());
        let mut metadata = Vec::with_capacity(page.len());
        for (id, item, item_metadata) in page {
            stored.push((id, item));
            metadata.push(item_metadata);
        }
        // The published copy, never the working one: edits must not leak to a site.
        let items = self.format_items(&schema, stored).await?;
        let items: Vec<(CollectionItemId, ItemMetadata, CollectionItemResponse)> = items
            .into_iter()
            .zip(metadata)
            .map(|((id, values), metadata)| (id, metadata, values))
            .collect();
        Ok(pagination.wrap(items, total))
    }

    /// One item, but only if it is published.
    ///
    /// A draft answers "not found" rather than "forbidden", so the delivery API does not
    /// reveal that unpublished content exists.
    pub async fn get_published_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<(ItemMetadata, CollectionItemResponse), HttpError> {
        let metadata = self.get_item_metadata(collection_name, item_id).await?;
        if !metadata.is_published() {
            return Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id, collection_name
            )));
        }
        let item = self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?
            .ok_or_else(|| {
                HttpError::NotFound(&format!(
                    "Item with id '{}' not found in collection '{}'",
                    item_id, collection_name
                ))
            })?;
        Ok((metadata, self.format_item(collection_name, &item).await?))
    }

    /// The published item whose published copy holds `value`, for a site resolving a URL.
    ///
    /// The index names the candidate; the published copy is what decides. That keeps a draft
    /// from publishing a value early: while a draft changes the value, the old one still
    /// resolves (it is what the site serves) and the new one answers "not found" until the
    /// change is released.
    pub async fn get_published_item_by_unique_value(
        &self,
        collection_name: &CollectionName,
        field: &str,
        value: &str,
    ) -> Result<(CollectionItemId, ItemMetadata, CollectionItemResponse), HttpError> {
        let (item_id, _) = self
            .get_item_by_unique_value(collection_name, field, value)
            .await?;
        let (metadata, values) = self.get_published_item(collection_name, item_id).await?;
        // Asked in the same form the index holds: for a slug that is the canonical spelling, so
        // `Hello World` and `hello-world` are one question rather than two.
        let value = Self::lookup_value(
            &self.get_collection_schema(collection_name).await?,
            field,
            value,
        );
        let holds_it = self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .await
            .map_err(map_internal_error)?
            .and_then(|item| match item.get(field) {
                Some(FieldValue::Text(text)) => Some(text.trim() == value),
                _ => None,
            })
            .unwrap_or(false);
        if !holds_it {
            return Err(HttpError::NotFound(&format!(
                "no published item holds {field} = '{value}'"
            )));
        }
        Ok((item_id, metadata, values))
    }

    /// Collections that have at least one published item.
    pub async fn list_collections_with_published_items(
        &self,
    ) -> Result<Vec<CollectionName>, HttpError> {
        let mut names = Vec::new();
        for name in self.list_collections().await? {
            if !self.published_item_ids(&name).await?.is_empty() {
                names.push(name);
            }
        }
        Ok(names)
    }

    async fn published_item_ids(
        &self,
        collection_name: &CollectionName,
    ) -> Result<HashSet<CollectionItemId>, HttpError> {
        Ok(self
            .collection_repository
            .list_item_metadata(collection_name)
            .await
            .map_err(map_internal_error)?
            .into_iter()
            .filter(|(_, metadata)| metadata.is_published())
            .map(|(id, _)| id)
            .collect())
    }

    async fn require_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), HttpError> {
        if self.working_item(collection_name, item_id).await?.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id, collection_name
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use crate::models::collection::CollectionName;
    use crate::models::image::{Image, ImageId, NewImageInfo, NewImageRequest, ReplacementInfo};
    use crate::models::schema::{CompositeFieldId, RelationOptions};
    use crate::models::values::{CompositeFieldSchema, FieldValueMap, TextFieldOptions};
    use crate::models::values::{FieldSchema, FieldType, FieldValue};
    use crate::repositories::image_repository::{ImageRepository, Replacement};

    use super::*;
    use crate::models::user::UserId;
    use crate::repositories::collection_repository::{Reservation, UniqueValue};
    use crate::repositories::relation_repository::NoRelations;
    use crate::repositories::relation_targets::StaticRelationTargets;
    use crate::webhook::NoopNotifier;
    use crate::webhook::NotifyFuture;

    struct MockCollectionRepository {
        schemas: Arc<RwLock<HashMap<CollectionName, CollectionSchema>>>,
        items: Arc<RwLock<HashMap<CollectionName, HashMap<CollectionItemId, CollectionItem>>>>,
        item_counter: Arc<RwLock<u64>>,
        item_metadata: Arc<RwLock<HashMap<(CollectionName, CollectionItemId), ItemMetadata>>>,
        drafts: Arc<RwLock<HashMap<(CollectionName, CollectionItemId), CollectionItem>>>,
        /// The unique index: (collection, field, value) to the item that holds it.
        unique: Arc<RwLock<HashMap<(CollectionName, String, String), CollectionItemId>>>,
        /// A store that refuses to write a schema, for the rollback a failed save has to do.
        fail_schema_save: bool,
        /// A store that answers "the working copy moved" to a promotion, for the refusal the
        /// service has to turn into a 409.
        fail_apply_status: bool,
        /// Fail the reservation whose number this is (1 for the first), for the half-claim a
        /// storage failure leaves behind.
        fail_reserve_on_call: Option<usize>,
        /// How many reservations have been asked for, so the above can count.
        reserve_calls: std::sync::atomic::AtomicUsize,
        /// Every whole-record metadata write. A save must not make one: it stamps the record
        /// through `touch_item_metadata`, which reads and writes as one step (see the trait).
        metadata_writes: std::sync::atomic::AtomicUsize,
    }
    impl CollectionRepository for MockCollectionRepository {
        async fn get_collection_schema(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Option<CollectionSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(collection_name).cloned())
        }
        async fn list_collection_names(
            &self,
        ) -> Result<Vec<CollectionName>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().keys().cloned().collect())
        }
        async fn add_collection_schema(
            &self,
            collection_name: &CollectionName,
            schema: &CollectionSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            if self.fail_schema_save {
                return Err("the schema store is unavailable".into());
            }
            self.schemas
                .write()
                .unwrap()
                .insert(collection_name.clone(), schema.clone());
            Ok(())
        }
        async fn delete_collection(
            &self,
            collection_name: &CollectionName,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(collection_name);
            self.item_metadata
                .write()
                .unwrap()
                .retain(|(name, _), _| name != collection_name);
            Ok(())
        }
        async fn list_collection_items(
            &self,
            collection_name: &CollectionName,
        ) -> Result<
            Vec<(CollectionItemId, CollectionItem)>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self
                .items
                .read()
                .unwrap()
                .get(collection_name)
                .map_or(vec![], |items_map| {
                    items_map
                        .iter()
                        .map(|(id, item)| (id.clone(), item.clone()))
                        .collect()
                }))
        }
        async fn get_collection_item(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<Option<CollectionItem>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            if let Some(items_map) = self.items.read().unwrap().get(collection_name) {
                Ok(items_map.get(item_id).cloned())
            } else {
                Ok(None)
            }
        }
        async fn add_collection_item(
            &self,
            collection_name: &CollectionName,
            item_data: &CollectionItem,
        ) -> Result<u64, Box<dyn std::error::Error + Send + Sync + 'static>> {
            let mut items_map = self.items.write().unwrap();
            let collection_items = items_map
                .entry(collection_name.clone())
                .or_insert_with(HashMap::new);
            let new_id = {
                let mut counter = self.item_counter.write().unwrap();
                *counter += 1;
                *counter
            };
            collection_items.insert(CollectionItemId::from_u64(new_id), item_data.clone());
            Ok(new_id)
        }
        async fn update_collection_item(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            item_data: &CollectionItem,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            if let Some(items_map) = self.items.write().unwrap().get_mut(collection_name) {
                items_map.insert(item_id.clone(), item_data.clone());
            }
            Ok(())
        }
        async fn delete_collection_item(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            if let Some(items_map) = self.items.write().unwrap().get_mut(collection_name) {
                items_map.remove(item_id);
            }
            self.item_metadata
                .write()
                .unwrap()
                .remove(&(collection_name.clone(), item_id.clone()));
            Ok(())
        }
        async fn get_item_metadata(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<Option<ItemMetadata>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self
                .item_metadata
                .read()
                .unwrap()
                .get(&(collection_name.clone(), item_id.clone()))
                .cloned())
        }
        async fn touch_item_metadata(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            now: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            // The real adapters read and write as one step; the double holds one lock, so the
            // same promise is kept by doing both here.
            let mut all = self.item_metadata.write().unwrap();
            let metadata = all
                .get(&(collection_name.clone(), item_id.clone()))
                .cloned()
                .unwrap_or_default()
                .touched(now);
            all.insert((collection_name.clone(), item_id.clone()), metadata);
            Ok(())
        }
        async fn set_item_dates(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            dates: &ItemDates,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            // A patch applied under the one lock, like the adapters.
            let mut all = self.item_metadata.write().unwrap();
            let metadata = all
                .get(&(collection_name.clone(), item_id.clone()))
                .cloned()
                .unwrap_or_default()
                .with_dates(dates);
            all.insert((collection_name.clone(), item_id.clone()), metadata);
            Ok(())
        }
        async fn set_item_metadata(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            metadata: &ItemMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.metadata_writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.item_metadata
                .write()
                .unwrap()
                .insert((collection_name.clone(), item_id.clone()), metadata.clone());
            Ok(())
        }
        async fn get_collection_item_draft(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<Option<CollectionItem>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self
                .drafts
                .read()
                .unwrap()
                .get(&(collection_name.clone(), item_id.clone()))
                .cloned())
        }
        async fn set_collection_item_draft(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            item_data: &CollectionItem,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.drafts.write().unwrap().insert(
                (collection_name.clone(), item_id.clone()),
                item_data.clone(),
            );
            Ok(())
        }
        async fn delete_collection_item_draft(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.drafts
                .write()
                .unwrap()
                .remove(&(collection_name.clone(), item_id.clone()));
            Ok(())
        }
        async fn list_collection_item_drafts(
            &self,
            collection_name: &CollectionName,
        ) -> Result<
            Vec<(CollectionItemId, CollectionItem)>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self
                .drafts
                .read()
                .unwrap()
                .iter()
                .filter(|((name, _), _)| name == collection_name)
                .map(|((_, id), item)| (id.clone(), item.clone()))
                .collect())
        }
        async fn apply_item_status(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            draft: Option<&CollectionItem>,
            metadata: &ItemMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            if self.fail_apply_status {
                return Err(Box::new(ApplyStatusError::DraftChanged));
            }
            // The mock has no transaction to offer, and nothing here can fail half-way, so it
            // does in a row what the real adapters do as one step.
            if let Some(draft) = draft {
                self.items
                    .write()
                    .unwrap()
                    .entry(collection_name.clone())
                    .or_default()
                    .insert(item_id.clone(), draft.clone());
                self.drafts
                    .write()
                    .unwrap()
                    .remove(&(collection_name.clone(), item_id.clone()));
            }
            self.item_metadata
                .write()
                .unwrap()
                .insert((collection_name.clone(), item_id.clone()), metadata.clone());
            Ok(())
        }
        /// An in-memory index, so the service's bookkeeping can be tested without a backend.
        async fn reserve_unique_value(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            unique: &UniqueValue,
        ) -> Result<Reservation, Box<dyn std::error::Error + Send + Sync + 'static>> {
            let call = self
                .reserve_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            if self.fail_reserve_on_call == Some(call) {
                return Err("the unique index is unavailable".into());
            }
            let mut index = self.unique.write().unwrap();
            let key = (
                collection_name.clone(),
                unique.field.clone(),
                unique.value.clone(),
            );
            match index.get(&key) {
                Some(owner) if owner != item_id => Ok(Reservation::Taken {
                    owner: owner.clone(),
                }),
                Some(_) => Ok(Reservation::AlreadyHeld),
                None => {
                    index.insert(key, item_id.clone());
                    Ok(Reservation::Claimed)
                }
            }
        }

        async fn list_unique_values(
            &self,
            collection_name: &CollectionName,
            field: &str,
        ) -> Result<
            Vec<(CollectionItemId, UniqueValue)>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self
                .unique
                .read()
                .unwrap()
                .iter()
                .filter(|((name, held_field, _), _)| name == collection_name && held_field == field)
                .map(|((_, held_field, value), owner)| {
                    (
                        owner.clone(),
                        UniqueValue {
                            field: held_field.clone(),
                            value: value.clone(),
                        },
                    )
                })
                .collect())
        }

        async fn find_unique_value(
            &self,
            collection_name: &CollectionName,
            unique: &UniqueValue,
        ) -> Result<Option<CollectionItemId>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            let index = self.unique.read().unwrap();
            let key = (
                collection_name.clone(),
                unique.field.clone(),
                unique.value.clone(),
            );
            Ok(index.get(&key).cloned())
        }

        async fn release_unique_value(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            unique: &UniqueValue,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            let mut index = self.unique.write().unwrap();
            let key = (
                collection_name.clone(),
                unique.field.clone(),
                unique.value.clone(),
            );
            if index.get(&key) == Some(item_id) {
                index.remove(&key);
            }
            Ok(())
        }

        async fn list_item_metadata(
            &self,
            collection_name: &CollectionName,
        ) -> Result<
            Vec<(CollectionItemId, ItemMetadata)>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self
                .item_metadata
                .read()
                .unwrap()
                .iter()
                .filter(|((name, _), _)| name == collection_name)
                .map(|((_, id), metadata)| (id.clone(), metadata.clone()))
                .collect())
        }
    }

    struct MockCompositeFieldRepository {
        schemas: Arc<RwLock<HashMap<CompositeFieldId, CompositeFieldSchema>>>,
    }
    impl CompositeFieldRepository for MockCompositeFieldRepository {
        async fn list_composite_field_schemas(
            &self,
        ) -> Result<
            HashMap<CompositeFieldId, CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().clone())
        }
        async fn get_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<Option<CompositeFieldSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(id).cloned())
        }
        async fn add_composite_field_schema(
            &self,
            id: &CompositeFieldId,
            schema: &CompositeFieldSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas
                .write()
                .unwrap()
                .insert(id.clone(), schema.clone());
            Ok(())
        }
        async fn delete_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(id);
            Ok(())
        }
    }

    /// The image store a service test needs: what exists, and which file each record names.
    ///
    /// `image_file_name` answers from `file_names` rather than from the URLs this double invents,
    /// which is the same rule the adapters follow: the file name is a fact about the record, not
    /// something to be read back out of however the image happens to be served.
    #[derive(Default)]
    struct MockImageRepository {
        file_names: std::sync::RwLock<std::collections::HashMap<ImageId, String>>,
    }
    impl ImageRepository for MockImageRepository {
        async fn get_image(
            &self,
            id: &ImageId,
        ) -> Result<Option<Image>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(Some(Image {
                original_filename: format!("image_{}.jpg", id),
                url: format!("/images/{}", id),
                uploaded_at: chrono::Utc::now(),
                deleted_at: None,
            }))
        }
        async fn list_images(
            &self,
        ) -> Result<Vec<(ImageId, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(vec![])
        }
        async fn generate_image_upload_url(
            &self,
            _upload_info: &NewImageRequest,
        ) -> Result<NewImageInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(NewImageInfo {
                id: ImageId::from_u64(1),
                upload_url: "/upload/1".to_string(),
                url: "/images/1".to_string(),
            })
        }
        async fn generate_replacement_upload_url(
            &self,
            _id: &ImageId,
            request: &crate::models::image::ReplaceImageRequest,
        ) -> Result<ReplacementInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
            let ext = request.ext.as_str();
            Ok(ReplacementInfo {
                file_name: format!("replacement.{ext}"),
                upload_url: "/upload/replacement".to_string(),
            })
        }
        async fn image_bytes_exist(
            &self,
            _file_name: &str,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(true)
        }
        async fn replace_image(
            &self,
            _id: &ImageId,
            _file_name: &str,
        ) -> Result<Replacement, Box<dyn std::error::Error + Send + Sync + 'static>> {
            // These tests never go through a replacement; answering `Applied` keeps the double
            // out of the way of the content they are about.
            Ok(Replacement::Applied)
        }
        async fn rename_image(
            &self,
            _id: &ImageId,
            _original_filename: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn set_image_uploaded_at(
            &self,
            _id: &ImageId,
            _uploaded_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn delete_image(
            &self,
            _id: &ImageId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn image_file_name(
            &self,
            id: &ImageId,
        ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.file_names.read().unwrap().get(id).cloned())
        }

        async fn set_image_references(
            &self,
            _owner: &crate::models::owner::ItemOwner,
            _images: &[ImageId],
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn get_image_references(
            &self,
            _id: &ImageId,
        ) -> Result<
            Vec<crate::models::owner::ItemOwner>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(Vec::new())
        }
        async fn set_image_deleted_at(
            &self,
            _id: &ImageId,
            _at: Option<chrono::DateTime<chrono::Utc>>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
    }

    /// Records the events a service emits, so the notification contract can be asserted
    /// without standing up an HTTP receiver.
    #[derive(Default)]
    struct RecordingNotifier {
        events: std::sync::Mutex<Vec<ContentEvent>>,
    }

    impl Notifier for RecordingNotifier {
        fn notify(&self, event: ContentEvent) -> NotifyFuture<'_> {
            self.events.lock().unwrap().push(event);
            Box::pin(async {})
        }
    }

    #[tokio::test]
    async fn a_status_change_notifies_once_and_a_missing_item_notifies_nothing() {
        let notifier = Arc::new(RecordingNotifier::default());
        let mut schemas = HashMap::new();
        schemas.insert("blog".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            notifier.clone(),
        );

        let item_id = service
            .create_collection_item(&"blog".into(), &create_test_item("Hello", 1.0))
            .await
            .unwrap();

        let metadata = service
            .set_item_status(
                &"blog".into(),
                CollectionItemId::from_u64(item_id),
                ItemStatus::Published,
                publisher(),
            )
            .await
            .unwrap();
        // Publishing leaves an audit trail: who did it, alongside when.
        assert_eq!(
            metadata
                .published_by
                .as_ref()
                .map(|by| by.username.as_str()),
            Some("admin@example.com")
        );
        {
            let events = notifier.events.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), "collection_item.published");
            assert_eq!(events[0].collection.as_ref().unwrap().as_str(), "blog");
            assert_eq!(*events[0].item_id.unwrap(), item_id);
        }

        // A publish that fails (unknown item) must not claim that anything changed.
        assert!(
            service
                .set_item_status(
                    &"blog".into(),
                    CollectionItemId::from_u64(99),
                    ItemStatus::Published,
                    publisher()
                )
                .await
                .is_err()
        );
        assert_eq!(notifier.events.lock().unwrap().len(), 1);
    }

    /// The account the tests publish as.
    fn publisher() -> PublishedBy {
        PublishedBy::from(&crate::models::user::User::new(
            "admin@example.com",
            true,
            crate::models::user::Permission::admin(),
        ))
    }

    // Test helper functions
    fn create_test_service() -> CollectionService<
        MockCollectionRepository,
        MockCompositeFieldRepository,
        MockImageRepository,
    > {
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        )
    }

    fn create_test_schema() -> CollectionSchema {
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
                name: "count".to_string(),
                field_type: FieldType::Number,
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
        ]
    }

    fn create_test_item(title: &str, count: f64) -> CollectionItem {
        FieldValueMap(
            HashMap::from([
                ("title".to_string(), FieldValue::Text(title.to_string())),
                ("count".to_string(), FieldValue::Number(Some(count))),
            ]),
            std::marker::PhantomData,
        )
    }

    fn create_test_item_response(title: &str, count: f64) -> CollectionItemResponse {
        use crate::models::values::FieldValueResponse;
        HashMap::from([
            (
                "title".to_string(),
                FieldValueResponse::Text(title.to_string()),
            ),
            ("count".to_string(), FieldValueResponse::Number(Some(count))),
        ])
    }

    #[tokio::test]
    async fn create_collection_service() {
        let service = create_test_service();
        assert!(service.list_collections().await.is_ok());
    }
    #[tokio::test]
    async fn get_all_collections_empty() {
        let service = create_test_service();
        let collection_names = service.list_collections().await.unwrap();
        assert_eq!(collection_names.len(), 0);
    }

    #[tokio::test]
    async fn get_collection_schema_not_found() {
        let service = create_test_service();
        let result = service.get_collection_schema(&"non_existent".into()).await;
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Collection not found")
        );
    }

    #[tokio::test]
    async fn add_collection_schema_success() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];

        let result = service
            .add_collection_schema(&"test_collection".into(), &schema)
            .await;
        assert!(result.is_ok());

        let collection_names = service.list_collections().await.unwrap();
        assert_eq!(collection_names, vec!["test_collection".into()]);

        let retrieved_schema = service
            .get_collection_schema(&"test_collection".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    /// A relation is a promise that the other end is there, so a schema naming something this
    /// site does not have is refused rather than stored as a field nobody could ever fill in.
    #[tokio::test]
    async fn a_schema_may_only_relate_to_what_the_site_has() {
        let name = CollectionName::from("blog");
        let service = service_over_with_targets(
            test_repository(HashMap::new(), false),
            vec![
                RelationTarget::Collection {
                    name: "authors".to_string(),
                },
                RelationTarget::SinglePage {
                    name: "home".to_string(),
                },
            ],
        );

        let related_to = |target: RelationTarget| {
            vec![FieldSchema {
                name: "author".to_string(),
                field_type: FieldType::Relation(RelationOptions {
                    target,
                    has_many: false,
                    inverse_name: None,
                }),
                required: false,
                width: 12,
                height: 1,
                unique: false,
            }]
        };
        let collection = |name: &str| RelationTarget::Collection {
            name: name.to_string(),
        };

        service
            .add_collection_schema(&name, &related_to(collection("authors")))
            .await
            .unwrap();
        // A single page is a target as much as a collection is.
        service
            .update_collection_schema(
                &name,
                &related_to(RelationTarget::SinglePage {
                    name: "home".to_string(),
                }),
            )
            .await
            .unwrap();

        // Nothing of that name: refused, and the message says which kind was wanted.
        let missing = service
            .update_collection_schema(&name, &related_to(collection("writers")))
            .await
            .unwrap_err();
        assert_eq!(missing.status_code, 400, "{}", missing.message);
        assert!(
            missing.message.contains("'writers' is not a collection"),
            "{}",
            missing.message
        );

        // A name that exists as the other kind is a different target, so it is missing too.
        let wrong_kind = service
            .update_collection_schema(&name, &related_to(collection("home")))
            .await
            .unwrap_err();
        assert_eq!(wrong_kind.status_code, 400, "{}", wrong_kind.message);
        assert!(
            wrong_kind.message.contains("'home' is not a collection"),
            "{}",
            wrong_kind.message
        );
    }

    /// Everything a mock store needs, with the index and the schemas the test placed in it.
    fn test_repository(
        schemas: HashMap<CollectionName, CollectionSchema>,
        fail_schema_save: bool,
    ) -> Arc<MockCollectionRepository> {
        test_repository_that(schemas, fail_schema_save, false)
    }

    fn test_repository_that(
        schemas: HashMap<CollectionName, CollectionSchema>,
        fail_schema_save: bool,
        fail_apply_status: bool,
    ) -> Arc<MockCollectionRepository> {
        test_repository_failing_reserve(schemas, fail_schema_save, fail_apply_status, None)
    }

    fn test_repository_failing_reserve(
        schemas: HashMap<CollectionName, CollectionSchema>,
        fail_schema_save: bool,
        fail_apply_status: bool,
        fail_reserve_on_call: Option<usize>,
    ) -> Arc<MockCollectionRepository> {
        Arc::new(MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save,
            fail_apply_status,
            fail_reserve_on_call,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn service_over(
        repository: Arc<MockCollectionRepository>,
    ) -> CollectionService<
        MockCollectionRepository,
        MockCompositeFieldRepository,
        MockImageRepository,
    > {
        service_over_with_targets(repository, Vec::new())
    }

    /// The same, for a site that has something a relation could point at.
    fn service_over_with_targets(
        repository: Arc<MockCollectionRepository>,
        targets: Vec<RelationTarget>,
    ) -> CollectionService<
        MockCollectionRepository,
        MockCompositeFieldRepository,
        MockImageRepository,
    > {
        CollectionService::new(
            repository,
            Arc::new(MockCompositeFieldRepository {
                schemas: Arc::new(RwLock::new(HashMap::new())),
            }),
            Arc::new(MockImageRepository::default()),
            Arc::new(StaticRelationTargets::new(targets)),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        )
    }

    fn text_field(name: &str, unique: bool) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: false,
            width: 12,
            height: 1,
            unique,
        }
    }

    fn item_with(field: &str, value: &str) -> CollectionItem {
        FieldValueMap(
            HashMap::from([(field.to_string(), FieldValue::Text(value.to_string()))]),
            std::marker::PhantomData,
        )
    }

    /// The schema save re-claims every value the collection holds - that is what puts the index in
    /// step with a new schema - and a rollback used to give *all* of them back, including the ones
    /// the items already held. The index was then missing entries for values that were still in
    /// use: lookups failed, and a second item could take a value that was taken.
    #[tokio::test]
    async fn a_failed_schema_save_leaves_the_values_the_collection_already_holds() {
        let name = CollectionName::from("blog");
        let schema = vec![text_field("title", true)];
        let repository = test_repository(HashMap::from([(name.clone(), schema.clone())]), true);
        let service = service_over(repository);

        let held = CollectionItemId::from_u64(
            service
                .create_collection_item(&name, &item_with("title", "kept"))
                .await
                .unwrap(),
        );
        assert_eq!(
            service
                .get_item_by_unique_value(&name, "title", "kept")
                .await
                .unwrap()
                .0,
            held
        );

        // The save fails, after the backfill had re-claimed `kept` for the new schema.
        let changed = vec![text_field("title", true), text_field("subtitle", false)];
        let failure = service
            .update_collection_schema(&name, &changed)
            .await
            .unwrap_err();
        assert_eq!(failure.status_code, 500, "{}", failure.message);

        // The value still belongs to the item that holds it...
        assert_eq!(
            service
                .get_item_by_unique_value(&name, "title", "kept")
                .await
                .unwrap()
                .0,
            held
        );
        // ...so nobody else can take it.
        let stolen = service
            .create_collection_item(&name, &item_with("title", "kept"))
            .await
            .unwrap_err();
        assert_eq!(stolen.status_code, 409);
        assert_eq!(stolen.code, "value_taken");
    }

    /// A promotion refused because the working copy moved is not a storage failure: it is the
    /// editor's turn, so the answer carries the code the screen has wording for.
    #[tokio::test]
    async fn a_publish_refused_because_the_working_copy_moved_answers_409() {
        let name = CollectionName::from("blog");
        let schema = vec![text_field("title", false)];
        let repository = test_repository_that(HashMap::from([(name.clone(), schema)]), false, true);
        let service = service_over(repository);

        let id = CollectionItemId::from_u64(
            service
                .create_collection_item(&name, &item_with("title", "kept"))
                .await
                .unwrap(),
        );
        let refusal = service
            .set_item_status(
                &name,
                id,
                ItemStatus::Published,
                PublishedBy {
                    id: UserId::from("user-1"),
                    username: "admin".to_string(),
                },
            )
            .await
            .unwrap_err();

        assert_eq!(refusal.status_code, 409);
        assert_eq!(refusal.code, "draft_changed");
    }

    /// A storage failure while claiming the second of two values used to leave the first claimed
    /// by an item that was never saved, so a value nothing holds stayed blocked for everyone.
    #[tokio::test]
    async fn a_failed_reservation_gives_back_what_the_save_already_claimed() {
        let name = CollectionName::from("blog");
        let schema = vec![text_field("title", true), text_field("code", true)];
        // The second reservation is the one that fails.
        let repository = test_repository_failing_reserve(
            HashMap::from([(name.clone(), schema)]),
            false,
            false,
            Some(2),
        );
        let service = service_over(repository);

        let mut item = item_with("title", "first");
        item.0
            .insert("code".to_string(), FieldValue::Text("one".to_string()));
        let failure = service
            .create_collection_item(&name, &item)
            .await
            .unwrap_err();
        assert_eq!(failure.status_code, 500, "{}", failure.message);

        // The title the failed save claimed is free again: the store no longer fails, so the
        // claim succeeds if it was given back.
        let mut retry = item_with("title", "first");
        retry
            .0
            .insert("code".to_string(), FieldValue::Text("two".to_string()));
        let created = service.create_collection_item(&name, &retry).await;
        assert!(
            created.is_ok(),
            "the title is still claimed by the failed save: {created:?}"
        );
    }

    /// The schema scan claims values for a schema that is not saved yet, so *every* way it can end
    /// early has to give them back - not only the refusal by an owner. A storage failure and a
    /// slug that is not canonical yet both used to leave the earlier claims behind, and the values
    /// they named stayed blocked by a constraint that was never saved.
    #[tokio::test]
    async fn a_schema_scan_that_stops_early_gives_back_everything_it_claimed() {
        // One item holding a plain unique value and one slug that is not canonical yet: the scan
        // claims the first, then refuses the second.
        let name = CollectionName::from("blog");
        let stored = vec![text_field("title", false), text_field("address", false)];
        let mut item = item_with("title", "held");
        item.0.insert(
            "address".to_string(),
            FieldValue::Text("Not A Slug".to_string()),
        );
        let repository = test_repository(HashMap::from([(name.clone(), stored.clone())]), false);
        repository
            .items
            .write()
            .unwrap()
            .entry(name.clone())
            .or_default()
            .insert(CollectionItemId::from_u64(1), item);
        repository.item_counter.write().unwrap().clone_from(&mut 1);
        let service = service_over(repository.clone());

        let adding_slug = vec![
            text_field("title", true),
            FieldSchema {
                name: "address".to_string(),
                field_type: FieldType::Slug(Default::default()),
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
        ];
        let refusal = service
            .update_collection_schema(&name, &adding_slug)
            .await
            .unwrap_err();
        assert_eq!(refusal.code, "invalid_slug");

        // The title it claimed on the way is not in the index any more.
        assert!(
            repository.unique.read().unwrap().is_empty(),
            "{:?}",
            repository.unique.read().unwrap()
        );

        // And a storage failure part-way through a scan gives back the same way.
        let both_unique = vec![text_field("title", true), text_field("code", true)];
        let mut two = item_with("title", "held");
        two.0
            .insert("code".to_string(), FieldValue::Text("one".to_string()));
        let repository = test_repository_failing_reserve(
            HashMap::from([(name.clone(), stored)]),
            false,
            false,
            Some(2),
        );
        repository
            .items
            .write()
            .unwrap()
            .entry(name.clone())
            .or_default()
            .insert(CollectionItemId::from_u64(1), two);
        let service = service_over(repository.clone());

        let failure = service
            .update_collection_schema(&name, &both_unique)
            .await
            .unwrap_err();
        assert_eq!(failure.status_code, 500, "{}", failure.message);
        assert!(
            repository.unique.read().unwrap().is_empty(),
            "{:?}",
            repository.unique.read().unwrap()
        );
    }

    #[tokio::test]
    async fn update_collection_schema_success() {
        let service = create_test_service();
        let initial_schema = vec![FieldSchema {
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_collection_schema(&"test_collection".into(), &initial_schema)
            .await
            .unwrap();

        let updated_schema = vec![FieldSchema {
            name: "title2".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        let result = service
            .update_collection_schema(&"test_collection".into(), &updated_schema)
            .await;
        assert!(result.is_ok());

        let retrieved_schema = service
            .get_collection_schema(&"test_collection".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[tokio::test]
    async fn add_collection_schema_already_exists() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_collection_schema(&"test_collection".into(), &schema)
            .await
            .unwrap();

        let duplicate_schema = vec![FieldSchema {
            name: "other".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        let result = service
            .add_collection_schema(&"test_collection".into(), &duplicate_schema)
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::Conflict("Collection with id 'test_collection' already exists")
        );

        let retrieved_schema = service
            .get_collection_schema(&"test_collection".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[tokio::test]
    async fn delete_collection_success() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_collection_schema(&"test_collection".into(), &schema)
            .await
            .unwrap();

        let result = service.delete_collection(&"test_collection".into()).await;
        assert!(result.is_ok());

        let collection_names = service.list_collections().await.unwrap();
        assert_eq!(collection_names.len(), 0);
    }

    #[tokio::test]
    async fn update_missing_schema() {
        let service = create_test_service();
        let update_result = service
            .update_collection_schema(
                &"non_existent".into(),
                &vec![FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                }],
            )
            .await;
        assert!(update_result.is_err());
    }
    #[tokio::test]
    async fn create_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .create_collection_item(
                &"test_composite".into(),
                &create_test_item("Test Title", 42.0),
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[tokio::test]
    async fn create_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service
            .create_collection_item(
                &"non_existent".into(),
                &FieldValueMap(HashMap::new(), std::marker::PhantomData),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Collection with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn a_working_copy_may_be_missing_a_required_field() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        // A working copy is what an editor is in the middle of, so a required field may still be
        // empty: the site is not served from it, and refusing the save would mean an editor cannot
        // save half-written work (or a schema that gained a required field cannot be saved at all).
        // Publication is what asks for completeness, and the tests for it are in the contract
        // suite.
        let result = service
            .create_collection_item(
                &"test_composite".into(),
                &FieldValueMap(
                    HashMap::from([("count".to_string(), FieldValue::Number(Some(42.0)))]),
                    std::marker::PhantomData,
                ),
            )
            .await;
        assert!(result.is_ok(), "an incomplete draft is a draft: {result:?}");
    }

    #[tokio::test]
    async fn get_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(
            CollectionItemId::from_u64(1),
            create_test_item("Sample Title", 10.0),
        );
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1))
            .await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            create_test_item_response("Sample Title", 10.0)
        );
    }

    #[tokio::test]
    async fn get_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(999))
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Item with id '999' not found in collection 'test_composite'")
        );
    }

    #[tokio::test]
    async fn get_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service
            .get_collection_item(&"non_existent".into(), CollectionItemId::from_u64(1))
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Collection with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn update_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(
            CollectionItemId::from_u64(1),
            create_test_item("Original Title", 10.0),
        );
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .update_collection_item(
                &"test_composite".into(),
                CollectionItemId::from_u64(1),
                &create_test_item("Updated Title", 100.0),
            )
            .await;
        assert!(result.is_ok());

        let item = service
            .get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1))
            .await
            .unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    /// A save stamps the record through the repository's one-step touch, never by writing back the
    /// metadata it read: that second shape loses a publish that lands between the read and the
    /// write (see `CollectionRepository::touch_item_metadata`).
    #[tokio::test]
    async fn a_save_stamps_the_metadata_without_writing_it_back() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(
            CollectionItemId::from_u64(1),
            create_test_item("Original Title", 10.0),
        );
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let repository = Arc::new(MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        });
        // The item is published before the save, which is the state the save must leave alone.
        let published_at = chrono::Utc::now() - chrono::Duration::hours(1);
        repository.item_metadata.write().unwrap().insert(
            ("test_composite".into(), CollectionItemId::from_u64(1)),
            ItemMetadata {
                status: crate::models::item_status::ItemStatus::Published,
                published_at: Some(published_at),
                ..ItemMetadata::default()
            },
        );
        let service = CollectionService::new(
            repository.clone(),
            Arc::new(MockCompositeFieldRepository {
                schemas: Arc::new(RwLock::new(HashMap::new())),
            }),
            Arc::new(MockImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        service
            .update_collection_item(
                &"test_composite".into(),
                CollectionItemId::from_u64(1),
                &create_test_item("Updated Title", 100.0),
            )
            .await
            .unwrap();

        assert_eq!(
            repository
                .metadata_writes
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "a save must not write the whole metadata record"
        );
        let stored = repository
            .item_metadata
            .read()
            .unwrap()
            .get(&("test_composite".into(), CollectionItemId::from_u64(1)))
            .cloned()
            .expect("the record the save stamped");
        assert_eq!(
            stored.status,
            crate::models::item_status::ItemStatus::Published,
            "the publication state belongs to a publish, not to a save"
        );
        assert_eq!(stored.published_at, Some(published_at));
        assert!(
            stored.updated_at.is_some(),
            "and the save is still recorded"
        );
    }

    #[tokio::test]
    async fn update_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .update_collection_item(
                &"test_composite".into(),
                CollectionItemId::from_u64(999),
                &create_test_item("Updated Title", 100.0),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Item with id '999' not found in collection 'test_composite'")
        );
    }

    #[tokio::test]
    async fn update_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service
            .update_collection_item(
                &"non_existent".into(),
                CollectionItemId::from_u64(1),
                &create_test_item("Updated Title", 100.0),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Collection with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn a_save_may_leave_a_required_field_empty() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(
            CollectionItemId::from_u64(1),
            create_test_item("Original Title", 10.0),
        );
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        // The same as a create: a save stores a working copy, and a working copy may be
        // incomplete (see `a_working_copy_may_be_missing_a_required_field`).
        let result = service
            .update_collection_item(
                &"test_composite".into(),
                CollectionItemId::from_u64(1),
                &FieldValueMap(
                    HashMap::from([("count".to_string(), FieldValue::Number(Some(100.0)))]),
                    std::marker::PhantomData,
                ),
            )
            .await;
        assert!(result.is_ok(), "an incomplete draft is a draft: {result:?}");
    }

    #[tokio::test]
    async fn get_collection_items_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(
            CollectionItemId::from_u64(1),
            create_test_item("Sample Title", 10.0),
        );
        collection_item.insert(
            CollectionItemId::from_u64(2),
            create_test_item("Another Title", 20.0),
        );
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(2)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service.get_collection_items(&"test_composite".into()).await;
        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.contains(&(
            CollectionItemId::from_u64(1),
            create_test_item_response("Sample Title", 10.0)
        )));
        assert!(items.contains(&(
            CollectionItemId::from_u64(2),
            create_test_item_response("Another Title", 20.0)
        )));
    }

    #[tokio::test]
    async fn get_collection_items_missing_collection() {
        let service = create_test_service();
        let result = service.get_collection_items(&"non_existent".into()).await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Collection with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn delete_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(
            CollectionItemId::from_u64(1),
            create_test_item("Sample Title", 10.0),
        );
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .delete_collection_item(
                &"test_composite".into(),
                CollectionItemId::from_u64(1),
                false,
            )
            .await;
        assert!(result.is_ok());

        let get_result = service
            .get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1))
            .await;
        assert!(get_result.is_err());
        assert_eq!(
            get_result.err().unwrap(),
            HttpError::NotFound("Item with id '1' not found in collection 'test_composite'")
        );
    }

    #[tokio::test]
    async fn delete_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            unique: Arc::new(RwLock::new(HashMap::new())),
            fail_schema_save: false,
            fail_apply_status: false,
            fail_reserve_on_call: None,
            reserve_calls: std::sync::atomic::AtomicUsize::new(0),
            metadata_writes: std::sync::atomic::AtomicUsize::new(0),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoopNotifier),
        );

        let result = service
            .delete_collection_item(
                &"test_composite".into(),
                CollectionItemId::from_u64(999),
                false,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Item with id '999' not found in collection 'test_composite'")
        );
    }

    #[tokio::test]
    async fn delete_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service
            .delete_collection_item(&"non_existent".into(), CollectionItemId::from_u64(1), false)
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Collection with id 'non_existent' does not exist")
        );
    }
}
