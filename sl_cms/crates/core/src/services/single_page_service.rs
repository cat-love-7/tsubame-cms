use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;

use crate::models::error::{HttpError, map_internal_error};
use crate::repositories::collection_repository::ApplyStatusError;
use crate::models::image::{Image, ImageId};
use crate::models::item_status::{ItemMetadata, ItemStatus, PublishedBy};
use crate::models::schema::{
    SchemaScope,validate_composite_references, validate_schema, CompositeFieldId};
use crate::models::single_page::{SinglePageItem, SinglePageItemResponse, SinglePageName, SinglePageSchema};
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::ImageRepository;
use crate::repositories::single_page_repository::SinglePageRepository;
use crate::webhook::{ContentEvent, Notifier};

pub struct SinglePageService<SR: SinglePageRepository, CFR: CompositeFieldRepository, IR: ImageRepository> {
    single_page_repository: Arc<SR>,
    composite_field_repository: Arc<CFR>,
    image_repository: Arc<IR>,
    /// Told about every publish/unpublish so a site build can be triggered.
    notifier: Arc<dyn Notifier>,
}


impl<SR: SinglePageRepository, CFR: CompositeFieldRepository, IR: ImageRepository> SinglePageService<SR, CFR, IR> {
    pub fn new(single_page_repository: Arc<SR>, composite_field_repository: Arc<CFR>, image_repository: Arc<IR>, notifier: Arc<dyn Notifier>) -> Self {
        SinglePageService {
            single_page_repository,
            composite_field_repository,
            image_repository,
            notifier,
        }
    }
    pub async fn get_single_page_schema(
        &self,
        name: &SinglePageName,
    ) -> Result<SinglePageSchema, HttpError> {
        let single_page = self.single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?;
        match single_page {
            Some(schema) => Ok(schema),
            None => Err(HttpError::NotFound("Single page not found")),
        }
    }
    pub async fn update_single_page_schema(
        &self,
        name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::SinglePage).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema).await?;
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        self.single_page_repository
            .add_single_page_schema(name, schema)
            .await.map_err(map_internal_error)
    }
    pub async fn list_page_names(&self) -> Result<Vec<SinglePageName>, HttpError> {
        self.single_page_repository
            .list_all_page_names()
            .await.map_err(map_internal_error)
    }

    /// Every composite a schema references must exist, otherwise the schema can be stored
    /// but never used to read or write values.
    async fn ensure_composites_exist(&self, schema: &SinglePageSchema) -> Result<(), HttpError> {
        let available: HashSet<CompositeFieldId> = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await.map_err(map_internal_error)?
            .into_keys()
            .collect();
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))
    }
    pub async fn add_single_page_schema(
        &self,
        name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::SinglePage).map_err(|e| HttpError::BadRequest(&e))?;
        self.ensure_composites_exist(schema).await?;
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?
            .is_some()
        {
            return Err(HttpError::Conflict(&format!(
                "Single page with id '{}' already exists",
                name
            )));
        }
        self.single_page_repository
            .add_single_page_schema(name, schema)
            .await.map_err(map_internal_error)
    }
    /// Record which images this page uses, so a delete can say what it would break.
    ///
    /// Both copies, and a failure in the log rather than in the answer (see
    /// `CollectionService::record_image_references`).
    async fn record_image_references(&self, name: &SinglePageName) {
        let published = self
            .single_page_repository
            .get_single_page_item(name)
            .await
            .map_err(|e| tracing::warn!("could not read {name} for its image references: {e}"))
            .ok()
            .flatten();
        let working = self
            .single_page_repository
            .get_single_page_item_draft(name)
            .await
            .map_err(|e| tracing::warn!("could not read {name}'s working copy: {e}"))
            .ok()
            .flatten();
        // The published copy counts only while the page is published: a draft keeps the record it
        // was created from, and nobody serves it (the rule the unique index uses too).
        let is_published = self
            .single_page_repository
            .get_page_metadata(name)
            .await
            .map(|metadata| metadata.unwrap_or_default().is_published())
            .unwrap_or(false);
        let mut images = Vec::new();
        if let Some(effective) = working.as_ref().or(published.as_ref()) {
            images.extend(crate::models::image::referenced_images(effective));
        }
        if is_published {
            if let Some(published) = published.as_ref() {
                images.extend(crate::models::image::referenced_images(published));
            }
        }
        images.sort();
        images.dedup();
        self.set_image_references(
            &crate::models::image::ImageOwner::single_page(name.as_str()),
            &images,
        )
        .await;
    }

    async fn set_image_references(
        &self,
        owner: &crate::models::image::ImageOwner,
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

    pub async fn delete_single_page(&self, name: &SinglePageName) -> Result<(), HttpError> {
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        self.single_page_repository
            .delete_single_page(name)
            .await.map_err(map_internal_error)?;
        // Nothing uses this page's images any more.
        self.set_image_references(
            &crate::models::image::ImageOwner::single_page(name.as_str()),
            &[],
        )
        .await;
        Ok(())
    }
    /// The page's working copy, falling back to the published one.
    async fn working_page_item(&self, name: &SinglePageName) -> Result<Option<SinglePageItem>, HttpError> {
        match self
            .single_page_repository
            .get_single_page_item_draft(name)
            .await.map_err(map_internal_error)?
        {
            Some(draft) => Ok(Some(draft)),
            None => self
                .single_page_repository
                .get_single_page_item(name)
                .await.map_err(map_internal_error),
        }
    }

    /// Format a page's values the way the HTTP layer reports them.
    async fn format_page_item(
        &self,
        name: &SinglePageName,
        item: Option<SinglePageItem>,
    ) -> Result<SinglePageItemResponse, HttpError> {
        let schema = self.get_single_page_schema(name).await?;
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
            .unwrap_or_default()
            .format_to_schema(&composite_schema_map, &schema)
            .to_response(&images))
    }

    pub async fn get_single_page_item(
        &self,
        name: &SinglePageName,
    ) -> Result<SinglePageItemResponse, HttpError> {
        let schema = self
            .single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?;
        match schema {
            None => {
                Err(HttpError::NotFound(&format!(
                    "Single page with id '{}' does not exist",
                    name
                )))
            }
            Some(_) => {
                // The working copy: this is the screen an editor saves from.
                let item = self.working_page_item(name).await?;
                self.format_page_item(name, item).await
            },
        }
    }
    pub async fn update_single_page_item(
        &self,
        name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), HttpError> {
        let schema = self
            .single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?;
        match schema {
            None => {
                return Err(HttpError::NotFound(&format!(
                    "Single page with id '{}' does not exist",
                    name
                )));
            }
            Some(schema) => {
                // A working copy may be incomplete; publishing is what asks for the required
                // fields (see `FieldValueMap::validate_draft`).
                item_data.validate_draft(
                    &self
                        .composite_field_repository
                        .list_composite_field_schemas()
                        .await.map_err(map_internal_error)?,
                    &schema,
                ).map_err(|e| e.into_http_error())?;

                // Saved into the working copy: the published page keeps serving the live
                // site until this version is published.
                self.single_page_repository
                    .set_single_page_item_draft(name, item_data)
                    .await.map_err(map_internal_error)?;
                // Both copies: the published one may still be serving.
                self.record_image_references(name).await;
                self.stamp_page(name).await?;
                Ok(())
            }
        }
    }

    /// Record when the page's values were last saved.
    ///
    /// Saving must not disturb the published state: a published page stays published, and
    /// only `updated_at` moves.
    async fn stamp_page(&self, name: &SinglePageName) -> Result<(), HttpError> {
        // Only `updated_at` is this call's to say, and the repository reads and writes the record
        // as one step: reading it here would undo a publish that landed in between (see
        // `CollectionRepository::touch_item_metadata`).
        self.single_page_repository
            .touch_page_metadata(name, Utc::now())
            .await.map_err(map_internal_error)
    }

    /// Update a single page's item from an **untagged** JSON body.
    ///
    /// Values arrive without type tags; the page schema is what gives them meaning.
    /// Types that do not match are rejected with 400 rather than coerced.
    pub async fn update_single_page_item_from_json(
        &self,
        name: &SinglePageName,
        body: &serde_json::Value,
    ) -> Result<(), HttpError> {
        let item = self.parse_item(name, body).await?;
        self.update_single_page_item(name, &item).await
    }

    async fn parse_item(
        &self,
        name: &SinglePageName,
        body: &serde_json::Value,
    ) -> Result<SinglePageItem, HttpError> {
        let schema = self
            .single_page_repository
            .get_single_page_schema(name)
            .await.map_err(map_internal_error)?
            .ok_or_else(|| {
                HttpError::NotFound(&format!(
                    "Single page with id '{}' does not exist",
                    name
                ))
            })?;
        let composite_schemas = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await.map_err(map_internal_error)?;
        SinglePageItem::from_untyped(body, &composite_schemas, &schema)
            .map_err(|e| HttpError::BadRequest(&e))
    }

    // ---- draft / published ---------------------------------------------------

    /// Draft/published state of every page that can be read, keyed by name.
    ///
    /// The list screen shows a status per page, and asking per page would be one round trip each;
    /// this answers with all of them at once, like the collection list does.
    pub async fn list_page_statuses(
        &self,
        readable: &[SinglePageName],
    ) -> Result<Vec<(SinglePageName, ItemMetadata, bool)>, HttpError> {
        let mut statuses = Vec::with_capacity(readable.len());
        for name in readable {
            let metadata = self.get_page_metadata(name).await?;
            let has_draft = self.page_has_draft(name).await?;
            statuses.push((name.clone(), metadata, has_draft));
        }
        Ok(statuses)
    }

    /// Draft/published state of one page. Absent metadata means "draft".
    pub async fn get_page_metadata(&self, name: &SinglePageName) -> Result<ItemMetadata, HttpError> {
        // Reuses the schema lookup so a missing page is a 404 rather than a default.
        self.get_single_page_schema(name).await?;
        Ok(self
            .single_page_repository
            .get_page_metadata(name)
            .await.map_err(map_internal_error)?
            .unwrap_or_default())
    }

    /// Whether the page has an unpublished working copy.
    pub async fn page_has_draft(&self, name: &SinglePageName) -> Result<bool, HttpError> {
        self.get_single_page_schema(name).await?;
        Ok(self
            .single_page_repository
            .get_single_page_item_draft(name)
            .await.map_err(map_internal_error)?
            .is_some())
    }

    /// Publish or unpublish a single page, recording when it happened and who did it
    /// (see [`CollectionService::set_item_status`] for why `actor` is required).
    pub async fn set_page_status(
        &self,
        name: &SinglePageName,
        status: ItemStatus,
        actor: PublishedBy,
    ) -> Result<ItemMetadata, HttpError> {
        self.get_single_page_schema(name).await?;
        // Built from the stored record so publishing keeps the first publication date and the
        // content timestamps.
        let mut metadata = self
            .single_page_repository
            .get_page_metadata(name)
            .await.map_err(map_internal_error)?
            .unwrap_or_default()
            .with_status(status, Some(actor));

        // Publishing is the copy, handed over in one call (see
        // `CollectionService::set_item_status`).
        let pending = if metadata.is_published() {
            self.single_page_repository
                .get_single_page_item_draft(name)
                .await.map_err(map_internal_error)?
        } else {
            None
        };
        if pending.is_some() {
            // The content the site serves just changed, so the release moves `updated_at`.
            metadata = metadata.released(chrono::Utc::now());
        }
        // Publishing is where completeness is asked for: a working copy may be missing a required
        // field, and the site must not be served one (see `CollectionService::set_item_status`).
        if let Some(copy) = pending.as_ref() {
            let schema = self.get_single_page_schema(name).await?;
            copy.validate_to_schema(
                &self
                    .composite_field_repository
                    .list_composite_field_schemas()
                    .await.map_err(map_internal_error)?,
                &schema,
            )
            .map_err(|e| e.into_http_error())?;
        }
        if let Err(e) = self
            .single_page_repository
            .apply_page_status(name, pending.as_ref(), &metadata)
            .await
        {
            // The same race as a collection item's publish: the page was saved again while it was
            // being published, so the editor is the one that has to look again.
            if e.downcast_ref::<ApplyStatusError>() == Some(&ApplyStatusError::DraftChanged) {
                return Err(HttpError::Conflict(
                    "the page was saved again while it was being published",
                )
                .with_code("draft_changed"));
            }
            return Err(map_internal_error(e));
        }
        // What the page holds now: publishing promoted the working copy, unpublishing took it off
        // the site but kept it.
        self.record_image_references(name).await;
        // Only after the status is stored (see `CollectionService::set_item_status`).
        self.notifier
            .notify(ContentEvent::single_page(name, &metadata))
            .await;
        Ok(metadata)
    }

    /// A page's content, but only if it is published, with the metadata the delivery API
    /// reports.
    pub async fn get_published_page_item(
        &self,
        name: &SinglePageName,
    ) -> Result<(ItemMetadata, SinglePageItemResponse), HttpError> {
        let metadata = self.get_page_metadata(name).await?;
        if !metadata.is_published() {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        // The published copy, never the working one: edits must not leak to a site.
        let published = self
            .single_page_repository
            .get_single_page_item(name)
            .await.map_err(map_internal_error)?;
        Ok((metadata, self.format_page_item(name, published).await?))
    }

    /// Pages visible to the public delivery API.
    pub async fn list_published_page_names(&self) -> Result<Vec<SinglePageName>, HttpError> {
        let mut names = Vec::new();
        for name in self.list_page_names().await? {
            let metadata = self
                .single_page_repository
                .get_page_metadata(&name)
                .await.map_err(map_internal_error)?;
            if metadata.map(|m| m.is_published()).unwrap_or(false) {
                names.push(name);
            }
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::marker::PhantomData;
    use std::sync::{Arc, RwLock};

    use crate::models::field::{CompositeFieldSchema, TextFieldOptions};
    use crate::models::field::{FieldSchema, FieldType, FieldValue, FieldValueMap, FieldValueResponse};
    use crate::models::image::{
        Image, ImageId, NewImageInfo, NewImageRequest, ReplacementInfo,
    };
    use crate::models::schema::CompositeFieldId;
    use crate::repositories::image_repository::Replacement;

    use super::*;
    use crate::webhook::NotifyFuture;
    use crate::webhook::NoopNotifier;

    struct MockSinglePageRepository {
        schemas: Arc<RwLock<HashMap<SinglePageName, SinglePageSchema>>>,
        items: Arc<RwLock<HashMap<SinglePageName, SinglePageItem>>>,
        page_metadata: Arc<RwLock<HashMap<SinglePageName, ItemMetadata>>>,
        drafts: Arc<RwLock<HashMap<SinglePageName, SinglePageItem>>>,
        /// Every whole-record metadata write. A save must not make one: it stamps the record
        /// through `touch_page_metadata` (see the trait).
        metadata_writes: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl SinglePageRepository for MockSinglePageRepository {
        async fn get_single_page_schema(
            &self,
            name: &SinglePageName,
        ) -> Result<Option<SinglePageSchema>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok(self.schemas.read().unwrap().get(name).cloned())
        }
        async fn list_all_page_names(&self) -> Result<Vec<SinglePageName>,Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.schemas.read().unwrap().keys().cloned().collect())
        }
        async fn add_single_page_schema(
            &self,
            _single_page_name: &SinglePageName,
            _schema: &SinglePageSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(_single_page_name.clone(), _schema.clone());
            Ok(())
        }
        async fn delete_single_page(
            &self,
            _single_page_name: &SinglePageName,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().remove(_single_page_name);
            self.page_metadata.write().unwrap().remove(_single_page_name);
            Ok(())
        }
        async fn get_single_page_item(
            &self,
            single_page_name: &SinglePageName
        ) -> Result<Option<SinglePageItem>, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
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
            self.items.write().unwrap().insert(single_page_name.clone(), item_data.clone());
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
            self.drafts.write().unwrap().insert(page_name.clone(), item_data.clone());
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
            let metadata = all
                .get(page_name)
                .cloned()
                .unwrap_or_default()
                .touched(now);
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
            self.page_metadata.write().unwrap().insert(page_name.clone(), metadata.clone());
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
                self.items.write().unwrap().insert(page_name.clone(), draft.clone());
                self.drafts.write().unwrap().remove(page_name);
            }
            self.page_metadata
                .write()
                .unwrap()
                .insert(page_name.clone(), metadata.clone());
            Ok(())
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
        ) -> Result<
            Option<CompositeFieldSchema>,
            Box<dyn std::error::Error + Send + Sync + 'static>,
        > {
            Ok(self.schemas.read().unwrap().get(id).cloned())
        }
        async fn add_composite_field_schema(
            &self,
            id: &CompositeFieldId,
            schema: &CompositeFieldSchema,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            self.schemas.write().unwrap().insert(id.clone(), schema.clone());
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
        async fn get_image(&self, id: &ImageId) -> Result<Option<Image>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(Some(Image {
                original_filename: format!("image_{}.jpg", id),
                url: format!("/images/{}", id),
                uploaded_at: chrono::Utc::now(),
                deleted_at: None,
            }))
        }
        async fn list_images(&self) -> Result<Vec<(ImageId, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(vec![])
        }
        async fn generate_image_upload_url(&self, _upload_info: &NewImageRequest) -> Result<NewImageInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(NewImageInfo {
                id: ImageId::from_u64(1),
                upload_url: "/upload/1".to_string(),
                url: "/images/1".to_string(),
            })
        }
        async fn generate_replacement_upload_url(&self, _id: &ImageId, ext: &str) -> Result<ReplacementInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(ReplacementInfo {
                file_name: format!("replacement.{ext}"),
                upload_url: "/upload/replacement".to_string(),
            })
        }
        async fn image_bytes_exist(&self, _file_name: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(true)
        }
        async fn replace_image(&self, _id: &ImageId, _file_name: &str) -> Result<Replacement, Box<dyn std::error::Error + Send + Sync + 'static>> {
            // These tests never go through a replacement; answering `Applied` keeps the double
            // out of the way of the content they are about.
            Ok(Replacement::Applied)
        }
        async fn rename_image(&self, _id: &ImageId, _original_filename: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn delete_image(&self, _id: &ImageId) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn image_file_name(&self, id: &ImageId) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(self.file_names.read().unwrap().get(id).cloned())
        }

        async fn set_image_references(
            &self,
            _owner: &crate::models::image::ImageOwner,
            _images: &[ImageId],
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok(())
        }
        async fn get_image_references(
            &self,
            _id: &ImageId,
        ) -> Result<Vec<crate::models::image::ImageOwner>, Box<dyn std::error::Error + Send + Sync + 'static>> {
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

    // Test helper functions
    fn create_test_service() -> SinglePageService<MockSinglePageRepository, MockCompositeFieldRepository, MockImageRepository> {
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier))
    }

    /// Records the events a service emits (see the collection service tests).
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
    async fn a_status_change_notifies_once_and_an_unknown_page_notifies_nothing() {
        let notifier = Arc::new(RecordingNotifier::default());
        let mut schemas = HashMap::new();
        schemas.insert("home".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            notifier.clone(),
        );

        let metadata = service
            .set_page_status(&"home".into(), ItemStatus::Published, publisher())
            .await
            .unwrap();
        assert_eq!(
            metadata.published_by.as_ref().map(|by| by.username.as_str()),
            Some("admin@example.com")
        );
        {
            let events = notifier.events.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].name(), "single_page.published");
            assert_eq!(events[0].page.as_ref().unwrap().as_str(), "home");
        }

        // An unknown page is a 404 and must not look like a change.
        assert!(service
            .set_page_status(&"missing".into(), ItemStatus::Published, publisher())
            .await
            .is_err());
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

    fn create_test_schema() -> SinglePageSchema {
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

    fn create_test_item(title: &str, count: f64) -> SinglePageItem {
        FieldValueMap(HashMap::from([
            ("title".to_string(), FieldValue::Text(title.to_string())),
            ("count".to_string(), FieldValue::Number(Some(count))),
        ]), PhantomData)
    }

    /// `get_single_page_item` returns the API response form, not the stored item form.
    fn create_test_item_response(title: &str, count: f64) -> SinglePageItemResponse {
        HashMap::from([
            ("title".to_string(), FieldValueResponse::Text(title.to_string())),
            ("count".to_string(), FieldValueResponse::Number(Some(count))),
        ])
    }

    #[tokio::test]
    async fn test_create_single_page_service() {
        let service = create_test_service();
        assert!(service.list_page_names().await.is_ok());
    }
    #[tokio::test]
    async fn test_get_all_pages_empty() {
        let service = create_test_service();
        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names.len(), 0);
    }

    #[tokio::test]
    async fn test_get_single_page_schema_not_found() {
        let service = create_test_service();
        let result = service.get_single_page_schema(&"non_existent".into()).await;
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page not found"));
    }

    #[tokio::test]
    async fn test_add_single_page_schema_success() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];

        let result = service.add_single_page_schema(&"test_page".into(), &schema).await;
        assert!(result.is_ok());

        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names, vec!["test_page".into()]);
        let retrieved_schema = service.get_single_page_schema(&"test_page".into()).await.unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[tokio::test]
    async fn test_update_single_page_schema_success() {
        let service = create_test_service();
        let initial_schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];
        service.add_single_page_schema(&"test_page".into(), &initial_schema).await.unwrap();

        let updated_schema = vec![
            FieldSchema {
                name: "title2".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];
        let result = service.update_single_page_schema(&"test_page".into(), &updated_schema).await;
        assert!(result.is_ok());

        let retrieved_schema = service.get_single_page_schema(&"test_page".into()).await.unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[tokio::test]
    async fn test_add_single_page_schema_already_exists() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];
        service.add_single_page_schema(&"test_page".into(), &schema).await.unwrap();

        let duplicate_schema = vec![
            FieldSchema {
                name: "other".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];
        let result = service.add_single_page_schema(&"test_page".into(), &duplicate_schema).await;
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::Conflict("Single page with id 'test_page' already exists"));

        let retrieved_schema = service.get_single_page_schema(&"test_page".into()).await.unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[tokio::test]
    async fn test_delete_single_page_success() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];
        service.add_single_page_schema(&"test_page".into(), &schema).await.unwrap();

        let result = service.delete_single_page(&"test_page".into()).await;
        assert!(result.is_ok());

        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names.len(), 0);
    }
    #[tokio::test]
    async fn test_delete_non_exists_single_page() {
        let service = create_test_service();
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            }
        ];
        service.add_single_page_schema(&"test_page".into(), &schema).await.unwrap();

        let result = service.delete_single_page(&"non_existent".into()).await;
        assert!(result.is_err());

        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names, vec!["test_page".into()]);
    }

    #[tokio::test]
    async fn test_update_missing_schema() {
        let service = create_test_service();
        let update_result = service.update_single_page_schema(
            &"non_existent".into(),
            &vec![
                FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::Text(TextFieldOptions::default()),
                    required: true,
                    width: 12,
                    height: 1,
                    unique: false,
                }
            ],
        ).await;
        assert!(update_result.is_err());
    }
    #[tokio::test]
    async fn test_create_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_composite".into(),
            &create_test_item("Test Title", 42.0),
        ).await;
        assert!(result.is_ok());
    }

    /// The same promise as the collection editor's save: stamping a page records that it changed
    /// and writes back nothing else it read (see `CollectionRepository::touch_item_metadata`).
    #[tokio::test]
    async fn a_page_save_stamps_the_metadata_without_writing_it_back() {
        use crate::models::item_status::ItemStatus;

        let repository = Arc::new(MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(HashMap::from([("home".into(), create_test_schema())]))),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::from([(
                SinglePageName::from("home"),
                ItemMetadata {
                    status: ItemStatus::Published,
                    published_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
                    ..ItemMetadata::default()
                },
            )]))),
            drafts: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        });
        let service = SinglePageService::new(
            repository.clone(),
            Arc::new(MockCompositeFieldRepository { schemas: Arc::new(RwLock::new(HashMap::new())) }),
            Arc::new(MockImageRepository::default()),
            Arc::new(NoopNotifier),
        );

        service
            .update_single_page_item(&"home".into(), &create_test_item("Test Title", 42.0))
            .await
            .unwrap();

        assert_eq!(
            repository.metadata_writes.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "a save must not write the whole metadata record"
        );
        let stored = repository
            .page_metadata
            .read()
            .unwrap()
            .get(&SinglePageName::from("home"))
            .cloned()
            .expect("the record the save stamped");
        assert_eq!(stored.status, ItemStatus::Published, "a save is not a publish");
        assert!(stored.updated_at.is_some(), "and the save is still recorded");
    }

    #[tokio::test]
    async fn test_create_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.update_single_page_item(&"non_existent".into(), &FieldValueMap(HashMap::new(), PhantomData)).await;
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
    }

    #[tokio::test]
    async fn a_page_working_copy_may_be_missing_a_required_field() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_composite".into(),
            &FieldValueMap(HashMap::from([("count".to_string(), FieldValue::Number(Some(42.0)))]), PhantomData),
        ).await;
        // A working copy may be incomplete; publishing is what asks for the required fields
        // (see the collection item editor's tests and the contract suite).
        assert!(result.is_ok(), "an incomplete draft is a draft: {result:?}");
    }

    #[tokio::test]
    async fn test_get_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_schema".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_schema".into(), create_test_item("Sample Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MockImageRepository::default();
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(image_repository), Arc::new(NoopNotifier));

        let result = service.get_single_page_item(&"test_schema".into()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), create_test_item_response("Sample Title", 10.0));
    }

    #[tokio::test]
    async fn test_get_single_page_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_schema".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository::default()), Arc::new(NoopNotifier));
        let result = service.get_single_page_item(&"test_schema".into()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HashMap::from([
            ("title".to_string(), FieldValueResponse::Text("".to_string())),
            ("count".to_string(), FieldValueResponse::Number(None)),
        ]));
    }

    #[tokio::test]
    async fn test_get_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.get_single_page_item(&"non_existent".into()).await;
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
    }

    #[tokio::test]
    async fn test_update_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Original Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository::default()), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_page".into(),
            &create_test_item("Updated Title", 100.0),
        ).await;
        assert!(result.is_ok());

        let item = service.get_single_page_item(&"test_page".into()).await.unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[tokio::test]
    async fn test_update_single_page_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository::default()), Arc::new(NoopNotifier));
        let result = service.update_single_page_item(
            &"test_page".into(),
            &create_test_item("Updated Title", 100.0),
        ).await;
        assert!(result.is_ok());

        let item = service.get_single_page_item(&"test_page".into()).await.unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[tokio::test]
    async fn test_update_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.update_single_page_item(
            &"non_existent".into(),
            &create_test_item("Updated Title", 100.0),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), HttpError::NotFound("Single page with id 'non_existent' does not exist"));
    }

    #[tokio::test]
    async fn a_page_save_may_leave_a_required_field_empty() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Original Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository::default()), Arc::new(NoopNotifier));

        let result = service.update_single_page_item(
            &"test_page".into(),
            &FieldValueMap(HashMap::from([
            ("count".to_string(), FieldValue::Number(Some(1.0))),
        ]), PhantomData),
        ).await;
        // A working copy may be incomplete; publishing is what asks for the required fields
        // (see the collection item editor's tests and the contract suite).
        assert!(result.is_ok(), "an incomplete draft is a draft: {result:?}");
    }

    #[tokio::test]
    async fn test_get_single_page_items_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Sample Title", 10.0));
        let single_page_repository = MockSinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MockCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(Arc::new(single_page_repository), Arc::new(composite_field_repository), Arc::new(MockImageRepository::default()), Arc::new(NoopNotifier));

        let result = service.get_single_page_item(&"test_page".into()).await;
        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items, create_test_item_response("Sample Title", 10.0));
    }
}
