use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema};
use crate::models::error::HttpError;

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
    Path(collection_name): Path<String>,
    Json(schema): Json<CollectionSchema>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .add_collection_schema(&name, &schema)?;
    Ok((StatusCode::OK, "Collection schema added successfully"))
}

async fn update_collection_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Json(schema): Json<CollectionSchema>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .update_collection_schema(&name, &schema)?;
    Ok((StatusCode::OK, "Collection schema updated successfully"))
}

async fn delete_collection<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module.collection_service.delete_collection(&name)?;
    Ok((StatusCode::OK, "Collection deleted successfully"))
}

async fn get_collection_items<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    Ok(Json(
        module.collection_service.get_collection_items(&name)?,
    ))
}

async fn add_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Json(item): Json<CollectionItem>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let item_id = module.collection_service.create_collection_item(&name, &item)?;
    // Preserved from the previous implementation: the new id is returned as a JSON
    // string (e.g. `"1"`) rather than a number.
    Ok(Json(item_id.to_string()))
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
    Json(item): Json<CollectionItem>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module.collection_service.update_collection_item(
        &name,
        CollectionItemId::from_u64(id),
        &item,
    )?;
    Ok((StatusCode::OK, "Collection item updated successfully"))
}

async fn delete_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    module
        .collection_service
        .delete_collection_item(&name, CollectionItemId::from_u64(id))?;
    Ok((StatusCode::OK, "Collection item deleted successfully"))
}
