use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use std::collections::HashMap;

use crate::app_module::Storage;
use crate::http::{
    AppState, AuthenticatedUser, PreviewTokenQuery, Resource, preview_link_error, require_admin,
    require_publish,
};
use crate::models::collection::{CollectionItemId, CollectionName, CollectionSchema};
use crate::models::error::HttpError;
use crate::models::item_status::{ItemDates, ItemMetadata, ItemStatus, PublishedBy};
use crate::models::pagination::{PageQuery, Pagination};
use crate::models::sort::Sort;
use crate::preview_link::PreviewTarget;

pub fn routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/models/collections", get(get_collections::<R>))
        .route(
            "/models/collections/{collection_name}/schema",
            get(get_collection_schema::<R>)
                .post(add_collection_schema::<R>)
                .put(update_collection_schema::<R>),
        )
        .route(
            "/models/collections/{collection_name}",
            delete(delete_collection::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items",
            get(get_collection_items::<R>),
        )
        // Static segment registered alongside `{id}`; axum prefers the static one.
        .route(
            "/models/collections/{collection_name}/items/metadata",
            get(get_collection_items_metadata::<R>),
        )
        .route(
            "/models/collections/{collection_name}/item",
            post(add_collection_item::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}",
            get(get_collection_item::<R>)
                .put(update_collection_item::<R>)
                .delete(delete_collection_item::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/by/{field}/{value}",
            get(get_collection_item_by_unique_value::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/metadata",
            get(get_collection_item_metadata::<R>).put(set_collection_item_metadata::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/preview",
            get(preview_collection_item::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/preview-link",
            post(create_collection_item_preview_link::<R>),
        )
        // Static segment, so axum routes it before `{id}`.
        .route(
            "/models/collections/{collection_name}/items/status",
            post(set_items_status::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/duplicate",
            post(duplicate_collection_item::<R>),
        )
        // The names of items a reference may point at, for the screens that show a reference.
        .route(
            "/models/collections/{collection_name}/items/titles",
            get(get_collection_item_titles::<R>),
        )
        // Who points at this item, so a delete can say what it would break.
        .route(
            "/models/collections/{collection_name}/items/{id}/references",
            get(get_collection_item_references::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/publish",
            post(publish_collection_item::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/unpublish",
            post(unpublish_collection_item::<R>),
        )
}

/// Status of every item, keyed by id so a client can line it up with the item list.
async fn get_collection_items_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let with_draft = module.collection_service.draft_item_ids(&name).await?;
    let metadata: HashMap<String, ItemStatusResponse> = module
        .collection_service
        .list_item_metadata(&name)
        .await?
        .into_iter()
        .map(|(id, metadata)| {
            let status = ItemStatusResponse {
                has_draft: with_draft.contains(&id),
                metadata,
            };
            (id.to_string(), status)
        })
        .collect();
    Ok(Json(metadata))
}

async fn get_collection_item_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = CollectionItemId::from_u64(id);
    Ok(Json(ItemStatusResponse {
        metadata: module
            .collection_service
            .get_item_metadata(&name, item_id)
            .await?,
        has_draft: module.collection_service.has_draft(&name, item_id).await?,
    }))
}

/// The stored metadata plus whether an unpublished working copy exists, which is what the
/// admin screens need to tell "published" from "published, with changes waiting".
#[derive(serde::Serialize)]
struct ItemStatusResponse {
    #[serde(flatten)]
    metadata: ItemMetadata,
    has_draft: bool,
}

/// State when the content was created, changed or published, which is what migrating it from
/// another CMS needs to be able to do.
///
/// A patch: only the dates that were sent change, so this is safe to run against content that is
/// being edited or published at the same time. When the content went live is publish permission
/// rather than edit, because those are the dates a build compares to decide whether its copy of
/// the site is out of date.
async fn set_collection_item_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
    Json(dates): Json<ItemDates>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = CollectionItemId::from_u64(id);
    if dates.touches_publication() {
        require_publish(&user, Some(Resource::Collection(&name)))?;
    }
    Ok(Json(ItemStatusResponse {
        metadata: module
            .collection_service
            .set_collection_item_dates(&name, item_id, &dates)
            .await?,
        has_draft: module.collection_service.has_draft(&name, item_id).await?,
    }))
}

/// What the item would look like if it were published now: the working copy, with the
/// schema. Authenticated, because that copy is unpublished content.
/// The item holding a unique value, for a screen that wants to open whoever already has it.
///
/// The index answers, so a draft that is changing its value does not hide the item: the value
/// it is *about* to use is held too.
async fn get_collection_item_by_unique_value<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, field, value)): Path<(String, String, String)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let (id, values) = module
        .collection_service
        .get_item_by_unique_value(&name, &field, &value)
        .await?;
    Ok(Json(ItemLookup { id: *id, values }))
}

#[derive(serde::Serialize)]
struct ItemLookup {
    id: u64,
    values: crate::models::collection::CollectionItemResponse,
}

async fn preview_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = CollectionItemId::from_u64(id);
    Ok(Json(ItemPreview {
        schema: module
            .collection_service
            .get_collection_schema(&name)
            .await?,
        id,
        values: module
            .collection_service
            .get_collection_item(&name, item_id)
            .await?,
    }))
}

