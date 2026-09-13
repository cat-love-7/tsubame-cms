use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;

use crate::models::collection::{CollectionItem, CollectionItemId, CollectionItemResponse, CollectionName, CollectionSchema};
use crate::models::error::{HttpError, map_internal_error};
use crate::models::item_status::{ItemMetadata, ItemStatus, PublishedBy};
use crate::models::pagination::{Page, Pagination};
use crate::models::schema::{validate_composite_references, validate_schema, CompositeFieldId};
use crate::repositories::collection_repository::CollectionRepository;
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::ImageRepository;
use crate::webhook::{ContentEvent, Notifier};

pub struct CollectionService<CR: CollectionRepository, CFR: CompositeFieldRepository, IR: ImageRepository> {
    collection_repository: Arc<CR>,
    composite_field_repository: Arc<CFR>,
    image_repository: Arc<IR>,
    /// Told about every publish/unpublish so a site build can be triggered.
    notifier: Arc<dyn Notifier>,
}


impl<CR: CollectionRepository, CFR: CompositeFieldRepository, IR: ImageRepository> CollectionService<CR, CFR, IR> {
    pub fn new(collection_repository: Arc<CR>, composite_field_repository: Arc<CFR>, image_repository: Arc<IR>, notifier: Arc<dyn Notifier>) -> Self {
        CollectionService {
            collection_repository,
            composite_field_repository,
            image_repository,
            notifier,
        }
    }
    pub fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<CollectionSchema, HttpError> {
        let collection = self.collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match collection {
            Some(schema) => Ok(schema),
            None => Err(HttpError::NotFound("Collection not found")),
        }
    }
    pub fn update_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema)?;
        if self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        self.collection_repository
            .add_collection_schema(collection_name, schema)
            .map_err(map_internal_error)
    }
    pub fn get_all_collections(&self) -> Result<Vec<CollectionName>, HttpError> {
        self.collection_repository
            .list_collection_names()
            .map_err(map_internal_error)
    }

    /// Every composite a schema references must exist, otherwise the schema can be stored
    /// but never used to read or write values.
    fn ensure_composites_exist(&self, schema: &CollectionSchema) -> Result<(), HttpError> {
        let available: HashSet<CompositeFieldId> = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?
            .into_keys()
            .collect();
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))
    }
    pub fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema)?;
        if self
            .collection_repository
            .get_collection_schema(collection_name)
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
            .map_err(map_internal_error)
    }
    pub fn delete_collection(&self, collection_name: &CollectionName) -> Result<(), HttpError> {
        if self
            .collection_repository
            .get_collection_schema(collection_name)
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
            .map_err(map_internal_error)
    }
    pub fn get_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match schema {
            None => {
                Err(HttpError::NotFound(&format!(
                    "Collection with id '{}' does not exist",
                    collection_name
                )))
            }
            Some(schema) => {
                // The working copies: this is the screen an editor saves from.
                let items = self.working_items(collection_name)?;
                self.format_items(&schema, items)
            },
        }
    }
    pub fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<CollectionItemResponse, HttpError> {
        let schema = self
                    .collection_repository
                    .get_collection_schema(collection_name)
                    .map_err(map_internal_error)?;
        if schema.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        let item = self.working_item(collection_name, &item_id)?;
        match item {
            Some(item) => self.format_item(collection_name, &item),
            None => Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id.to_string(), collection_name
            ))),
        }
    }
    pub fn create_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &CollectionItem,
    ) -> Result<u64, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        match schema {
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
                    .map_err(map_internal_error)?;
                item_data.validate_to_schema(&composite_schema_map, &schema)
                    .map_err(|e| HttpError::BadRequest(&e.to_string()))?;
            }
        }
        let item_id = CollectionItemId::from_u64(
            self.collection_repository
                .add_collection_item(collection_name, item_data)
                .map_err(map_internal_error)?,
        );
        // The new item starts as a working copy; publishing is what puts it in front of
        // the delivery API.
        self.collection_repository
            .set_collection_item_draft(collection_name, &item_id, item_data)
            .map_err(map_internal_error)?;
        self.stamp_item(collection_name, item_id, true)?;
        Ok(*item_id)
    }
    pub fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        item_data: &CollectionItem,
    ) -> Result<(), HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
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
                    .map_err(|e| HttpError::InternalServerError(&e.to_string()))?
                    .is_none()
                {
                    return Err(HttpError::NotFound(&format!(
                        "Item with id '{}' not found in collection '{}'",
                        item_id.to_string(), collection_name
                    )));
                }
                item_data.validate_to_schema(
                    &self
                        .composite_field_repository
                        .list_composite_field_schemas()
                        .map_err(|e| HttpError::InternalServerError(&e.to_string()))?,
                    &schema,
                ).map_err(|e| HttpError::BadRequest(&e.to_string()))?;

                // Saved into the working copy: the published item keeps serving the live
                // site until this version is published.
                self.collection_repository
                    .set_collection_item_draft(collection_name, &item_id, item_data)
                    .map_err(|e| HttpError::InternalServerError(&e.to_string()))?;
                self.stamp_item(collection_name, item_id, false)?;
                Ok(())
            }
        }
    }

    /// Record when the item's values were last saved.
    ///
    /// `created` distinguishes the first save from a later one: a new item starts as a
    /// draft with both timestamps, while an edit only moves `updated_at` and leaves the
    /// status and `published_at` exactly as they were.
    fn stamp_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        created: bool,
    ) -> Result<(), HttpError> {
        let now = Utc::now();
        let metadata = if created {
            ItemMetadata {
                created_at: Some(now),
                updated_at: Some(now),
                ..ItemMetadata::default()
            }
        } else {
            self.collection_repository
                .get_item_metadata(collection_name, &item_id)
                .map_err(map_internal_error)?
                .unwrap_or_default()
                .touched(now)
        };

        self.collection_repository
            .set_item_metadata(collection_name, &item_id, &metadata)
            .map_err(map_internal_error)
    }

    pub fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<(), HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
            .map_err(map_internal_error)?;
        if schema.is_none() {
            return Err(HttpError::NotFound(&format!(
                "Collection with id '{}' does not exist",
                collection_name
            )));
        }
        if self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id.to_string(), collection_name
            )));
        }

        self.collection_repository
            .delete_collection_item(collection_name, &item_id)
            .map_err(map_internal_error)?;
        Ok(())
    }

    /// Create an item from an **untagged** JSON body.
    ///
    /// This is the entry point the HTTP layer uses: values arrive without type tags, so
    /// the collection schema is what gives them meaning. Types that do not match are
    /// rejected with 400 rather than coerced.
    pub fn create_collection_item_from_json(
        &self,
        collection_name: &CollectionName,
        body: &serde_json::Value,
    ) -> Result<u64, HttpError> {
        let item = self.parse_item(collection_name, body)?;
        self.create_collection_item(collection_name, &item)
    }

    /// Update an item from an **untagged** JSON body. See
    /// [`CollectionService::create_collection_item_from_json`].
    pub fn update_collection_item_from_json(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        body: &serde_json::Value,
    ) -> Result<(), HttpError> {
        let item = self.parse_item(collection_name, body)?;
        self.update_collection_item(collection_name, item_id, &item)
    }

    /// Read an untagged body into a [`CollectionItem`] using the collection schema.
    fn parse_item(
        &self,
        collection_name: &CollectionName,
        body: &serde_json::Value,
    ) -> Result<CollectionItem, HttpError> {
        let schema = self
            .collection_repository
            .get_collection_schema(collection_name)
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
            .map_err(map_internal_error)?;
        CollectionItem::from_untyped(body, &composite_schemas, &schema)
            .map_err(|e| HttpError::BadRequest(&e))
    }

    // ---- draft / published ---------------------------------------------------

    /// The item as an editor sees it: the working copy if there is one, the published
    /// copy otherwise.
    fn working_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<CollectionItem>, HttpError> {
        match self
            .collection_repository
            .get_collection_item_draft(collection_name, item_id)
            .map_err(map_internal_error)?
        {
            Some(draft) => Ok(Some(draft)),
            None => self
                .collection_repository
                .get_collection_item(collection_name, item_id)
                .map_err(map_internal_error),
        }
    }

    /// Every item as an editor sees it, ordered by id.
    ///
    /// The working copies are read in one scan and laid over the published ones, so a list
    /// of a collection costs two reads rather than one per item.
    fn working_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, CollectionItem)>, HttpError> {
        let mut drafts: HashMap<CollectionItemId, CollectionItem> = self
            .collection_repository
            .list_collection_item_drafts(collection_name)
            .map_err(map_internal_error)?
            .into_iter()
            .collect();

        let mut items = self
            .collection_repository
            .list_collection_items(collection_name)
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
    fn format_items(
        &self,
        schema: &CollectionSchema,
        items: Vec<(CollectionItemId, CollectionItem)>,
    ) -> Result<Vec<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        let composite_schema_map = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?;
        Ok(items
            .into_iter()
            .map(|(id, item)| {
                let formatted = item.format_to_schema(&composite_schema_map, schema);
                (id, formatted.to_response(self.image_repository.as_ref()))
            })
            .collect())
    }

    /// Format one stored item the way the HTTP layer reports it.
    fn format_item(
        &self,
        collection_name: &CollectionName,
        item: &CollectionItem,
    ) -> Result<CollectionItemResponse, HttpError> {
        let schema = self.get_collection_schema(collection_name)?;
        let composite_schema_map = self
            .composite_field_repository
            .list_composite_field_schemas()
            .map_err(map_internal_error)?;
        Ok(item
            .format_to_schema(&composite_schema_map, &schema)
            .to_response(self.image_repository.as_ref()))
    }

    /// Draft/published state of one item.
    ///
    /// Absent metadata means "draft": an item that was never published is not an error.
    pub fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<ItemMetadata, HttpError> {
        self.require_item(collection_name, &item_id)?;
        Ok(self
            .collection_repository
            .get_item_metadata(collection_name, &item_id)
            .map_err(map_internal_error)?
            .unwrap_or_default())
    }

    /// Metadata for every item, so the admin list can show a status for items that were
    /// never published (those have no stored record).
    pub fn list_item_metadata(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, HttpError> {
        let stored: HashMap<CollectionItemId, ItemMetadata> = self
            .collection_repository
            .list_item_metadata(collection_name)
            .map_err(map_internal_error)?
            .into_iter()
            .collect();
        let items = self
            .collection_repository
            .list_collection_items(collection_name)
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
    pub fn has_draft(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<bool, HttpError> {
        self.require_item(collection_name, &item_id)?;
        Ok(self
            .collection_repository
            .get_collection_item_draft(collection_name, &item_id)
            .map_err(map_internal_error)?
            .is_some())
    }

    /// Ids of the items with an unpublished working copy, for the admin list.
    pub fn draft_item_ids(
        &self,
        collection_name: &CollectionName,
    ) -> Result<HashSet<CollectionItemId>, HttpError> {
        Ok(self
            .collection_repository
            .list_collection_item_drafts(collection_name)
            .map_err(map_internal_error)?
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// Publish or unpublish one item, recording when it happened and who did it.
    ///
    /// `actor` is required rather than optional: an audit trail with holes in it is worse
    /// than none, and every caller in the HTTP layer already has the authenticated user.
    pub fn set_item_status(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
        status: ItemStatus,
        actor: PublishedBy,
    ) -> Result<ItemMetadata, HttpError> {
        self.require_item(collection_name, &item_id)?;
        // Built from the stored record so publishing keeps the content timestamps.
        let metadata = self
            .collection_repository
            .get_item_metadata(collection_name, &item_id)
            .map_err(map_internal_error)?
            .unwrap_or_default()
            .with_status(status, Some(actor));

        if metadata.is_published() {
            // Publishing *is* the copy: whatever the editor has been working on replaces
            // the published item and stops being a separate draft. With nothing pending,
            // publishing only refreshes the timestamp.
            if let Some(draft) = self
                .collection_repository
                .get_collection_item_draft(collection_name, &item_id)
                .map_err(map_internal_error)?
            {
                self.collection_repository
                    .update_collection_item(collection_name, &item_id, &draft)
                    .map_err(map_internal_error)?;
                self.collection_repository
                    .delete_collection_item_draft(collection_name, &item_id)
                    .map_err(map_internal_error)?;
            }
        }

        self.collection_repository
            .set_item_metadata(collection_name, &item_id, &metadata)
            .map_err(map_internal_error)?;
        // Only after the status is stored: a receiver that reacts by reading the delivery
        // API must not see the previous state.
        self.notifier
            .notify(ContentEvent::collection_item(collection_name, item_id, &metadata));
        Ok(metadata)
    }

    /// Items with the window the caller asked for. The total travels alongside, because
    /// `X-Total-Count` is what tells an admin caller there is more to fetch.
    pub fn get_collection_items_page(
        &self,
        collection_name: &CollectionName,
        pagination: &Pagination,
    ) -> Result<Page<(CollectionItemId, CollectionItemResponse)>, HttpError> {
        Ok(pagination.apply(self.get_collection_items(collection_name)?))
    }

    /// Items visible to the public delivery API, with the metadata it reports.
    ///
    /// Ordered by id, which is the order the pages have to be walked in.
    pub fn list_published_items(
        &self,
        collection_name: &CollectionName,
        pagination: &Pagination,
    ) -> Result<Page<(CollectionItemId, ItemMetadata, CollectionItemResponse)>, HttpError> {
        let metadata: HashMap<CollectionItemId, ItemMetadata> =
            self.list_item_metadata(collection_name)?.into_iter().collect();

        // The published copy, never the working one: edits must not leak to a site.
        let mut stored = self
            .collection_repository
            .list_collection_items(collection_name)
            .map_err(map_internal_error)?;
        stored.sort_by_key(|(id, _)| **id);
        let items = self.format_items(
            &self.get_collection_schema(collection_name)?,
            stored,
        )?;

        let mut published = Vec::new();
        for (id, values) in items {
            if let Some(metadata) = metadata.get(&id) {
                if metadata.is_published() {
                    published.push((id, metadata.clone(), values));
                }
            }
        }
        Ok(pagination.apply(published))
    }

    /// One item, but only if it is published.
    ///
    /// A draft answers "not found" rather than "forbidden", so the delivery API does not
    /// reveal that unpublished content exists.
    pub fn get_published_item(
        &self,
        collection_name: &CollectionName,
        item_id: CollectionItemId,
    ) -> Result<(ItemMetadata, CollectionItemResponse), HttpError> {
        let metadata = self.get_item_metadata(collection_name, item_id)?;
        if !metadata.is_published() {
            return Err(HttpError::NotFound(&format!(
                "Item with id '{}' not found in collection '{}'",
                item_id, collection_name
            )));
        }
        let item = self
            .collection_repository
            .get_collection_item(collection_name, &item_id)
            .map_err(map_internal_error)?
            .ok_or_else(|| {
                HttpError::NotFound(&format!(
                    "Item with id '{}' not found in collection '{}'",
                    item_id, collection_name
                ))
            })?;
        Ok((metadata, self.format_item(collection_name, &item)?))
    }

    /// Collections that have at least one published item.
    pub fn list_collections_with_published_items(
        &self,
    ) -> Result<Vec<CollectionName>, HttpError> {
        let mut names = Vec::new();
        for name in self.get_all_collections()? {
            if !self.published_item_ids(&name)?.is_empty() {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn published_item_ids(
        &self,
        collection_name: &CollectionName,
    ) -> Result<HashSet<CollectionItemId>, HttpError> {
        Ok(self
            .collection_repository
            .list_item_metadata(collection_name)
            .map_err(map_internal_error)?
            .into_iter()
            .filter(|(_, metadata)| metadata.is_published())
            .map(|(id, _)| id)
            .collect())
    }

    fn require_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), HttpError> {
        if self.working_item(collection_name, item_id)?.is_none() {
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
    use crate::models::field::{CompositeFieldSchema, FieldValueMap, TextFieldOptions};
    use crate::models::field::{FieldSchema, FieldType, FieldValue};
    use crate::models::schema::CompositeFieldId;
    use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};
    use crate::repositories::image_repository::ImageRepository;

    use super::*;
    use crate::webhook::NoopNotifier;

    struct MockCollectionRepository {
        schemas: Arc<RwLock<HashMap<CollectionName, CollectionSchema>>>,
        items: Arc<RwLock<HashMap<CollectionName, HashMap<CollectionItemId, CollectionItem>>>>,
        item_counter: Arc<RwLock<u64>>,
        item_metadata: Arc<RwLock<HashMap<(CollectionName, CollectionItemId), ItemMetadata>>>,
        drafts: Arc<RwLock<HashMap<(CollectionName, CollectionItemId), CollectionItem>>>,
    }
    impl CollectionRepository for MockCollectionRepository {
        fn get_collection_schema(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Option<CollectionSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(collection_name).cloned())
        }
        fn list_collection_names(
            &self,
        ) -> Result<Vec<CollectionName>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.schemas.read().unwrap().keys().cloned().collect())
        }
        fn add_collection_schema(
            &self,
            collection_name: &CollectionName,
            schema: &CollectionSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(collection_name.clone(), schema.clone());
            Ok(())
        }
        fn delete_collection(
            &self,
            collection_name: &CollectionName,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(collection_name);
            self.item_metadata.write().unwrap().retain(|(name, _), _| name != collection_name);
            Ok(())
        }
        fn list_collection_items(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Vec<(CollectionItemId, CollectionItem)>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.items.read().unwrap().get(collection_name).map_or(vec![], |items_map| {
                items_map.iter().map(|(id, item)| (id.clone(), item.clone())).collect()
            }))
        }
        fn get_collection_item(
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
        fn add_collection_item(
            &self,
            collection_name: &CollectionName,
            item_data: &CollectionItem,
        ) -> Result<u64, Box<dyn std::error::Error + Send + Sync + 'static>> {
            let mut items_map = self.items.write().unwrap();
            let collection_items = items_map.entry(collection_name.clone()).or_insert_with(HashMap::new);
            let new_id = {
                let mut counter = self.item_counter.write().unwrap();
                *counter += 1;
                *counter
            };
            collection_items.insert(CollectionItemId::from_u64(new_id), item_data.clone());
            Ok(new_id)
        }
        fn update_collection_item(
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
        fn delete_collection_item(
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
        fn get_item_metadata(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<Option<ItemMetadata>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self
                .item_metadata
                .read()
                .unwrap()
                .get(&(collection_name.clone(), item_id.clone()))
                .cloned())
        }
        fn set_item_metadata(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            metadata: &ItemMetadata,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.item_metadata
                .write()
                .unwrap()
                .insert((collection_name.clone(), item_id.clone()), metadata.clone());
            Ok(())
        }
        fn get_collection_item_draft(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
        ) -> Result<Option<CollectionItem>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self
                .drafts
                .read()
                .unwrap()
                .get(&(collection_name.clone(), item_id.clone()))
                .cloned())
        }
        fn set_collection_item_draft(
            &self,
            collection_name: &CollectionName,
            item_id: &CollectionItemId,
            item_data: &CollectionItem,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.drafts
                .write()
                .unwrap()
                .insert((collection_name.clone(), item_id.clone()), item_data.clone());
            Ok(())
        }
        fn delete_collection_item_draft(
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
        fn list_collection_item_drafts(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Vec<(CollectionItemId, CollectionItem)>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self
                .drafts
                .read()
                .unwrap()
                .iter()
                .filter(|((name, _), _)| name == collection_name)
                .map(|((_, id), item)| (id.clone(), item.clone()))
                .collect())
        }
        fn list_item_metadata(
            &self,
            collection_name: &CollectionName,
        ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
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
        fn list_composite_field_schemas(
            &self,
        ) -> Result<
            HashMap<CompositeFieldId, CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().clone())
        }
        fn get_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<
            Option<CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().get(id).cloned())
        }
        fn add_composite_field_schema(
            &self,
            id: &CompositeFieldId,
            schema: &CompositeFieldSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(id.clone(), schema.clone());
            Ok(())
        }
        fn delete_composite_field_schema(
            &self,
            id: &CompositeFieldId,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(id);
            Ok(())
        }
    }

    struct MockImageRepository {}
    impl ImageRepository for MockImageRepository {
        fn get_image(&self, id: &ImageID) -> Result<Option<Image>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(Some(Image {
                original_filename: format!("image_{}.jpg", id),
                url: format!("/images/{}", id),
                uploaded_at: chrono::Utc::now(),
            }))
        }
        fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(vec![])
        }
        fn generate_image_upload_url(&self, _upload_info: &NewImageRequest) -> Result<NewImageInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(NewImageInfo {
                id: ImageID::from_u64(1),
                upload_url: "/upload/1".to_string(),
            })
        }
        fn delete_image(&self, _id: &ImageID) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        fn take_upload_key(&self, _key: &str) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(None)
        }
        fn read_image_bytes(&self, _file_name: &str) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(None)
        }
        fn write_image_bytes(&self, _file_name: &str, _data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
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
        fn notify(&self, event: ContentEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn a_status_change_notifies_once_and_a_missing_item_notifies_nothing() {
        let notifier = Arc::new(RecordingNotifier::default());
        let mut schemas = HashMap::new();
        schemas.insert("blog".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(
            Arc::new(collection_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            notifier.clone(),
        );

        let item_id = service
            .create_collection_item(&"blog".into(), &create_test_item("Hello", 1.0))
            .unwrap();

        let metadata = service
            .set_item_status(
                &"blog".into(),
                CollectionItemId::from_u64(item_id),
                ItemStatus::Published,
                publisher(),
            )
            .unwrap();
        // Publishing leaves an audit trail: who did it, alongside when.
        assert_eq!(
            metadata.published_by.as_ref().map(|by| by.username.as_str()),
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
        assert!(service
            .set_item_status(
                &"blog".into(),
                CollectionItemId::from_u64(99),
                ItemStatus::Published,
                publisher()
            )
            .is_err());
        assert_eq!(notifier.events.lock().unwrap().len(), 1);
    }

    /// The account the tests publish as.
    fn publisher() -> PublishedBy {
        PublishedBy::from(&crate::models::user::User::new(
            "admin@example.com",
            String::new(),
            true,
            crate::models::user::Permission::admin(),
        ))
    }

    // Test helper functions
    fn create_test_service() -> CollectionService<MockCollectionRepository, MockCompositeFieldRepository, MockImageRepository> {
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier))
    }

    fn create_test_schema() -> CollectionSchema {
        vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            },
            FieldSchema {
                name: "count".to_string(),
                field_type: FieldType::Number,
                required: false,
                width: 12,
                height: 1,
            },
        ]
    }

    fn create_test_item(title: &str, count: f64) -> CollectionItem {
        FieldValueMap(HashMap::from([
            ("title".to_string(), FieldValue::Text(title.to_string())),
            ("count".to_string(), FieldValue::Number(Some(count))),
        ]), std::marker::PhantomData)
    }

    fn create_test_item_response(title: &str, count: f64) -> CollectionItemResponse {
        use crate::models::field::FieldValueResponse;
        HashMap::from([
            ("title".to_string(), FieldValueResponse::Text(title.to_string())),
            ("count".to_string(), FieldValueResponse::Number(Some(count))),
        ])
    }

    #[test]
    fn test_create_collection_service() {
        let service = create_test_service();
        assert!(service.get_all_collections().is_ok());
    }
    #[test]
    fn test_get_all_collections_empty() {
        let service = create_test_service();
        let collection_names = service.get_all_collections().unwrap();
        assert_eq!(collection_names.len(), 0);
    }

    #[test]
    fn test_get_collection_schema_not_found() {
        let service = create_test_service();
        let result = service.get_collection_schema(&"non_existent".into());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection not found"));
    }

    #[test]
    fn test_add_collection_schema_success() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];

        let result = service.add_collection_schema(&"test_collection".into(), &schema);
        assert!(result.is_ok());

        let collection_names = service.get_all_collections().unwrap();
        assert_eq!(collection_names, vec!["test_collection".into()]);

        let retrieved_schema = service.get_collection_schema(&"test_collection".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_update_collection_schema_success() {
        let service = create_test_service();
        let initial_schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        service.add_collection_schema(&"test_collection".into(), &initial_schema).unwrap();

        let updated_schema = vec![
            FieldSchema {
                name: "title2".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.update_collection_schema(&"test_collection".into(), &updated_schema);
        assert!(result.is_ok());

        let retrieved_schema = service.get_collection_schema(&"test_collection".into()).unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[test]
    fn test_add_collection_schema_already_exists() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        service.add_collection_schema(&"test_collection".into(), &schema).unwrap();

        let duplicate_schema = vec![
            FieldSchema {
                name: "other".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        let result = service.add_collection_schema(&"test_collection".into(), &duplicate_schema);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::Conflict("Collection with id 'test_collection' already exists"));

        let retrieved_schema = service.get_collection_schema(&"test_collection".into()).unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[test]
    fn test_delete_collection_success() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
            }
        ];
        service.add_collection_schema(&"test_collection".into(), &schema).unwrap();

        let result = service.delete_collection(&"test_collection".into());
        assert!(result.is_ok());

        let collection_names = service.get_all_collections().unwrap();
        assert_eq!(collection_names.len(), 0);
    }

    #[test]
    fn test_update_missing_schema() {
        let service = create_test_service();
        let update_result = service.update_collection_schema(
            &"non_existent".into(),
            &vec![
                FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                }
            ],
        );
        assert!(update_result.is_err());
    }
    #[test]
    fn test_create_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.create_collection_item(
            &"test_composite".into(),
            &create_test_item("Test Title", 42.0),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn test_create_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.create_collection_item(&"non_existent".into(), &FieldValueMap(HashMap::new(), std::marker::PhantomData));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_create_collection_item_invalid_data() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.create_collection_item(
            &"test_composite".into(),
            &FieldValueMap(HashMap::from([("count".to_string(), FieldValue::Number(Some(42.0)))]), std::marker::PhantomData),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Field 'title' is missing"));
    }

    #[test]
    fn test_get_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Sample Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), create_test_item_response("Sample Title", 10.0));
    }

    #[test]
    fn test_get_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(999));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Item with id '999' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_get_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.get_collection_item(&"non_existent".into(), CollectionItemId::from_u64(1));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_update_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Original Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_collection_item(
            &"test_composite".into(),
            CollectionItemId::from_u64(1),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_ok());

        let item = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1)).unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[test]
    fn test_update_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_collection_item(
            &"test_composite".into(),
            CollectionItemId::from_u64(999),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Item with id '999' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_update_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.update_collection_item(
            &"non_existent".into(),
            CollectionItemId::from_u64(1),
            &create_test_item("Updated Title", 100.0),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_update_collection_item_invalid_data() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Original Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_collection_item(
            &"test_composite".into(),
            CollectionItemId::from_u64(1),
            &FieldValueMap(HashMap::from([("count".to_string(), FieldValue::Number(Some(100.0)))]), std::marker::PhantomData),
        );
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::BadRequest("Field 'title' is missing"));
    }

    #[test]
    fn test_get_collection_items_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Sample Title", 10.0));
        collection_item.insert(CollectionItemId::from_u64(2), create_test_item("Another Title", 20.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(2)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.get_collection_items(&"test_composite".into());
        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.contains(&(CollectionItemId::from_u64(1), create_test_item_response("Sample Title", 10.0))));
        assert!(items.contains(&(CollectionItemId::from_u64(2), create_test_item_response("Another Title", 20.0))));
    }

    #[test]
    fn test_get_collection_items_missing_collection() {
        let service = create_test_service();
        let result = service.get_collection_items(&"non_existent".into());
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }

    #[test]
    fn test_delete_collection_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let mut collection_item = HashMap::new();
        collection_item.insert(CollectionItemId::from_u64(1), create_test_item("Sample Title", 10.0));
        let mut items = HashMap::new();
        items.insert("test_composite".into(), collection_item);
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            item_counter: Arc::new(RwLock::new(1)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.delete_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1));
        assert!(result.is_ok());

        let get_result = service.get_collection_item(&"test_composite".into(), CollectionItemId::from_u64(1));
        assert!(get_result.is_err());
        assert_eq!(get_result.err().unwrap(), HttpError::NotFound("Item with id '1' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_delete_collection_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let collection_repository = MockCollectionRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            item_counter: Arc::new(RwLock::new(0)),
            item_metadata: Arc::new(RwLock::new(HashMap::new())),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository {};
        let service = CollectionService::new(Arc::new(collection_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.delete_collection_item(&"test_composite".into(), CollectionItemId::from_u64(999));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Item with id '999' not found in collection 'test_composite'"));
    }

    #[test]
    fn test_delete_collection_item_missing_collection() {
        let service = create_test_service();
        let result = service.delete_collection_item(&"non_existent".into(), CollectionItemId::from_u64(1));
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Collection with id 'non_existent' does not exist"));
    }
}
