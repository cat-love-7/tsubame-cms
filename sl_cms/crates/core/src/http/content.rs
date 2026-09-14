//! Public, read-only delivery API.
//!
//! Everything here is unauthenticated and shows **published content only**, which is what
//! a static site build needs. A draft answers 404 rather than 403, so its existence is not
//! revealed.
//!
//! Responses carry the collection schema alongside the values: values are untyped (the
//! schema is what gives them meaning), so a consumer would otherwise need a second,
//! authenticated request just to interpret them.
//!
//! Item lists are paginated. `/content/collections/{name}` returns at most
//! [`DEFAULT_PAGE_LIMIT`] items ordered by id and reports `total` and `next_offset`, so a
//! build walks the pages instead of pulling everything in one request.

use axum::extract::{Path, Query, State};
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
use crate::models::pagination::{PageQuery, Pagination};
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
    /// Published items in the collection, before this page was cut out of them.
    total: usize,
    /// Page size that was applied (`null` would mean "everything", which this route never
    /// does), and where this page starts.
    limit: Option<usize>,
    offset: usize,
    /// Pass this as `?offset=` for the next page; `null` on the last page.
    next_offset: Option<usize>,
}

#[derive(serde::Serialize)]
struct PublishedItem {
    id: CollectionItemId,
    /// When the item was first published: the date a site shows as its publication date.
    published_at: Option<DateTime<Utc>>,
    /// When this copy went live: the published copy only changes when an item is published, so
    /// this is what an incremental build should compare.
    last_published_at: Option<DateTime<Utc>>,
    values: CollectionItemResponse,
}

#[derive(serde::Serialize)]
struct SinglePageContent {
    schema: SinglePageSchema,
    published_at: Option<DateTime<Utc>>,
    last_published_at: Option<DateTime<Utc>>,
    values: SinglePageItemResponse,
}

/// Collections that currently have published content.
async fn list_collections<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .collection_service
            .list_collections_with_published_items().await?,
    ))
}

async fn get_collection<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, HttpError> {
    let pagination = Pagination::limited(query)?;
    let name = CollectionName::from(collection_name.as_str());
    let schema = module.collection_service.get_collection_schema(&name).await?;
    let page = module
        .collection_service
        .list_published_items(&name, &pagination).await?;
    let items = page
        .items
        .into_iter()
        .map(|(id, metadata, values)| PublishedItem {
            id,
            published_at: metadata.published_at,
            last_published_at: metadata.last_published_at,
            values,
        })
        .collect();

    Ok(Json(CollectionContent {
        schema,
        items,
        total: page.total,
        limit: page.limit,
        offset: page.offset,
        next_offset: page.next_offset,
    }))
}

async fn get_collection_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, id)): Path<(String, u64)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let (metadata, values) = module
        .collection_service
        .get_published_item(&name, CollectionItemId::from_u64(id)).await?;

    Ok(Json(PublishedItem {
        id: CollectionItemId::from_u64(id),
        published_at: metadata.published_at,
        last_published_at: metadata.last_published_at,
        values,
    }))
}

async fn list_single_pages<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module.single_page_service.list_published_page_names().await?,
    ))
}

async fn get_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    let schema = module.single_page_service.get_single_page_schema(&name).await?;
    let (metadata, values) = module.single_page_service.get_published_page_item(&name).await?;

    Ok(Json(SinglePageContent {
        schema,
        published_at: metadata.published_at,
        last_published_at: metadata.last_published_at,
        values,
    }))
}
