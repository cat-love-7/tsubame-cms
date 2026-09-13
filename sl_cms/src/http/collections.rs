use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use std::collections::HashMap;

use crate::app_module::Storage;
use crate::http::{
    preview_link_error, require_admin, require_publish, AppState, AuthenticatedUser,
    PreviewTokenQuery, Resource,
};
use crate::models::collection::{CollectionItemId, CollectionName, CollectionSchema};
use crate::models::error::HttpError;
use crate::models::item_status::{ItemMetadata, ItemStatus, PublishedBy};
use crate::models::pagination::{PageQuery, Pagination};
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
            "/models/collections/{collection_name}/items/{id}/metadata",
            get(get_collection_item_metadata::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/preview",
            get(preview_collection_item::<R>),
        )
        .route(
            "/models/collections/{collection_name}/items/{id}/preview-link",
            post(create_collection_item_preview_link::<R>),
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
    let with_draft = module.collection_service.draft_item_ids(&name)?;
    let metadata: HashMap<String, ItemStatusResponse> = module
        .collection_service
        .list_item_metadata(&name)?
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
        metadata: module.collection_service.get_item_metadata(&name, item_id)?,
        has_draft: module.collection_service.has_draft(&name, item_id)?,
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

/// What the item would look like if it were published now: the working copy, with the
/// schema. Authenticated, because that copy is unpublished content.
async fn preview_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = CollectionItemId::from_u64(id);
    Ok(Json(ItemPreview {
        schema: module.collection_service.get_collection_schema(&name)?,
        id,
        values: module.collection_service.get_collection_item(&name, item_id)?,
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
    module.collection_service.get_collection_item(&name, item_id)?;

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
        schema: module.collection_service.get_collection_schema(&name)?,
        id,
        values: module.collection_service.get_collection_item(&name, item_id)?,
    }))
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
    Ok(Json(
        module
            .collection_service
            .set_item_status(&name, CollectionItemId::from_u64(id), status, actor)
            .await?,
    ))
}

async fn get_collections<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
) -> Result<impl IntoResponse, HttpError> {
    // Only the collections this account may read: the sidebar should not offer a name that
    // answers 403 when it is opened. An administrator sees them all.
    let readable: Vec<CollectionName> = module
        .collection_service
        .get_all_collections()?
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
        module.collection_service.get_collection_schema(&name)?,
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
        .add_collection_schema(&name, &schema)?;
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
        .update_collection_schema(&name, &schema)?;
    Ok(StatusCode::OK)
}

async fn delete_collection<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = CollectionName::from(collection_name.as_str());
    module.collection_service.delete_collection(&name)?;
    Ok(StatusCode::OK)
}

async fn get_collection_items<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, HttpError> {
    // The admin list is returned whole unless the caller asks for a page: the UI renders
    // every row, and silently truncating it would be worse than a long response.
    let pagination = Pagination::optional(query)?;
    let name = CollectionName::from(collection_name.as_str());
    let page = module
        .collection_service
        .get_collection_items_page(&name, &pagination)?;

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
        .create_collection_item_from_json(&name, &body)?;
    Ok(Json(item_id))
}

async fn get_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(module.collection_service.get_collection_item(
        &name,
        CollectionItemId::from_u64(id),
    )?))
}

async fn update_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module.collection_service.update_collection_item_from_json(
        &name,
        CollectionItemId::from_u64(id),
        &body,
    )?;
    Ok(StatusCode::OK)
}

async fn delete_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    // Deleting content removes it from the site just as unpublishing does.
    require_publish(&user, Some(Resource::Collection(&collection_name)))?;
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .delete_collection_item(&name, CollectionItemId::from_u64(id))?;
    Ok(StatusCode::OK)
}
