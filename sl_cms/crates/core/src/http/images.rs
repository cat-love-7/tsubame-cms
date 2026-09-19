use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
use crate::models::image::{
    ImageId, NewImageRequest, ReplaceImageRequest, UpdateImageRequest, is_safe_image_ext,
};

/// Uploading requires authentication (enforced by the auth middleware).
pub fn protected_routes<R: Storage>() -> Router<AppState<R>> {
    // The library itself. `get_upload_url` below is a static segment, which axum prefers over
    // `{id}`, so both can live here.
    Router::new()
        .route("/models/images", get(list_images::<R>))
        .route("/models/images/trash", get(list_trash::<R>))
        .route(
            "/models/images/{id}",
            // The image's record can be changed: its display name, and which uploaded bytes it
            // serves once a replacement upload has landed. `DELETE` is the permanent one: the
            // trash has its own route, so neither has to guess what the other meant.
            put(update_image::<R>).delete(delete_image::<R>),
        )
        .route(
            "/models/images/{id}/replace",
            post(request_replacement::<R>),
        )
        // Trashing and restoring are the two halves of one act, so they are both here rather than
        // one being a `DELETE` that does not delete.
        .route("/models/images/{id}/references", get(image_references::<R>))
        .route("/models/images/{id}/trash", post(trash_image::<R>))
        .route("/models/images/{id}/restore", post(restore_image::<R>))
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

/// Change an image's record: the name it is shown under, the bytes it serves, or when it arrived.
///
/// Answers with an empty body like the other mutations.
async fn update_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
    Json(request): Json<UpdateImageRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let image_id = ImageId::from_u64(id);
    if let Some(name) = request.original_filename.as_deref() {
        module.image_service.rename_image(image_id, name).await?;
    }
    if let Some(file_name) = request.file_name.as_deref() {
        module
            .image_service
            .replace_image(image_id, file_name)
            .await?;
    }
    if let Some(uploaded_at) = request.uploaded_at {
        module
            .image_service
            .set_image_uploaded_at(image_id, uploaded_at)
            .await?;
    }
    Ok(StatusCode::OK)
}

/// Ask for a place to upload bytes that will replace what an image shows.
///
/// The image keeps its id and its name; only what it shows changes. The record is untouched until
/// the bytes are in place (see [`update_image`]), so an upload that fails changes nothing.
async fn request_replacement<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
    Json(request): Json<ReplaceImageRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .image_service
            .request_replacement(ImageId::from_u64(id), &request)
            .await?,
    ))
}

/// The trash: images taken out of the library, most recently trashed first.
async fn list_trash<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(module.image_service.list_trash().await?))
}

/// The content that uses an image, so a delete can say what it would break.
async fn image_references<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .image_service
            .references(ImageId::from_u64(id))
            .await?,
    ))
}

/// Move an image to the trash, where content that uses it still resolves.
async fn trash_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    module
        .image_service
        .trash_image(ImageId::from_u64(id))
        .await?;
    Ok(StatusCode::OK)
}

/// Put a trashed image back in the library.
async fn restore_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    module
        .image_service
        .restore_image(ImageId::from_u64(id))
        .await?;
    Ok(StatusCode::OK)
}

/// Delete an image, bytes included, for good. Answers with an empty body like the other mutations.
async fn delete_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    module
        .image_service
        .purge_image(ImageId::from_u64(id))
        .await?;
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
        module
            .image_service
            .generate_image_upload_url(request)
            .await?,
    ))
}
