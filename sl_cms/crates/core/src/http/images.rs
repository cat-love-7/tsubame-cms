use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
use crate::models::image::{is_safe_image_ext, ImageID, NewImageRequest, RenameImageRequest};

/// Uploading requires authentication (enforced by the auth middleware).
pub fn protected_routes<R: Storage>() -> Router<AppState<R>> {
    // The library itself. `get_upload_url` below is a static segment, which axum prefers over
    // `{id}`, so both can live here.
    Router::new()
        .route("/models/images", get(list_images::<R>))
        .route(
            "/models/images/{id}",
            // The image's metadata can be changed (its display name); its bytes are replaced
            // through `replace_url` below, which hands out an upload URL for the same id.
            put(rename_image::<R>).delete(delete_image::<R>),
        )
        .route(
            "/models/images/get_upload_url",
            post(generate_image_upload_url::<R>),
        )
}

/// Every uploaded image, newest first.
async fn list_images<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(module.image_service.list_images().await?))
}

/// Rename an image: the label the library shows, not the stored file.
///
/// Answers with an empty body like the other mutations.
async fn rename_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
    Json(request): Json<RenameImageRequest>,
) -> Result<impl IntoResponse, HttpError> {
    module
        .image_service
        .rename_image(ImageID::from_u64(id), &request.original_filename)
        .await?;
    Ok(StatusCode::OK)
}

/// Delete an image, bytes included. Answers with an empty body like the other mutations.
async fn delete_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    module.image_service.delete_image(ImageID::from_u64(id)).await?;
    Ok(StatusCode::OK)
}

/// Ask for a place to upload an image to.
///
/// On-premises this returns a local PUT URL plus a one-shot capability token; on AWS the
/// same contract is fulfilled by an S3 presigned URL.
async fn generate_image_upload_url<R: Storage>(
    State(module): State<AppState<R>>,
    Json(request): Json<NewImageRequest>,
) -> Result<impl IntoResponse, HttpError> {
    if !is_safe_image_ext(&request.ext) {
        return Err(HttpError::BadRequest("Invalid image extension"));
    }
    Ok(Json(
        module.image_service.generate_image_upload_url(request).await?,
    ))
}
