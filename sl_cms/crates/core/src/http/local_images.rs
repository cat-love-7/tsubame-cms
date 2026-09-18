//! Serving and accepting image bytes, for a backend that stores them itself.
//!
//! An adapter backed by object storage hands the browser an absolute, signed URL and the
//! bytes never reach this application, so these routes are not part of the shared surface.
//! They exist for the local adapter, and that backend's composition root is what puts them
//! into the router (see [`crate::http::router_with`]).
//!
//! Both are deliberately in one place: whoever serves the bytes is also who authorises the
//! upload of them.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, put};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::error::HttpError;
use crate::models::image::is_safe_file_name;
use crate::repositories::local_image_bytes::LocalImageBytes;

/// Reading the bytes back. Public: an `<img>` tag cannot send an `Authorization` header, and
/// uploaded media is content rather than API data.
pub fn public_routes<R: Storage + LocalImageBytes>() -> Router<AppState<R>> {
    Router::new()
        .route("/images/{file_name}", get(get_image_file::<R>))
        // The *durable* link to an image: the file name changes when the bytes are replaced, the id
        // does not, so this is what a hand-written link (in a Markdown body, say) should use. It
        // answers with the bytes rather than pointing at them because this deployment is reached
        // through a prefix of its own (`/api` behind the site's proxy), which the backend cannot
        // know from a backend-relative URL.
        .route("/images/by-id/{id}", get(get_image_by_id::<R>))
}

/// Serve whatever the image shows right now, under the link that names it by id.
async fn get_image_by_id<R: Storage + LocalImageBytes>(
    State(module): State<AppState<R>>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, HttpError> {
    let Some(url) = module
        .image_service
        .image_url(crate::models::image::ImageId::from_u64(id))
        .await?
    else {
        return Err(HttpError::NotFound("Image not found"));
    };
    // The stored URL is where the bytes are now; the file name is its last segment.
    let file_name = url.rsplit('/').next().unwrap_or_default();
    match module.image_service.read_image_bytes(file_name).await? {
        Some(data) => Ok((
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                // The bytes behind an id can change, so this answer must not be cached: a reader
                // would keep showing the picture that was replaced.
                (header::CACHE_CONTROL, "no-cache"),
            ],
            data,
        )),
        None => Err(HttpError::NotFound("File not found")),
    }
}

/// Accepting the bytes, with the one-shot token the upload URL carried. The route sits behind
/// the same auth middleware as the rest of the admin API; the token is what actually
/// authorises the write, and it is bound to this file name.
pub fn protected_routes<R: Storage + LocalImageBytes>() -> Router<AppState<R>> {
    Router::new().route("/images/{file_name}", put(put_image_file::<R>))
}

#[derive(serde::Deserialize)]
struct UploadQuery {
    key: String,
}

async fn put_image_file<R: Storage + LocalImageBytes>(
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
    let Some(authorised) = module.image_service.take_upload_key(&query.key).await? else {
        return Err(HttpError::Unauthorized("Invalid upload key"));
    };
    if authorised != file_name {
        return Err(HttpError::new(
            403,
            "Upload key was issued for a different file name",
        ));
    }

    module
        .image_service
        .write_image_bytes(&file_name, &body)
        .await?;
    Ok((StatusCode::CREATED, ()))
}
async fn get_image_file<R: Storage + LocalImageBytes>(
    State(module): State<AppState<R>>,
    Path(file_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    // Rejected here so the storage adapter is never asked to touch an unsafe path.
    if !is_safe_file_name(&file_name) {
        return Err(HttpError::BadRequest("Invalid file name"));
    }
    match module.image_service.read_image_bytes(&file_name).await? {
        Some(data) => Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data)),
        None => Err(HttpError::NotFound("File not found")),
    }
}
