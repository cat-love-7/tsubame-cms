use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::{require_admin, require_publish, AppState, AuthenticatedUser};
use crate::models::error::HttpError;
use crate::models::item_status::{ItemMetadata, ItemStatus, PublishedBy};
use crate::models::single_page::{
    SinglePageItemResponse, SinglePageName, SinglePageSchema,
};

pub fn routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/models/single_pages", get(get_single_pages::<R>))
        .route(
            "/models/single_pages/{page_name}/schema",
            get(get_single_page_schema::<R>)
                .post(add_single_page_schema::<R>)
                .put(update_single_page_schema::<R>),
        )
        .route(
            "/models/single_pages/{page_name}",
            delete(delete_single_page::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/item",
            get(get_single_page_item::<R>).put(update_single_page_item::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/item/metadata",
            get(get_single_page_item_metadata::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/preview",
            get(preview_single_page::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/publish",
            post(publish_single_page::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/unpublish",
            post(unpublish_single_page::<R>),
        )
}

async fn get_single_page_item_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(PageStatusResponse {
        metadata: module.single_page_service.get_page_metadata(&name)?,
        has_draft: module.single_page_service.page_has_draft(&name)?,
    }))
}

/// The stored metadata plus whether an unpublished working copy exists (see the collection
/// equivalent).
#[derive(serde::Serialize)]
struct PageStatusResponse {
    #[serde(flatten)]
    metadata: ItemMetadata,
    has_draft: bool,
}

/// What the page would look like if it were published now: the working copy, with the
/// schema. Authenticated, because that copy is unpublished content.
async fn preview_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(SinglePagePreview {
        schema: module.single_page_service.get_single_page_schema(&name)?,
        values: module.single_page_service.get_single_page_item(&name)?,
    }))
}

#[derive(serde::Serialize)]
struct SinglePagePreview {
    schema: SinglePageSchema,
    values: SinglePageItemResponse,
}

async fn publish_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user)?;
    let actor = PublishedBy::from(&user);
    set_single_page_status(module, page_name, ItemStatus::Published, actor).await
}

async fn unpublish_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user)?;
    let actor = PublishedBy::from(&user);
    set_single_page_status(module, page_name, ItemStatus::Draft, actor).await
}

async fn set_single_page_status<R: Storage>(
    module: AppState<R>,
    page_name: String,
    status: ItemStatus,
    actor: PublishedBy,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(module.single_page_service.set_page_status(
        &name, status, actor,
    )?))
}

async fn get_single_pages<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(module.single_page_service.get_all_page_names()?))
}

async fn get_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module.single_page_service.get_single_page_schema(&name)?,
    ))
}

async fn add_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Json(schema): Json<SinglePageSchema>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .add_single_page_schema(&name, &schema)?;
    Ok(StatusCode::OK)
}

async fn update_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Json(schema): Json<SinglePageSchema>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .update_single_page_schema(&name, &schema)?;
    Ok(StatusCode::OK)
}

async fn delete_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    // The page and its schema go, so this is a structural change.
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module.single_page_service.delete_single_page(&name)?;
    Ok(StatusCode::OK)
}

async fn get_single_page_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module.single_page_service.get_single_page_item(&name)?,
    ))
}

async fn update_single_page_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .update_single_page_item_from_json(&name, &body)?;
    Ok(StatusCode::OK)
}