#[derive(serde::Serialize)]
struct ItemPreview {
    schema: CollectionSchema,
    id: u64,
    values: crate::models::collection::CollectionItemResponse,
}

/// Mint a signed, expiring link that shows this working copy to someone without an account.
///
/// Asking for one needs `can_edit` (the write rule of the middleware) and no more: an editor
/// can already read the draft, and reviewing it with a client is what the link is for.
async fn create_collection_item_preview_link<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = CollectionItemId::from_u64(id);
    // 404 for an item that is not there, rather than a link that opens nothing.
    module
        .collection_service
        .get_collection_item(&name, item_id)
        .await?;

    Ok(Json(module.preview_links.issue(
        &PreviewTarget::CollectionItem {
            collection: name.as_str().to_string(),
            item_id: id,
        },
        chrono::Utc::now(),
    )))
}

/// The routes a preview link opens. Public, because the signature is the credential - see
/// [`crate::preview_link`].
pub fn preview_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new().route(
        "/preview/collections/{collection_name}/items/{id}",
        get(open_collection_item_preview::<R>),
    )
}

async fn open_collection_item_preview<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
    Query(query): Query<PreviewTokenQuery>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let target = PreviewTarget::CollectionItem {
        collection: name.as_str().to_string(),
        item_id: id,
    };
    module
        .preview_links
        .verify(&target, &query.token, chrono::Utc::now())
        .map_err(preview_link_error)?;

    // The same body the authenticated preview returns, so a site's preview code needs one
    // parser for both.
    let item_id = CollectionItemId::from_u64(id);
    Ok(Json(ItemPreview {
        schema: module
            .collection_service
            .get_collection_schema(&name)
            .await?,
        id,
        values: module
            .collection_service
            .get_collection_item(&name, item_id)
            .await?,
    }))
}

/// The most items one batch may carry.
///
/// A batch is a convenience for a screen that is showing a page of a list, not a way to publish a
/// whole collection in one request: each item is a transaction, an index update and a webhook.
const MAX_BATCH: usize = 100;

/// What a batch asked for: which items, and which way.
#[derive(serde::Deserialize)]
struct BatchStatusRequest {
    ids: Vec<u64>,
    status: ItemStatus,
}

/// Publish or unpublish a batch, answering per item.
async fn set_items_status<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(collection_name): Path<String>,
    Json(request): Json<BatchStatusRequest>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user, Some(Resource::Collection(&collection_name)))?;
    if request.ids.is_empty() {
        return Err(HttpError::BadRequest("a batch needs at least one item"));
    }
    if request.ids.len() > MAX_BATCH {
        return Err(HttpError::BadRequest(&format!(
            "a batch carries at most {MAX_BATCH} items"
        )));
    }
    let name = CollectionName::from(collection_name.as_str());
    let ids: Vec<CollectionItemId> = request
        .ids
        .into_iter()
        .map(CollectionItemId::from_u64)
        .collect();
    let outcomes = module
        .collection_service
        .set_items_status(&name, &ids, request.status, PublishedBy::from(&user))
        .await;
    Ok(Json(outcomes))
}

/// Copy an item, answering with the new item's id so the caller can open it.
/// The middleware already refuses a write without `can_edit` on the collection, as it does for
/// creating one.
async fn duplicate_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let created = module
        .collection_service
        .duplicate_collection_item(&name, CollectionItemId::from_u64(id))
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn publish_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user, Some(Resource::Collection(&collection_name)))?;
    let actor = PublishedBy::from(&user);
    set_collection_item_status(module, collection_name, id, ItemStatus::Published, actor).await
}

async fn unpublish_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user, Some(Resource::Collection(&collection_name)))?;
    let actor = PublishedBy::from(&user);
    set_collection_item_status(module, collection_name, id, ItemStatus::Draft, actor).await
}

async fn set_collection_item_status<R: Storage>(
    module: AppState<R>,
    collection_name: String,
    id: u64,
    status: ItemStatus,
    actor: PublishedBy,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = CollectionItemId::from_u64(id);
    let metadata = module
        .collection_service
        .set_item_status(&name, item_id, status, actor)
        .await?;
    // The same shape the metadata endpoint answers with: the screens take this reply as the new
    // state, and after publishing what they need to know is whether anything is still waiting.
    Ok(Json(ItemStatusResponse {
        metadata,
        has_draft: module.collection_service.has_draft(&name, item_id).await?,
    }))
}

async fn get_collections<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
) -> Result<impl IntoResponse, HttpError> {
    // Only the collections this account may read: the sidebar should not offer a name that
    // answers 403 when it is opened. An administrator sees them all.
    let readable: Vec<CollectionName> = module
        .collection_service
        .list_collections()
        .await?
        .into_iter()
        .filter(|name| user.can_read(user.permission_for_collection(name.as_str())))
        .collect();
    Ok(Json(readable))
}

