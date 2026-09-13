use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
use crate::models::image::{is_safe_file_name, is_safe_image_ext, NewImageRequest};

/// Image bytes are served without a token: an `<img>` tag cannot send an
/// `Authorization` header, and uploaded media is content rather than API data.
pub fn public_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new().route("/images/{file_name}", get(get_image_file::<R>))
}

/// Uploading requires authentication (enforced by the auth middleware).
pub fn protected_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route(
            "/models/images/get_upload_url",
            post(generate_image_upload_url::<R>),
        )
        .route("/images/{file_name}", axum::routing::put(put_image_file::<R>))
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

#[derive(serde::Deserialize)]
struct UploadQuery {
    key: String,
}

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
        return Err(HttpError::new(401, "Invalid upload key"));
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
