use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;

use crate::models::delivery::{
    DeliveredItem, DeliveryMaps, DeliveryValue, Expansion, inverse_references,
};
use crate::models::error::{HttpError, map_internal_error};
use crate::models::image::{Image, ImageId};
use crate::models::item_status::{ItemDates, ItemMetadata, ItemStatus, PublishedBy};
use crate::models::owner::ItemOwner;
use crate::models::schema::{
    CompositeFieldId, CompositeFieldSchema, RelationTarget, SchemaScope, SchemaSettings,
    referenced_relation_targets, validate_composite_references, validate_relation_targets,
    validate_schema,
};
use crate::models::single_page::{
    SinglePageItem, SinglePageItemResponse, SinglePageName, SinglePageSchema,
};
use crate::models::values::{FieldType, FieldValueResponse};
use crate::repositories::collection_repository::ApplyStatusError;
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::content_reader::{ContentReader, SchemaOwner};
use crate::repositories::image_repository::ImageRepository;
use crate::repositories::relation_repository::RelationRepository;
use crate::repositories::relation_rules;
use crate::repositories::relation_targets::RelationTargetSource;
use crate::repositories::single_page_repository::SinglePageRepository;
use crate::webhook::{ContentEvent, Notifier};

pub struct SinglePageService<
    SR: SinglePageRepository,
    CFR: CompositeFieldRepository,
    IR: ImageRepository,
> {
    single_page_repository: Arc<SR>,
    composite_field_repository: Arc<CFR>,
    image_repository: Arc<IR>,
    /// What a relation field may point at, which only the schema save asks for.
    relation_targets: Arc<dyn RelationTargetSource>,
    /// The relation index, which answers who references a page.
    relations: Arc<dyn RelationRepository>,
    /// The content those references point at, which the publish rules read.
    content: Arc<dyn ContentReader>,
    /// Told about every publish/unpublish so a site build can be triggered.
    notifier: Arc<dyn Notifier>,
}