async fn get_collection_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(
        module
            .collection_service
            .get_collection_schema(&name)
            .await?,
    ))
}

async fn add_collection_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(collection_name): Path<String>,
    Json(schema): Json<CollectionSchema>,
) -> Result<impl IntoResponse, HttpError> {
    // Creating a collection changes what content can exist, which is not an editing act.
    require_admin(&user)?;
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .add_collection_schema(&name, &schema)
        .await?;
    Ok(StatusCode::OK)
}

async fn update_collection_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(collection_name): Path<String>,
    Json(schema): Json<CollectionSchema>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .update_collection_schema(&name, &schema)
        .await?;
    Ok(StatusCode::OK)
}

async fn delete_collection<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = CollectionName::from(collection_name.as_str());
    module.collection_service.delete_collection(&name).await?;
    Ok(StatusCode::OK)
}

/// What the admin item list takes: a page, and the order to read it in.
#[derive(serde::Deserialize)]
struct ItemsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    /// `?sort=` as the delivery API spells it (`models::sort`), so the two screens order the same
    /// way. The default is newest first.
    sort: Option<String>,
}

async fn get_collection_items<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Query(query): Query<ItemsQuery>,
) -> Result<impl IntoResponse, HttpError> {
    // The admin list is returned whole unless the caller asks for a page: the UI renders
    // every row, and silently truncating it would be worse than a long response.
    let pagination = Pagination::optional(PageQuery {
        limit: query.limit,
        offset: query.offset,
    })?;
    let name = CollectionName::from(collection_name.as_str());
    let schema = module
        .collection_service
        .get_collection_schema(&name)
        .await?;
    let sort = Sort::parse(query.sort.as_deref(), &schema)?;
    let page = module
        .collection_service
        .get_collection_items_page(&name, &pagination, sort.as_ref())
        .await?;

    // The body keeps the `[id, values]` array the UI already reads; the total travels in a
    // header so a paging caller knows how much is left.
    Ok((
        [("x-total-count", page.total.to_string())],
        Json(page.items),
    ))
}

async fn add_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = module
        .collection_service
        .create_collection_item_from_json(&name, &body)
        .await?;
    Ok(Json(item_id))
}

async fn get_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(
        module
            .collection_service
            .get_collection_item(&name, CollectionItemId::from_u64(id))
            .await?,
    ))
}

async fn update_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .update_collection_item_from_json(&name, CollectionItemId::from_u64(id), &body)
        .await?;
    Ok(StatusCode::OK)
}

/// `?ids=1,2,3`: the titles of the named items, which is what a reference to one shows.
///
/// Its own route rather than part of the list, because a list of items and the names of the items
/// *those* items point at are different questions - and the answers are wanted by the screens that
/// show a reference, which are not always the list.
#[derive(serde::Deserialize)]
struct TitleIdsQuery {
    #[serde(default)]
    ids: String,
}

async fn get_collection_item_titles<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Query(query): Query<TitleIdsQuery>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let ids = parse_item_ids(&query.ids)?;
    Ok(Json(
        module.collection_service.item_titles(&name, &ids).await?,
    ))
}

/// The ids a caller asked about, refusing anything that is not a list of them.
///
/// Bounded, because one request is one read per id: a page of a list asks about the items its rows
/// point at, not about a collection.
fn parse_item_ids(raw: &str) -> Result<Vec<CollectionItemId>, HttpError> {
    const MAX_IDS: usize = 200;
    let mut ids = Vec::new();
    for part in raw.split(',').filter(|part| !part.trim().is_empty()) {
        let id = part
            .trim()
            .parse::<u64>()
            .map_err(|_| HttpError::BadRequest(&format!("'{part}' is not an item id")))?;
        ids.push(CollectionItemId::from_u64(id));
    }
    if ids.len() > MAX_IDS {
        return Err(HttpError::BadRequest(&format!(
            "at most {MAX_IDS} ids can be asked about at once (got {})",
            ids.len()
        )));
    }
    Ok(ids)
}

/// The content that references this item, so a delete can say what it would break.
async fn get_collection_item_references<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(
        module
            .collection_service
            .get_item_references(&name, CollectionItemId::from_u64(id))
            .await?,
    ))
}

/// `?detach=true`: remove the references to this item and delete it anyway.
#[derive(serde::Deserialize)]
struct DetachQuery {
    #[serde(default)]
    detach: bool,
}

async fn delete_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
    Query(query): Query<DetachQuery>,
) -> Result<impl IntoResponse, HttpError> {
    // Deleting content removes it from the site just as unpublishing does.
    require_publish(&user, Some(Resource::Collection(&collection_name)))?;
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .delete_collection_item(&name, CollectionItemId::from_u64(id), query.detach)
        .await?;
    Ok(StatusCode::OK)
}
