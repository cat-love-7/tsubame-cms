//! HTTP layer, shared by every storage backend.
//!
//! The routers here are deliberately storage-agnostic: they only touch `AppModule`'s
//! services, so the same routes serve the on-premises adapter locally and the DynamoDB +
//! S3 adapter on AWS Lambda.

pub mod collections;
pub mod composite_fields;
pub mod images;
pub mod single_pages;

use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::app_module::{AppModule, Storage};
use crate::models::error::HttpError;

pub type AppState<R> = Arc<AppModule<R>>;

/// Build the CORS layer for the configured origins.
///
/// A single `*` entry allows any origin, which is intended for local development only —
/// the previous implementation used `Cors::permissive()` unconditionally.
pub fn cors_layer(origins: &[String]) -> CorsLayer {
    let base = CorsLayer::new().allow_methods(Any).allow_headers(Any);
    if origins.iter().any(|o| o == "*") {
        base.allow_origin(Any)
    } else {
        let allowed: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
        base.allow_origin(AllowOrigin::list(allowed))
    }
}

pub fn router<R: Storage>(state: AppState<R>, cors: CorsLayer) -> Router {
    Router::new()
        .route("/", get(root))
        .merge(collections::routes::<R>())
        .merge(composite_fields::routes::<R>())
        .merge(single_pages::routes::<R>())
        .merge(images::routes::<R>())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Liveness endpoint. AWS Lambda Web Adapter also uses this as its readiness check, so it
/// must answer 200 without touching storage.
async fn root() -> impl IntoResponse {
    (StatusCode::OK, "SL CMS API")
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, self.message).into_response()
    }
}
