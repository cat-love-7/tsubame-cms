//! In-memory repositories, for the service tests.
//!
//! One implementation of each storage trait, shared by every test module that needs one: the
//! alternative was a hand-written double per service module - the same mechanics written out
//! again, and again whenever a trait grew a method (a storage method costs eight implementations
//! across the workspace, and two of them were these).
//!
//! What the tests vary is *behaviour*, not storage: a store that refuses to write a schema, a
//! promotion that answers "the working copy moved", a count of whole-record metadata writes. Those
//! are the switches on the structs below.
//!
//! Test-only: nothing here is compiled into a deployment.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::models::collection::{
    CollectionItem, CollectionItemId, CollectionName, CollectionSchema,
};
use crate::models::image::{Image, ImageId, NewImageInfo, NewImageRequest, ReplacementInfo};
use crate::models::item_status::{ItemDates, ItemMetadata};
use crate::models::schema::{CompositeFieldId, CompositeFieldSchema, SchemaSettings};
use crate::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};
use crate::repositories::collection_repository::{
    ApplyStatusError, CollectionRepository, Reservation, UniqueValue,
};
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::{ImageRepository, Replacement};
use crate::repositories::single_page_repository::SinglePageRepository;

/// One collection's items, by id: the inner half of the item store.
pub type ItemsByCollection = HashMap<CollectionName, HashMap<CollectionItemId, CollectionItem>>;

/// The unique-value index: which item holds (collection, field, value).
pub type UniqueIndex = HashMap<(CollectionName, String, String), CollectionItemId>;

pub struct MemoryCollectionRepository {
    pub schemas: Arc<RwLock<HashMap<CollectionName, CollectionSchema>>>,
    pub items: Arc<RwLock<ItemsByCollection>>,
    pub item_counter: Arc<RwLock<u64>>,
    pub item_metadata: Arc<RwLock<HashMap<(CollectionName, CollectionItemId), ItemMetadata>>>,
    pub drafts: Arc<RwLock<HashMap<(CollectionName, CollectionItemId), CollectionItem>>>,
    /// The unique index: (collection, field, value) to the item that holds it.
    pub unique: Arc<RwLock<UniqueIndex>>,
    /// A store that refuses to write a schema, for the rollback a failed save has to do.
    pub fail_schema_save: bool,
    /// A store that answers "the working copy moved" to a promotion, for the refusal the
    /// service has to turn into a 409.
    pub fail_apply_status: bool,
    /// Fail the reservation whose number this is (1 for the first), for the half-claim a
    /// storage failure leaves behind.
    pub fail_reserve_on_call: Option<usize>,
    /// How many reservations have been asked for, so the above can count.
    pub reserve_calls: std::sync::atomic::AtomicUsize,
    /// Every whole-record metadata write. A save must not make one: it stamps the record
    /// through `touch_item_metadata`, which reads and writes as one step (see the trait).
    pub metadata_writes: std::sync::atomic::AtomicUsize,
}
impl CollectionRepository for MemoryCollectionRepository {
    async fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Option<CollectionSchema>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.schemas.read().unwrap().get(collection_name).cloned())
    }
    async fn list_collection_names(
        &self,
    ) -> Result<Vec<CollectionName>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.schemas.read().unwrap().keys().cloned().collect())
    }
    async fn get_collection_settings(
        &self,
        _collection_name: &CollectionName,
    ) -> Result<SchemaSettings, Box<dyn std::error::Error + Send + Sync + 'static>> {
        // Settings are storage's business, and nothing this service does with them is
        // decided here: the adapters' own suite is where both answers are exercised.
        Ok(SchemaSettings::default())
    }
    async fn set_collection_settings(
        &self,
        _collection_name: &CollectionName,
        _settings: &SchemaSettings,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(())
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
                    .map(|(id, item)| (*id, item.clone()))
                    .collect()
            }))
    }
    async fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<CollectionItem>, Box<dyn std::error::Error + Send + Sync + 'static>> {
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
        let collection_items = items_map.entry(collection_name.clone()).or_default();
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
            items_map.insert(*item_id, item_data.clone());
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
            .remove(&(collection_name.clone(), *item_id));
        Ok(())
    }
    async fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<ItemMetadata>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self
            .item_metadata
            .read()
            .unwrap()
            .get(&(collection_name.clone(), *item_id))
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
            .get(&(collection_name.clone(), *item_id))
            .cloned()
            .unwrap_or_default()
            .touched(now);
        all.insert((collection_name.clone(), *item_id), metadata);
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
            .get(&(collection_name.clone(), *item_id))
            .cloned()
            .unwrap_or_default()
            .with_dates(dates);
        all.insert((collection_name.clone(), *item_id), metadata);
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
            .insert((collection_name.clone(), *item_id), metadata.clone());
        Ok(())
    }
    async fn get_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<CollectionItem>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self
            .drafts
            .read()
            .unwrap()
            .get(&(collection_name.clone(), *item_id))
            .cloned())
    }
    async fn set_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &CollectionItem,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.drafts
            .write()
            .unwrap()
            .insert((collection_name.clone(), *item_id), item_data.clone());
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
            .remove(&(collection_name.clone(), *item_id));
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
            .map(|((_, id), item)| (*id, item.clone()))
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
                .insert(*item_id, draft.clone());
            self.drafts
                .write()
                .unwrap()
                .remove(&(collection_name.clone(), *item_id));
        }
        self.item_metadata
            .write()
            .unwrap()
            .insert((collection_name.clone(), *item_id), metadata.clone());
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
            Some(owner) if owner != item_id => Ok(Reservation::Taken { owner: *owner }),
            Some(_) => Ok(Reservation::AlreadyHeld),
            None => {
                index.insert(key, *item_id);
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
                    *owner,
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
    ) -> Result<Option<CollectionItemId>, Box<dyn std::error::Error + Send + Sync + 'static>> {
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
            .map(|((_, id), metadata)| (*id, metadata.clone()))
            .collect())
    }
}

