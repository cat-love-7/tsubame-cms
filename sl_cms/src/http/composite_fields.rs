use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
use crate::models::field::CompositeFieldSchema;
use crate::models::schema::CompositeFieldId;

pub fn routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route(
            "/models/composite_fields",
            get(list_composite_field_schemas::<R>),
        )
        .route(
            "/models/composite_fields/{id}",
            get(get_composite_field_schema::<R>)
                .post(add_composite_field_schema::<R>)
                .put(update_composite_field_schema::<R>)
                .delete(delete_composite_field_schema::<R>),
        )
}

async fn list_composite_field_schemas<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module.composite_field_service.list_composite_field_schemas()?,
    ))
}

async fn get_composite_field_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let id = CompositeFieldId::from(id.as_str());
    Ok(Json(
        module.composite_field_service.get_composite_field_schema(&id)?,
    ))
}

async fn add_composite_field_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<String>,
    Json(schema): Json<CompositeFieldSchema>,
) -> Result<impl IntoResponse, HttpError> {
    let id = CompositeFieldId::from(id.as_str());
    module
        .composite_field_service
        .add_composite_field_schema(&id, &schema)?;
    Ok(StatusCode::OK)
}

async fn update_composite_field_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<String>,
    Json(schema): Json<CompositeFieldSchema>,
) -> Result<impl IntoResponse, HttpError> {
    let id = CompositeFieldId::from(id.as_str());
    module
        .composite_field_service
        .update_composite_field_schema(&id, &schema)?;
    Ok(StatusCode::OK)
}

async fn delete_composite_field_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let id = CompositeFieldId::from(id.as_str());
    module
        .composite_field_service
        .delete_composite_field_schema(&id)?;
    Ok(StatusCode::OK)
}
