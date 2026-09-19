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
        .route(
            "/content/collections/{collection_name}/items/by/{field}/{value}",
            get(get_collection_item_by_unique_value::<R>),
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

/// How many bytes a value takes as JSON, or zero when it cannot be measured.
///
/// Measuring by serialising is what makes the budget about the *response*: a hundred short
/// strings and one long article are the same count of items and very different answers.
fn json_bytes<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_string(value)
        .map(|json| json.len())
        .unwrap_or(0)
}

fn schema_bytes(schema: &CollectionSchema) -> usize {
    json_bytes(schema)
}

/// Keep the items that fit in `max_bytes`, per the budget `used` already spends.
///
/// The first item is kept even when it alone is over the budget: a page that returned nothing
/// would leave a client asking for the same offset forever. Everything that did not fit belongs
/// to the next page - the caller moves `next_offset` to it - so a client that walks the pages
/// sees every item exactly once.
fn fitting<T: serde::Serialize>(items: Vec<T>, used: usize, max_bytes: usize) -> Vec<T> {
    let mut kept = Vec::new();
    let mut spent = used;
    for item in items {
        let size = json_bytes(&item);
        if !kept.is_empty() && spent + size > max_bytes {
            break;
        }
        spent += size;
        kept.push(item);
    }
    kept
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

/// The published item whose published copy holds a unique value: what a site resolves a URL
/// against.
///
/// The published copy decides, not the index: a value a draft is about to use answers "not
/// found" until the change is released, and the value still being served keeps resolving.
async fn get_collection_item_by_unique_value<R: Storage>(
    State(module): State<AppState<R>>,
    Path((collection_name, field, value)): Path<(String, String, String)>,
) -> Result<impl IntoResponse, HttpError> {
    let name = CollectionName::from(collection_name.as_str());
    let (id, metadata, values) = module
        .collection_service
        .get_published_item_by_unique_value(&name, &field, &value)
        .await?;
    Ok(Json(PublishedItem {
        id,
        published_at: metadata.published_at,
        last_published_at: metadata.last_published_at,
        values,
    }))
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
            .list_collections_with_published_items()
            .await?,
    ))
}

async fn get_collection<R: Storage>(
    State(module): State<AppState<R>>,
    Path(collection_name): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, HttpError> {
    let pagination = Pagination::limited(query)?;
    let name = CollectionName::from(collection_name.as_str());
    let schema = module
        .collection_service
        .get_collection_schema(&name)
        .await?;
    let page = module
        .collection_service
        .list_published_items(&name, &pagination)
        .await?;
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

    // A page is bounded by bytes as well as by count. The platform this runs on answers an
    // invocation with at most 6MB, so fifty items of a few hundred kilobytes each would not
    // arrive at all - the function would fail instead of the client learning anything. The
    // schema counts against the budget too: it travels with every page.
    let items = fitting(
        items,
        schema_bytes(&schema),
        module.limits.max_response_bytes,
    );
    // `wrap` recomputes `next_offset` from what is left, which is what makes the cut invisible
    // to a client that walks the pages: nothing is skipped, and the last page still says so.
    let page = pagination.wrap(items, page.total);

    Ok(Json(CollectionContent {
        schema,
        items: page.items,
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
        .get_published_item(&name, CollectionItemId::from_u64(id))
        .await?;

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
        module
            .single_page_service
            .list_published_page_names()
            .await?,
    ))
}

async fn get_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    let schema = module
        .single_page_service
        .get_single_page_schema(&name)
        .await?;
    let (metadata, values) = module
        .single_page_service
        .get_published_page_item(&name)
        .await?;

    Ok(Json(SinglePageContent {
        schema,
        published_at: metadata.published_at,
        last_published_at: metadata.last_published_at,
        values,
    }))
}
