#[cfg(feature = "on-premises")]
use axum::body::Bytes;
use axum::extract::{Path, State};
#[cfg(feature = "on-premises")]
use axum::extract::Query;
#[cfg(feature = "on-premises")]
use axum::http::header;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
#[cfg(feature = "on-premises")]
use crate::models::image::is_safe_file_name;
use crate::models::image::{is_safe_image_ext, ImageID, NewImageRequest};

/// Image bytes are served without a token: an `<img>` tag cannot send an
/// `Authorization` header, and uploaded media is content rather than API data.
///
/// Only a local adapter serves them at all: an object-storage adapter hands out absolute
/// URLs and the browser fetches them from there (see [`LocalImageBytes`]).
#[cfg(feature = "on-premises")]
pub fn public_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new().route("/images/{file_name}", get(get_image_file::<R>))
}

#[cfg(not(feature = "on-premises"))]
pub fn public_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
}

/// Uploading requires authentication (enforced by the auth middleware).
pub fn protected_routes<R: Storage>() -> Router<AppState<R>> {
    // The library itself. `get_upload_url` below is a static segment, which axum prefers over
    // `{id}`, so both can live here.
    let router = Router::new()
        .route("/models/images", get(list_images::<R>))
        .route("/models/images/{id}", delete(delete_image::<R>))
        .route(
            "/models/images/get_upload_url",
            post(generate_image_upload_url::<R>),
        );

    // Accepting the bytes is a local adapter's business, like serving them.
    #[cfg(feature = "on-premises")]
    let router = router.route("/images/{file_name}", axum::routing::put(put_image_file::<R>));
    router
}

/// Every uploaded image, newest first.
async fn list_images<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(module.image_service.list_images()?))
}

/// Delete an image, bytes included. Answers with an empty body like the other mutations.
async fn delete_image<R: Storage>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    module.image_service.delete_image(ImageID::from_u64(id))?;
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
        module.image_service.generate_image_upload_url(request)?,
    ))
}

#[cfg(feature = "on-premises")]
async fn get_image_file<R: Storage>(
    State(module): State<AppState<R>>,
    Path(file_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    // Rejected here so the storage adapter is never asked to touch an unsafe path.
    if !is_safe_file_name(&file_name) {
        return Err(HttpError::BadRequest("Invalid file name"));
    }
    match module.image_service.read_image_bytes(&file_name)? {
        Some(data) => Ok((
            [(header::CONTENT_TYPE, "application/octet-stream")],
            data,
        )),
        None => Err(HttpError::NotFound("File not found")),
    }
}

#[cfg(feature = "on-premises")]
#[derive(serde::Deserialize)]
struct UploadQuery {
    key: String,
}

#[cfg(feature = "on-premises")]
async fn put_image_file<R: Storage>(
    State(module): State<AppState<R>>,
    Path(file_name): Path<String>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<impl IntoResponse, HttpError> {
    if !is_safe_file_name(&file_name) {
        return Err(HttpError::BadRequest("Invalid file name"));
    }

    // The token is single-use and bound to one file name, so a token cannot be replayed
    // and cannot be used to write a different file than the one it was issued for.
    let Some(authorised) = module.image_service.take_upload_key(&query.key)? else {
        return Err(HttpError::Unauthorized("Invalid upload key"));
    };
    if authorised != file_name {
        return Err(HttpError::new(
            403,
            "Upload key was issued for a different file name",
        ));
    }

    module.image_service.write_image_bytes(&file_name, &body)?;
    Ok((StatusCode::CREATED, ()))
}
