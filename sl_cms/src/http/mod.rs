//! HTTP layer, shared by every storage backend.
//!
//! The routers here are deliberately storage-agnostic: they only touch `AppModule`'s
//! services, so the same routes serve the on-premises adapter locally and the DynamoDB +
//! S3 adapter on AWS Lambda.
//!
//! Mutating endpoints answer `200 OK` with an **empty body**. The Angular client asks for
//! JSON and parses any non-empty body as JSON, so the human-readable "… successfully"
//! strings these routes used to return made a successful save or delete look like a failed
//! request to the only client there is.

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
use crate::preview_link::PreviewLinkError;
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
        // Shareable preview links. Public by design: the signature in the query string is
        // the credential, and it only ever opens the one working copy it was made for.
        .merge(collections::preview_routes::<R>())
        .merge(single_pages::preview_routes::<R>())
        .merge(protected)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Authenticate the bearer token and apply coarse authorization.
///
/// Reads need `can_view`, every other method needs `can_edit`. The checks that depend on
/// more than the method — releasing content, or changing the site's structure — belong to
/// the handlers, which use [`require_publish`] and [`require_admin`], because one path
/// often serves several of them (a `GET` and a `DELETE` on the same URL, for instance).
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
    // `/auth/me` is self-service: everything under it acts on the caller's own account, so
    // it needs authentication but not permission to edit content. A read-only account must
    // still be able to change its password.
    let is_self_service = request.uri().path().starts_with("/auth/me");
    if is_write && !is_self_service {
        if !user.can_write() {
            return Err(HttpError::Forbidden("edit permission required"));
        }
    } else if !is_write && !user.can_read() {
        return Err(HttpError::Forbidden("view permission required"));
    }

    request.extensions_mut().insert(AuthenticatedUser(user));
    Ok(next.run(request).await)
}

/// Releasing or hiding content, and deleting it.
///
/// Safe to keep separate from "may edit" because the draft a user edits is not what the
/// delivery API serves; only publishing copies it across.
pub fn require_publish(user: &User) -> Result<(), HttpError> {
    if user.can_publish() {
        Ok(())
    } else {
        Err(HttpError::Forbidden("publish permission required"))
    }
}

/// Changing the shape of the site (schemas, collections, pages) or managing accounts.
///
/// Editing content is not the same as changing what content can exist, so this is
/// deliberately not implied by `can_edit`.
pub fn require_admin(user: &User) -> Result<(), HttpError> {
    if user.is_admin {
        Ok(())
    } else {
        Err(HttpError::Forbidden("administrator permission required"))
    }
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

/// The `?token=` that a shareable preview link carries.
#[derive(serde::Deserialize)]
pub(crate) struct PreviewTokenQuery {
    pub token: String,
}

/// An expired link is a matter of time rather than identity, so it is answered differently
/// from one that was never ours.
pub(crate) fn preview_link_error(error: PreviewLinkError) -> HttpError {
    match error {
        PreviewLinkError::Expired => HttpError::Forbidden(error.message()),
        PreviewLinkError::Malformed | PreviewLinkError::Invalid => {
            HttpError::Unauthorized(error.message())
        }
    }
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
        let retry_after = self.retry_after_seconds;
        let mut response = (status, self.message).into_response();
        // Only the 429 answers set this, and a client that knows when to come back does not
        // have to guess.
        if let Some(seconds) = retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}
