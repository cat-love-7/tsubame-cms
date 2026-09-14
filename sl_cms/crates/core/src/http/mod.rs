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
/// What this deployment can do, for a client that has to ask.
pub mod capabilities;
pub mod collections;
pub mod composite_fields;
pub mod content;
pub mod images;
/// The password endpoints, which only a deployment that stores credentials itself has.
pub mod password_auth;
/// The byte-serving routes, which only a backend that stores the bytes itself has.
pub mod local_images;
pub mod single_pages;

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
use crate::models::user::{Permission, User};

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
    router_with(state, cors, Router::new(), Router::new())
}

/// The same router, plus the routes a particular backend adds.
///
/// Serving image bytes is the one capability the shared surface does not assume: an adapter
/// that stores them itself brings its own routes, and only its composition root merges them.
/// They are merged *before* the layers, so CORS and tracing apply to them like everything
/// else.
pub fn router_with<R: Storage>(
    state: AppState<R>,
    cors: CorsLayer,
    extra_public: Router<AppState<R>>,
    extra_protected: Router<AppState<R>>,
) -> Router {
    // Everything that reads or writes CMS content requires a token.
    let protected = Router::new()
        .merge(collections::routes::<R>())
        .merge(composite_fields::routes::<R>())
        .merge(single_pages::routes::<R>())
        .merge(images::protected_routes::<R>())
        .merge(auth::protected_routes::<R>())
        .merge(extra_protected)
        // `route_layer` applies only to the routes above, so unknown paths still 404
        // instead of being reported as 401.
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth::<R>,
        ));

    Router::new()
        .route("/", get(root))
        .merge(auth::public_routes::<R>())
        .merge(extra_public)
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

/// One resource that can carry permissions of its own.
///
/// Only collections and single pages do; everything else (the image library, the composite
/// field definitions, the site's structure) is governed by the account-wide permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource<'a> {
    Collection(&'a str),
    SinglePage(&'a str),
}

/// The resource a path is about, when it is about one.
///
/// The shapes are the two the routes use: `/models/collections/{name}/...` and
/// `/models/single_pages/{name}/...`. Anything else — including the lists at
/// `/models/collections` — answers `None`, and the account-wide permission applies.
fn resource_of(path: &str) -> Option<Resource<'_>> {
    let rest = path.strip_prefix("/models/")?;
    let (kind, rest) = rest.split_once('/')?;
    let name = rest.split('/').next().filter(|name| !name.is_empty())?;
    match kind {
        "collections" => Some(Resource::Collection(name)),
        "single_pages" => Some(Resource::SinglePage(name)),
        _ => None,
    }
}

/// The permission that governs a request about `resource`.
///
/// A per-resource entry replaces the account-wide permission for that resource (see
/// [`crate::models::user::User::permission_for_collection`]), so an override can both give
/// and take away.
pub fn permission_for(user: &User, resource: Option<Resource<'_>>) -> Permission {
    match resource {
        Some(Resource::Collection(name)) => user.permission_for_collection(name),
        Some(Resource::SinglePage(name)) => user.permission_for_single_page(name),
        None => user.permission,
    }
}

/// Authenticate the bearer token and apply coarse authorization.
///
/// Reads need `can_view`, every other method needs `can_edit` — judged against the resource
/// the path is about, so an account with a grant for one collection is not refused before the
/// handler can see which collection was asked for. The checks that depend on more than the
/// method — releasing content, or changing the site's structure — belong to the handlers,
/// which use [`require_publish`] and [`require_admin`], because one path often serves several
/// of them (a `GET` and a `DELETE` on the same URL, for instance).
async fn require_auth<R: Storage>(
    State(state): State<AppState<R>>,
    mut request: Request,
    next: Next,
) -> Result<Response, HttpError> {
    let token = bearer_token(request.headers())
        .ok_or_else(|| HttpError::Unauthorized("missing bearer token"))?;
    let user = state.auth_service.user_from_token(token).await?;

    let is_write = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    // `/auth/me` is self-service: everything under it acts on the caller's own account, so
    // it needs authentication but not permission to edit content. A read-only account must
    // still be able to change its password.
    let is_self_service = request.uri().path().starts_with("/auth/me");
    let permission = permission_for(&user, resource_of(request.uri().path()));
    if is_write && !is_self_service {
        if !user.can_write(permission) {
            return Err(HttpError::Forbidden("edit permission required"));
        }
    } else if !is_write && !user.can_read(permission) {
        return Err(HttpError::Forbidden("view permission required"));
    }

    request.extensions_mut().insert(AuthenticatedUser(user));
    Ok(next.run(request).await)
}

/// Releasing or hiding content, and deleting it.
///
/// Safe to keep separate from "may edit" because the draft a user edits is not what the
/// delivery API serves; only publishing copies it across. `resource` is the collection or
/// page the request is about, so a per-resource grant applies here too.
pub fn require_publish(user: &User, resource: Option<Resource<'_>>) -> Result<(), HttpError> {
    if user.can_publish(permission_for(user, resource)) {
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
        // A code the client can translate, and the English message for anyone reading a log or
        // curling the API. Plain text would leave a client with nothing but English to show.
        let mut response = (
            status,
            axum::Json(serde_json::json!({ "code": self.code, "message": self.message })),
        )
            .into_response();
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