pub struct MemorySinglePageRepository {
    pub schemas: Arc<RwLock<HashMap<SinglePageName, SinglePageSchema>>>,
    pub items: Arc<RwLock<HashMap<SinglePageName, SinglePageItem>>>,
    pub page_metadata: Arc<RwLock<HashMap<SinglePageName, ItemMetadata>>>,
    pub drafts: Arc<RwLock<HashMap<SinglePageName, SinglePageItem>>>,
    /// Every whole-record metadata write. A save must not make one: it stamps the record
    /// through `touch_page_metadata` (see the trait).
    pub metadata_writes: Arc<std::sync::atomic::AtomicUsize>,
}
impl SinglePageRepository for MemorySinglePageRepository {
    async fn get_single_page_schema(
        &self,
        name: &SinglePageName,
    ) -> Result<Option<SinglePageSchema>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.schemas.read().unwrap().get(name).cloned())
    }
    async fn list_all_page_names(
        &self,
    ) -> Result<Vec<SinglePageName>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.schemas.read().unwrap().keys().cloned().collect())
    }
    async fn get_single_page_settings(
        &self,
        _page_name: &SinglePageName,
    ) -> Result<SchemaSettings, Box<dyn std::error::Error + Send + Sync + 'static>> {
        // As in the collection mock: the answer is storage's, and the adapters' suite covers it.
        Ok(SchemaSettings::default())
    }
    async fn set_single_page_settings(
        &self,
        _page_name: &SinglePageName,
        _settings: &SchemaSettings,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(())
    }
    async fn add_single_page_schema(
        &self,
        _single_page_name: &SinglePageName,
        _schema: &SinglePageSchema,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.schemas
            .write()
            .unwrap()
            .insert(_single_page_name.clone(), _schema.clone());
        Ok(())
    }
    async fn delete_single_page(
        &self,
        _single_page_name: &SinglePageName,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.schemas.write().unwrap().remove(_single_page_name);
        self.page_metadata
            .write()
            .unwrap()
            .remove(_single_page_name);
        Ok(())
    }
    async fn get_single_page_item(
        &self,
        single_page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        if let Some(item) = self.items.read().unwrap().get(single_page_name) {
            Ok(Some(item.clone()))
        } else {
            Ok(None)
        }
    }
    async fn update_single_page_item(
        &self,
        single_page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.items
            .write()
            .unwrap()
            .insert(single_page_name.clone(), item_data.clone());
        Ok(())
    }
    async fn get_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.drafts.read().unwrap().get(page_name).cloned())
    }
    async fn set_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.drafts
            .write()
            .unwrap()
            .insert(page_name.clone(), item_data.clone());
        Ok(())
    }
    async fn delete_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.drafts.write().unwrap().remove(page_name);
        Ok(())
    }
    async fn get_page_metadata(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<ItemMetadata>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.page_metadata.read().unwrap().get(page_name).cloned())
    }
    async fn touch_page_metadata(
        &self,
        page_name: &SinglePageName,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        // One lock for the read and the write, as the adapters promise (see
        // `CollectionRepository::touch_item_metadata`).
        let mut all = self.page_metadata.write().unwrap();
        let metadata = all.get(page_name).cloned().unwrap_or_default().touched(now);
        all.insert(page_name.clone(), metadata);
        Ok(())
    }
    async fn set_page_dates(
        &self,
        page_name: &SinglePageName,
        dates: &ItemDates,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        // A patch applied under the one lock, like the adapters.
        let mut all = self.page_metadata.write().unwrap();
        let metadata = all
            .get(page_name)
            .cloned()
            .unwrap_or_default()
            .with_dates(dates);
        all.insert(page_name.clone(), metadata);
        Ok(())
    }
    async fn set_page_metadata(
        &self,
        page_name: &SinglePageName,
        metadata: &ItemMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.metadata_writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.page_metadata
            .write()
            .unwrap()
            .insert(page_name.clone(), metadata.clone());
        Ok(())
    }
    async fn apply_page_status(
        &self,
        page_name: &SinglePageName,
        draft: Option<&SinglePageItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        // Nothing here can fail half-way, so the steps the real adapters combine are done
        // in a row (see `CollectionRepository::apply_item_status`).
        if let Some(draft) = draft {
            self.items
                .write()
                .unwrap()
                .insert(page_name.clone(), draft.clone());
            self.drafts.write().unwrap().remove(page_name);
        }
        self.page_metadata
            .write()
            .unwrap()
            .insert(page_name.clone(), metadata.clone());
        Ok(())
    }
}

pub struct MemoryCompositeFieldRepository {
    pub schemas: Arc<RwLock<HashMap<CompositeFieldId, CompositeFieldSchema>>>,
}
impl CompositeFieldRepository for MemoryCompositeFieldRepository {
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
pub struct MemoryImageRepository {
    pub file_names: std::sync::RwLock<std::collections::HashMap<ImageId, String>>,
}
impl ImageRepository for MemoryImageRepository {
    async fn get_image(
        &self,
        id: &ImageId,
    ) -> Result<Option<Image>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(Some(Image {
            original_filename: format!("image_{}.jpg", id),
            url: format!("/images/{}", id),
            thumbnail_url: None,
            uploaded_at: chrono::Utc::now(),
            deleted_at: None,
        }))
    }
    async fn list_images(
        &self,
    ) -> Result<Vec<(ImageId, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>> {
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
    async fn set_image_thumbnail(
        &self,
        _id: &ImageId,
        _ext: &str,
        _data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        // The bytes are storage's business, and nothing this service decides depends on them.
        Ok(())
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