impl<SR: SinglePageRepository, CFR: CompositeFieldRepository, IR: ImageRepository>
    SinglePageService<SR, CFR, IR>
{
    pub fn new(
        single_page_repository: Arc<SR>,
        composite_field_repository: Arc<CFR>,
        image_repository: Arc<IR>,
        relation_targets: Arc<dyn RelationTargetSource>,
        relations: Arc<dyn RelationRepository>,
        content: Arc<dyn ContentReader>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        SinglePageService {
            single_page_repository,
            composite_field_repository,
            image_repository,
            relation_targets,
            relations,
            content,
            notifier,
        }
    }
    pub async fn get_single_page_schema(
        &self,
        name: &SinglePageName,
    ) -> Result<SinglePageSchema, HttpError> {
        let single_page = self
            .single_page_repository
            .get_single_page_schema(name)
            .await
            .map_err(map_internal_error)?;
        match single_page {
            Some(schema) => Ok(schema),
            None => Err(HttpError::NotFound("Single page not found")),
        }
    }
    /// What a page is told about itself, apart from its fields; a 404 for a page that is not there,
    /// as for its schema (see `CollectionService::get_collection_settings`).
    pub async fn get_single_page_settings(
        &self,
        name: &SinglePageName,
    ) -> Result<SchemaSettings, HttpError> {
        self.get_single_page_schema(name).await?;
        self.single_page_repository
            .get_single_page_settings(name)
            .await
            .map_err(map_internal_error)
    }

    /// Replace what a page is told about itself, leaving its fields alone.
    pub async fn update_single_page_settings(
        &self,
        name: &SinglePageName,
        settings: &SchemaSettings,
    ) -> Result<(), HttpError> {
        self.get_single_page_schema(name).await?;
        self.single_page_repository
            .set_single_page_settings(name, settings)
            .await
            .map_err(map_internal_error)
    }

    pub async fn update_single_page_schema(
        &self,
        name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::SinglePage).map_err(|e| HttpError::BadRequest(&e))?;
        let composites = self.ensure_composites_exist(schema).await?;
        self.ensure_relation_targets_exist(schema, &composites)
            .await?;
        self.ensure_inverse_names_are_unique(
            &SchemaOwner::SinglePage(name.as_str().to_string()),
            schema,
        )
        .await?;
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .await
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        self.single_page_repository
            .add_single_page_schema(name, schema)
            .await
            .map_err(map_internal_error)
    }
    pub async fn list_page_names(&self) -> Result<Vec<SinglePageName>, HttpError> {
        self.single_page_repository
            .list_all_page_names()
            .await
            .map_err(map_internal_error)
    }

    /// Every composite a schema references must exist, otherwise the schema can be stored
    /// but never used to read or write values.
    /// The definitions are handed back as well, because the relation check that follows needs
    /// them: a relation declared inside one of them is a target of *this* schema.
    async fn ensure_composites_exist(
        &self,
        schema: &SinglePageSchema,
    ) -> Result<HashMap<CompositeFieldId, CompositeFieldSchema>, HttpError> {
        let composites = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?;
        let available: HashSet<CompositeFieldId> = composites.keys().cloned().collect();
        validate_composite_references(schema, &available).map_err(|e| HttpError::BadRequest(&e))?;
        Ok(composites)
    }

    /// An inverse name is how the other side addresses the relation, so one target cannot have two
    /// relations answering to the same name (see `CollectionService`).
    async fn ensure_inverse_names_are_unique(
        &self,
        owner: &SchemaOwner,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        for field in schema {
            let FieldType::Relation(options) = &field.field_type else {
                continue;
            };
            let Some(name) = options.inverse_name.as_deref() else {
                continue;
            };
            for declared in self
                .content
                .declared_inverses(&options.target)
                .await
                .map_err(map_internal_error)?
            {
                if declared.name == name && &declared.of != owner {
                    return Err(HttpError::Conflict(&format!(
                        "'{name}' is already how {} calls its relation to '{}'",
                        declared.of.describe(),
                        options.target.name()
                    )));
                }
            }
        }
        Ok(())
    }

    /// The same for relations: a target that does not exist is a field that can never hold
    /// anything, so the schema that names it is refused rather than stored.
    ///
    /// `composites` is what the schema embeds: a relation inside a definition is checked here,
    /// where the definition is used, as well as when the definition itself is saved.
    ///
    /// Most schemas name no relation at all, and answering the question costs two lists of names,
    /// so a schema without one asks for nothing.
    async fn ensure_relation_targets_exist(
        &self,
        schema: &SinglePageSchema,
        composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    ) -> Result<(), HttpError> {
        if referenced_relation_targets(schema, composites).is_empty() {
            return Ok(());
        }
        let available: Vec<RelationTarget> = self
            .relation_targets
            .relation_targets()
            .await
            .map_err(map_internal_error)?;
        validate_relation_targets(schema, composites, &available)
            .map_err(|e| HttpError::BadRequest(&e))
    }
    pub async fn add_single_page_schema(
        &self,
        name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), HttpError> {
        validate_schema(schema, SchemaScope::SinglePage).map_err(|e| HttpError::BadRequest(&e))?;
        let composites = self.ensure_composites_exist(schema).await?;
        self.ensure_relation_targets_exist(schema, &composites)
            .await?;
        self.ensure_inverse_names_are_unique(
            &SchemaOwner::SinglePage(name.as_str().to_string()),
            schema,
        )
        .await?;
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .await
            .map_err(map_internal_error)?
            .is_some()
        {
            return Err(HttpError::Conflict(&format!(
                "Single page with id '{}' already exists",
                name
            )));
        }
        self.single_page_repository
            .add_single_page_schema(name, schema)
            .await
            .map_err(map_internal_error)
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
            &crate::models::owner::ItemOwner::single_page(name.as_str()),
            &images,
        )
        .await;
    }

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

    /// The title of every page, keyed by the page's name.
    ///
    /// A page is named by its name, so a reference to one already reads; this is what a page whose
    /// schema has a title field says instead, under the same rule as a collection's items (see
    /// [`CollectionService::item_titles`](crate::services::collection_service::CollectionService::item_titles)).
    pub async fn page_titles(&self) -> Result<HashMap<String, FieldValueResponse>, HttpError> {
        let mut titles = HashMap::new();
        for name in self.list_page_names().await? {
            let Some(schema) = self
                .single_page_repository
                .get_single_page_schema(&name)
                .await
                .map_err(map_internal_error)?
            else {
                continue;
            };
            let Some(title) = schema.iter().find(|field| field.is_title) else {
                continue;
            };
            let Some(item) = self.working_page_item(&name).await? else {
                continue;
            };
            if let Some(value) = item.get(&title.name) {
                // No images to resolve: a title is one of the types that cannot be an image.
                titles.insert(name.to_string(), value.to_response(&HashMap::new()));
            }
        }
        Ok(titles)
    }

    /// The content that references this single page.
    ///
    /// A page that is not there has no references to report, so the answer is a 404 rather than an
    /// empty list: the caller asked about something that does not exist.
    pub async fn get_page_references(
        &self,
        name: &SinglePageName,
    ) -> Result<Vec<ItemOwner>, HttpError> {
        self.get_single_page_schema(name).await?;
        self.relations
            .get_relation_references(&ItemOwner::single_page(name.as_str()))
            .await
            .map_err(map_internal_error)
    }

    /// Delete a page, refusing while other content still points at it.
    ///
    /// `detach` is the caller saying "remove those references and go ahead" (`?detach=true`); see
    /// [`CollectionService::delete_collection_item`](crate::services::collection_service::CollectionService::delete_collection_item)
    /// for why all of them go first.
    pub async fn delete_single_page(
        &self,
        name: &SinglePageName,
        detach: bool,
    ) -> Result<(), HttpError> {
        if self
            .single_page_repository
            .get_single_page_schema(name)
            .await
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            )));
        }
        let owner = ItemOwner::single_page(name.as_str());
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
        self.single_page_repository
            .delete_single_page(name)
            .await
            .map_err(map_internal_error)?;
        // Nothing uses this page's images any more.
        self.set_image_references(&owner, &[]).await;
        Ok(())
    }
    /// The page's working copy, falling back to the published one.
    async fn working_page_item(
        &self,
        name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, HttpError> {
        match self
            .single_page_repository
            .get_single_page_item_draft(name)
            .await
            .map_err(map_internal_error)?
        {
            Some(draft) => Ok(Some(draft)),
            None => self
                .single_page_repository
                .get_single_page_item(name)
                .await
                .map_err(map_internal_error),
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
            .await
            .map_err(map_internal_error)?;
        match schema {
            None => Err(HttpError::NotFound(&format!(
                "Single page with id '{}' does not exist",
                name
            ))),
            Some(_) => {
                // The working copy: this is the screen an editor saves from.
                let item = self.working_page_item(name).await?;
                self.format_page_item(name, item).await
            }
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
            .await
            .map_err(map_internal_error)?;
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

                // Saved into the working copy: the published page keeps serving the live
                // site until this version is published.
                self.single_page_repository
                    .set_single_page_item_draft(name, item_data)
                    .await
                    .map_err(map_internal_error)?;
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
            .await
            .map_err(map_internal_error)
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
            .await
            .map_err(map_internal_error)?
            .ok_or_else(|| {
                HttpError::NotFound(&format!("Single page with id '{}' does not exist", name))
            })?;
        let composite_schemas = self
            .composite_field_repository
            .list_composite_field_schemas()
            .await
            .map_err(map_internal_error)?;
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
    pub async fn get_page_metadata(
        &self,
        name: &SinglePageName,
    ) -> Result<ItemMetadata, HttpError> {
        // Reuses the schema lookup so a missing page is a 404 rather than a default.
        self.get_single_page_schema(name).await?;
        Ok(self
            .single_page_repository
            .get_page_metadata(name)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default())
    }

    /// State when the page was created, changed or published (see
    /// [`CollectionService::set_collection_item_dates`](crate::services::collection_service::CollectionService::set_collection_item_dates)).
    pub async fn set_page_dates(
        &self,
        name: &SinglePageName,
        dates: &ItemDates,
    ) -> Result<ItemMetadata, HttpError> {
        let existing = self.get_page_metadata(name).await?;
        dates.check(&existing, Utc::now()).map_err(|reason| {
            HttpError::BadRequest(&format!("those dates cannot be right: {reason}"))
        })?;
        if !dates.is_empty() {
            self.single_page_repository
                .set_page_dates(name, dates)
                .await
                .map_err(map_internal_error)?;
        }
        self.get_page_metadata(name).await
    }

    /// Whether the page has an unpublished working copy.
    pub async fn page_has_draft(&self, name: &SinglePageName) -> Result<bool, HttpError> {
        self.get_single_page_schema(name).await?;
        Ok(self
            .single_page_repository
            .get_single_page_item_draft(name)
            .await
            .map_err(map_internal_error)?
            .is_some())
    }

    /// What the site serves for this page right now; a 404 while the page is not published, as for
    /// a collection item (see [`CollectionService::get_published_collection_item`]).
    pub async fn get_published_single_page_item(
        &self,
        name: &SinglePageName,
    ) -> Result<SinglePageItemResponse, HttpError> {
        if !self.get_page_metadata(name).await?.is_published() {
            return Err(HttpError::NotFound(
                "this page is not published, so the site is not serving it",
            ));
        }
        match self
            .single_page_repository
            .get_single_page_item(name)
            .await
            .map_err(map_internal_error)?
        {
            Some(item) => self.format_page_item(name, Some(item)).await,
            None => Err(HttpError::NotFound(&format!(
                "Single page with id '{}' not found",
                name
            ))),
        }
    }

    /// Throw the working copy away, leaving the page as the site serves it (see
    /// [`CollectionService::discard_collection_item_draft`]). Idempotent.
    pub async fn discard_single_page_item_draft(
        &self,
        name: &SinglePageName,
    ) -> Result<(), HttpError> {
        self.get_single_page_schema(name).await?;
        self.single_page_repository
            .delete_single_page_item_draft(name)
            .await
            .map_err(map_internal_error)
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
        let stored = self
            .single_page_repository
            .get_page_metadata(name)
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default();
        let was_published = stored.is_published();
        let mut metadata = stored.with_status(status, Some(actor));

        // Publishing is the copy, handed over in one call (see
        // `CollectionService::set_item_status`).
        let pending = if metadata.is_published() {
            self.single_page_repository
                .get_single_page_item_draft(name)
                .await
                .map_err(map_internal_error)?
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
            let composites = self
                .composite_field_repository
                .list_composite_field_schemas()
                .await
                .map_err(map_internal_error)?;
            copy.validate_to_schema(&composites, &schema)
                .map_err(|e| e.into_http_error())?;
            // Filled in is not the same as served: a required relation whose targets are all still
            // drafts would leave the site with a reference to nothing (§4).
            relation_rules::ensure_required_relations_are_published(
                &schema,
                copy,
                &composites,
                &*self.content,
            )
            .await?;
        }
        // Taking the page off the site is the same rule from the other end.
        if was_published && !metadata.is_published() {
            let composites = self
                .composite_field_repository
                .list_composite_field_schemas()
                .await
                .map_err(map_internal_error)?;
            relation_rules::ensure_unpublish_keeps_required_referrers(
                &ItemOwner::single_page(name.as_str()),
                &composites,
                &*self.content,
                &*self.relations,
            )
            .await?;
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
        expansion: &Expansion,
    ) -> Result<(ItemMetadata, DeliveredItem), HttpError> {
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
            .await
            .map_err(map_internal_error)?
            .unwrap_or_default();
        let schema = self.get_single_page_schema(name).await?;
        // The site's shape and its images, read once for the request (see
        // `CollectionService::delivery_maps`).
        let maps = DeliveryMaps {
            composites: self
                .composite_field_repository
                .list_composite_field_schemas()
                .await
                .map_err(map_internal_error)?,
            images: self
                .image_repository
                .list_images()
                .await
                .map_err(map_internal_error)?
                .into_iter()
                .collect(),
        };
        let formatted = published
            .format_to_schema(&maps.composites, &schema)
            .to_response(&maps.images);
        let forward = expansion.populate.forward_only(&schema);
        let mut values = maps
            .deliver(&schema, &formatted, &*self.content, &forward)
            .await?;
        // The other side's name for a relation is answered from the index, the way the collection
        // service does it: a name that is neither a field here nor declared by a referrer is a
        // refusal rather than an empty answer.
        let owner = ItemOwner::single_page(name.as_str());
        for asked in expansion
            .populate
            .iter()
            .filter(|asked| !forward.asks(asked))
        {
            let Some(references) = inverse_references(
                &owner,
                asked,
                expansion.inverse_limit,
                &maps,
                &*self.content,
                &*self.relations,
            )
            .await?
            else {
                return Err(HttpError::BadRequest(&format!(
                    "no relation field or inverse name '{asked}' to populate"
                ))
                .with_field(asked));
            };
            values.insert(asked.to_string(), DeliveryValue::Relation(references));
        }
        Ok((metadata, values))
    }

    /// Pages visible to the public delivery API.
    pub async fn list_published_page_names(&self) -> Result<Vec<SinglePageName>, HttpError> {
        let mut names = Vec::new();
        for name in self.list_page_names().await? {
            let metadata = self
                .single_page_repository
                .get_page_metadata(&name)
                .await
                .map_err(map_internal_error)?;
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

    use crate::models::schema::{RelationOptions};
    use crate::models::values::{TextFieldOptions};
    use crate::models::values::{
        FieldSchema, FieldType, FieldValue, FieldValueMap, FieldValueResponse,
    };
    
    use super::*;

    use crate::repositories::memory::MemorySinglePageRepository;
    use crate::repositories::memory::MemoryCompositeFieldRepository;
    use crate::repositories::memory::MemoryImageRepository;
    use crate::repositories::content_reader::NoContent;
    use crate::repositories::relation_repository::NoRelations;
    use crate::repositories::relation_targets::StaticRelationTargets;
    use crate::webhook::NoopNotifier;
    use crate::webhook::NotifyFuture;


    // Test helper functions
    fn create_test_service() -> SinglePageService<
        MemorySinglePageRepository,
        MemoryCompositeFieldRepository,
        MemoryImageRepository,
    > {
        create_test_service_with_targets(Vec::new())
    }

    /// The same, for a site that has something a relation could point at.
    fn create_test_service_with_targets(
        targets: Vec<RelationTarget>,
    ) -> SinglePageService<
        MemorySinglePageRepository,
        MemoryCompositeFieldRepository,
        MemoryImageRepository,
    > {
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MemoryImageRepository::default();
        SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::new(targets)),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        )
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
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MemoryImageRepository::default();
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            notifier.clone(),
        );

        let metadata = service
            .set_page_status(&"home".into(), ItemStatus::Published, publisher())
            .await
            .unwrap();
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
            assert_eq!(events[0].name(), "single_page.published");
            assert_eq!(events[0].page.as_ref().unwrap().as_str(), "home");
        }

        // An unknown page is a 404 and must not look like a change.
        assert!(
            service
                .set_page_status(&"missing".into(), ItemStatus::Published, publisher())
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

    fn create_test_schema() -> SinglePageSchema {
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
        FieldValueMap(
            HashMap::from([
                ("title".to_string(), FieldValue::Text(title.to_string())),
                ("count".to_string(), FieldValue::Number(Some(count))),
            ]),
            PhantomData,
        )
    }

    /// `get_single_page_item` returns the API response form, not the stored item form.
    fn create_test_item_response(title: &str, count: f64) -> SinglePageItemResponse {
        HashMap::from([
            (
                "title".to_string(),
                FieldValueResponse::Text(title.to_string()),
            ),
            ("count".to_string(), FieldValueResponse::Number(Some(count))),
        ])
    }

    #[tokio::test]
    async fn create_single_page_service() {
        let service = create_test_service();
        assert!(service.list_page_names().await.is_ok());
    }
    #[tokio::test]
    async fn get_all_pages_empty() {
        let service = create_test_service();
        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names.len(), 0);
    }

    #[tokio::test]
    async fn get_single_page_schema_not_found() {
        let service = create_test_service();
        let result = service.get_single_page_schema(&"non_existent".into()).await;
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Single page not found")
        );
    }

    #[tokio::test]
    async fn add_single_page_schema_success() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];

        let result = service
            .add_single_page_schema(&"test_page".into(), &schema)
            .await;
        assert!(result.is_ok());

        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names, vec!["test_page".into()]);
        let retrieved_schema = service
            .get_single_page_schema(&"test_page".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    /// The same rule as a collection's schema: a page may only relate to what the site has.
    #[tokio::test]
    async fn a_page_schema_may_only_relate_to_what_the_site_has() {
        let service = create_test_service_with_targets(vec![RelationTarget::Collection {
            name: "authors".to_string(),
        }]);

        let related_to = |target: RelationTarget| {
            vec![FieldSchema {
                is_title: false,
                show_in_list: false,
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
        let authors = || RelationTarget::Collection {
            name: "authors".to_string(),
        };

        service
            .add_single_page_schema(&"home".into(), &related_to(authors()))
            .await
            .unwrap();

        let missing = service
            .update_single_page_schema(
                &"home".into(),
                &related_to(RelationTarget::Collection {
                    name: "writers".to_string(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(missing.status_code, 400, "{}", missing.message);
        assert!(
            missing.message.contains("'writers' is not a collection"),
            "{}",
            missing.message
        );
    }

    #[tokio::test]
    async fn update_single_page_schema_success() {
        let service = create_test_service();
        let initial_schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_single_page_schema(&"test_page".into(), &initial_schema)
            .await
            .unwrap();

        let updated_schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title2".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        let result = service
            .update_single_page_schema(&"test_page".into(), &updated_schema)
            .await;
        assert!(result.is_ok());

        let retrieved_schema = service
            .get_single_page_schema(&"test_page".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, updated_schema);
    }

    #[tokio::test]
    async fn add_single_page_schema_already_exists() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_single_page_schema(&"test_page".into(), &schema)
            .await
            .unwrap();

        let duplicate_schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "other".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        let result = service
            .add_single_page_schema(&"test_page".into(), &duplicate_schema)
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::Conflict("Single page with id 'test_page' already exists")
        );

        let retrieved_schema = service
            .get_single_page_schema(&"test_page".into())
            .await
            .unwrap();
        assert_eq!(retrieved_schema, schema);
    }

    #[tokio::test]
    async fn delete_single_page_success() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_single_page_schema(&"test_page".into(), &schema)
            .await
            .unwrap();

        let result = service.delete_single_page(&"test_page".into(), false).await;
        assert!(result.is_ok());

        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names.len(), 0);
    }
    #[tokio::test]
    async fn delete_non_exists_single_page() {
        let service = create_test_service();
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "title".to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: true,
            width: 12,
            height: 1,
            unique: false,
        }];
        service
            .add_single_page_schema(&"test_page".into(), &schema)
            .await
            .unwrap();

        let result = service
            .delete_single_page(&"non_existent".into(), false)
            .await;
        assert!(result.is_err());

        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Single page with id 'non_existent' does not exist")
        );
        let page_names = service.list_page_names().await.unwrap();
        assert_eq!(page_names, vec!["test_page".into()]);
    }

    #[tokio::test]
    async fn update_missing_schema() {
        let service = create_test_service();
        let update_result = service
            .update_single_page_schema(
                &"non_existent".into(),
                &vec![FieldSchema {
                    is_title: false,
                    show_in_list: false,
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
    async fn create_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MemoryImageRepository::default();
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        let result = service
            .update_single_page_item(
                &"test_composite".into(),
                &create_test_item("Test Title", 42.0),
            )
            .await;
        assert!(result.is_ok());
    }

    /// The same promise as the collection editor's save: stamping a page records that it changed
    /// and writes back nothing else it read (see `CollectionRepository::touch_item_metadata`).
    #[tokio::test]
    async fn a_page_save_stamps_the_metadata_without_writing_it_back() {
        use crate::models::item_status::ItemStatus;

        let repository = Arc::new(MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(HashMap::from([(
                "home".into(),
                create_test_schema(),
            )]))),
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
            Arc::new(MemoryCompositeFieldRepository {
                schemas: Arc::new(RwLock::new(HashMap::new())),
            }),
            Arc::new(MemoryImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        service
            .update_single_page_item(&"home".into(), &create_test_item("Test Title", 42.0))
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
            .page_metadata
            .read()
            .unwrap()
            .get(&SinglePageName::from("home"))
            .cloned()
            .expect("the record the save stamped");
        assert_eq!(
            stored.status,
            ItemStatus::Published,
            "a save is not a publish"
        );
        assert!(
            stored.updated_at.is_some(),
            "and the save is still recorded"
        );
    }

    #[tokio::test]
    async fn create_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service
            .update_single_page_item(
                &"non_existent".into(),
                &FieldValueMap(HashMap::new(), PhantomData),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Single page with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn a_page_working_copy_may_be_missing_a_required_field() {
        let mut schemas = HashMap::new();
        schemas.insert("test_composite".into(), create_test_schema());
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MemoryImageRepository::default();
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        let result = service
            .update_single_page_item(
                &"test_composite".into(),
                &FieldValueMap(
                    HashMap::from([("count".to_string(), FieldValue::Number(Some(42.0)))]),
                    PhantomData,
                ),
            )
            .await;
        // A working copy may be incomplete; publishing is what asks for the required fields
        // (see the collection item editor's tests and the contract suite).
        assert!(result.is_ok(), "an incomplete draft is a draft: {result:?}");
    }

    #[tokio::test]
    async fn get_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_schema".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_schema".into(), create_test_item("Sample Title", 10.0));
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let image_repository = MemoryImageRepository::default();
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(image_repository),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        let result = service.get_single_page_item(&"test_schema".into()).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            create_test_item_response("Sample Title", 10.0)
        );
    }

    #[tokio::test]
    async fn get_single_page_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_schema".into(), create_test_schema());
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(MemoryImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );
        let result = service.get_single_page_item(&"test_schema".into()).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            HashMap::from([
                (
                    "title".to_string(),
                    FieldValueResponse::Text("".to_string())
                ),
                ("count".to_string(), FieldValueResponse::Number(None)),
            ])
        );
    }

    #[tokio::test]
    async fn get_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service.get_single_page_item(&"non_existent".into()).await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Single page with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn update_single_page_item_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Original Title", 10.0));
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(MemoryImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        let result = service
            .update_single_page_item(
                &"test_page".into(),
                &create_test_item("Updated Title", 100.0),
            )
            .await;
        assert!(result.is_ok());

        let item = service
            .get_single_page_item(&"test_page".into())
            .await
            .unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[tokio::test]
    async fn update_single_page_item_not_found() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(HashMap::new())),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(MemoryImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );
        let result = service
            .update_single_page_item(
                &"test_page".into(),
                &create_test_item("Updated Title", 100.0),
            )
            .await;
        assert!(result.is_ok());

        let item = service
            .get_single_page_item(&"test_page".into())
            .await
            .unwrap();
        assert_eq!(item, create_test_item_response("Updated Title", 100.0));
    }

    #[tokio::test]
    async fn update_single_page_item_missing_page() {
        let service = create_test_service();
        let result = service
            .update_single_page_item(
                &"non_existent".into(),
                &create_test_item("Updated Title", 100.0),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            HttpError::NotFound("Single page with id 'non_existent' does not exist")
        );
    }

    #[tokio::test]
    async fn a_page_save_may_leave_a_required_field_empty() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Original Title", 10.0));
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(MemoryImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        let result = service
            .update_single_page_item(
                &"test_page".into(),
                &FieldValueMap(
                    HashMap::from([("count".to_string(), FieldValue::Number(Some(1.0)))]),
                    PhantomData,
                ),
            )
            .await;
        // A working copy may be incomplete; publishing is what asks for the required fields
        // (see the collection item editor's tests and the contract suite).
        assert!(result.is_ok(), "an incomplete draft is a draft: {result:?}");
    }

    #[tokio::test]
    async fn get_single_page_items_success() {
        let mut schemas = HashMap::new();
        schemas.insert("test_page".into(), create_test_schema());
        let mut items = HashMap::new();
        items.insert("test_page".into(), create_test_item("Sample Title", 10.0));
        let single_page_repository = MemorySinglePageRepository {
            schemas: Arc::new(RwLock::new(schemas)),
            items: Arc::new(RwLock::new(items)),
            page_metadata: Arc::new(RwLock::new(HashMap::new())),
            metadata_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            drafts: Arc::new(RwLock::new(HashMap::new())),
        };
        let composite_field_repository = MemoryCompositeFieldRepository {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        };
        let service = SinglePageService::new(
            Arc::new(single_page_repository),
            Arc::new(composite_field_repository),
            Arc::new(MemoryImageRepository::default()),
            Arc::new(StaticRelationTargets::none()),
            Arc::new(NoRelations),
            Arc::new(NoContent),
            Arc::new(NoopNotifier),
        );

        let result = service.get_single_page_item(&"test_page".into()).await;
        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items, create_test_item_response("Sample Title", 10.0));
    }
}
