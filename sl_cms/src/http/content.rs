//! Public, read-only delivery API.
//!
//! Everything here is unauthenticated and shows **published content only**, which is what
//! a static site build needs. A draft answers 404 rather than 403, so its existence is not
//! revealed.
//!
//! Responses carry the collection schema alongside the values: values are untyped (the
//! schema is what gives them meaning), so a consumer would otherwise need a second,
//! authenticated request just to interpret them.

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::collection::{
    CollectionItemId, CollectionItemResponse, CollectionName, CollectionSchema,
};
use crate::models::error::HttpError;
use crate::models::single_page::{SinglePageItemResponse, SinglePageName, SinglePageSchema};

pub fn routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/content/collections", get(list_collections::<R>))
        .route(
            "/content/collections/{collection_name}",
            get(get_collection::<R>),
        )
        .route(
            "/content/collections/{collection_name}/items/{id}",
            get(get_collection_item::<R>),
        )
        .route("/content/single-pages", get(list_single_pages::<R>))
        .route(
            "/content/single-pages/{page_name}",
            get(get_single_page::<R>),
        )
}

#[derive(serde::Serialize)]
struct CollectionContent {
    schema: CollectionSchema,
    items: Vec<PublishedItem>,
}

#[derive(serde::Serialize)]
struct PublishedItem {
    id: CollectionItemId,
    published_at: Option<DateTime<Utc>>,
    values: CollectionItemResponse,
}

#[derive(serde::Serialize)]
struct SinglePageContent {
    schema: SinglePageSchema,
    published_at: Option<DateTime<Utc>>,
    values: SinglePageItemResponse,
}

/// Collections that currently have published content.
async fn list_collections<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .collection_service
            .list_collections_with_published_items()?,
    ))
}

async fn get_collection<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let schema = module.collection_service.get_collection_schema(&name)?;
    let items = module
        .collection_service
        .list_published_items(&name)?
        .into_iter()
        .map(|(id, metadata, values)| PublishedItem {
            id,
            published_at: metadata.published_at,
            values,
        })
        .collect();

    Ok(Json(CollectionContent { schema, items }))
}

async fn get_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let (metadata, values) = module
        .collection_service
        .get_published_item(&name, CollectionItemId::from_u64(id))?;

    Ok(Json(PublishedItem {
        id: CollectionItemId::from_u64(id),
        published_at: metadata.published_at,
        values,
    }))
}

async fn list_single_pages<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module.single_page_service.list_published_page_names()?,
    ))
}

async fn get_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    let schema = module.single_page_service.get_single_page_schema(&name)?;
    let (metadata, values) = module.single_page_service.get_published_page_item(&name)?;

    Ok(Json(SinglePageContent {
        schema,
        published_at: metadata.published_at,
        values,
    }))
}
