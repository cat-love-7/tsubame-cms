//! HTTP layer, shared by every storage backend.
//!
//! The routers here are deliberately storage-agnostic: they only touch `AppModule`'s
//! services, so the same routes serve the on-premises adapter locally and the DynamoDB +
//! S3 adapter on AWS Lambda.

pub mod auth;
pub mod collections;
pub mod composite_fields;
pub mod content;
pub mod images;
pub mod single_pages;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::app_module::{AppModule, Storage};
use crate::models::error::HttpError;
use crate::models::user::User;

pub type AppState<R> = Arc<AppModule<R>>;

/// The caller behind the current request, inserted by [`require_auth`].
#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub User);

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
    // Everything that reads or writes CMS content requires a token.
    let protected = Router::new()
        .merge(collections::routes::<R>())
        .merge(composite_fields::routes::<R>())
        .merge(single_pages::routes::<R>())
        .merge(images::protected_routes::<R>())
        .merge(auth::protected_routes::<R>())
        // `route_layer` applies only to the routes above, so unknown paths still 404
        // instead of being reported as 401.
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth::<R>,
        ));

    Router::new()
        .route("/", get(root))
        .merge(auth::public_routes::<R>())
        // Image bytes are public: an <img> tag cannot send an Authorization header, and
        // uploaded media is served content rather than API data.
        .merge(images::public_routes::<R>())
        // The read-only content API a site build consumes. Published content only, so it
        // needs no token.
        .merge(content::routes::<R>())
        .merge(protected)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Authenticate the bearer token and apply coarse authorization.
///
/// Reads are allowed to any authenticated account; writes additionally require edit
/// rights (or administrator status). Fine-grained, per-resource permissions are not
/// implemented yet.
async fn require_auth<R: Storage>(
    State(state): State<AppState<R>>,
    mut request: Request,
    next: Next,
) -> Result<Response, HttpError> {
    let token = bearer_token(request.headers())
        .ok_or_else(|| HttpError::Unauthorized("missing bearer token"))?;
    let user = state.auth_service.user_from_token(token)?;

    let is_write = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if is_write && !user.can_write() {
        return Err(HttpError::Forbidden("edit permission required"));
    }

    request.extensions_mut().insert(AuthenticatedUser(user));
    Ok(next.run(request).await)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// Liveness endpoint. AWS Lambda Web Adapter also uses this as its readiness check, so it
/// must answer 200 without touching storage or requiring a token.
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
