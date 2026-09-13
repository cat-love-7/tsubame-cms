use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
use crate::models::single_page::{SinglePageName, SinglePageSchema};

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
    Path(page_name): Path<String>,
    Json(schema): Json<SinglePageSchema>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .add_single_page_schema(&name, &schema)?;
    Ok((StatusCode::OK, "Single page schema added successfully"))
}

async fn update_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
    Json(schema): Json<SinglePageSchema>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .update_single_page_schema(&name, &schema)?;
    Ok((StatusCode::OK, "Single page schema updated successfully"))
}

async fn delete_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    module.single_page_service.delete_single_page(&name)?;
    Ok((StatusCode::OK, "Single page schema deleted successfully"))
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
    Ok((StatusCode::OK, "Single page item updated successfully"))
}
