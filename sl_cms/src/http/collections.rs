use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use std::collections::HashMap;

use crate::app_module::Storage;
use crate::http::{require_admin, require_publish, AppState, AuthenticatedUser};
use crate::models::collection::{CollectionItemId, CollectionName, CollectionSchema};
use crate::models::error::HttpError;
use crate::models::item_status::{ItemMetadata, ItemStatus};
use crate::models::pagination::{PageQuery, Pagination};

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
    let metadata: HashMap<String, ItemMetadata> = module
        .collection_service
        .list_item_metadata(&name)?
        .into_iter()
        .map(|(id, metadata)| (id.to_string(), metadata))
        .collect();
    Ok(Json(metadata))
}

async fn get_collection_item_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(module.collection_service.get_item_metadata(
        &name,
        CollectionItemId::from_u64(id),
    )?))
}

async fn publish_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user)?;
    set_collection_item_status(module, collection_name, id, ItemStatus::Published).await
}

async fn unpublish_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user)?;
    set_collection_item_status(module, collection_name, id, ItemStatus::Draft).await
}

async fn set_collection_item_status<R: Storage>(
    module: AppState<R>,
    collection_name: String,
    id: u64,
    status: ItemStatus,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(
        module
            .collection_service
            .set_item_status(&name, CollectionItemId::from_u64(id), status)?,
    ))
}

async fn get_collections<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(module.collection_service.get_all_collections()?))
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
    require_publish(&user)?;
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .delete_collection_item(&name, CollectionItemId::from_u64(id))?;
    Ok(StatusCode::OK)
}
