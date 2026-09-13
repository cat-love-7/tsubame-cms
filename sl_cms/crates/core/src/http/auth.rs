//! Account endpoints, shared by every deployment.
//!
//! What is *not* here is anything to do with passwords: verifying one, setting one, resetting
//! one. Those exist only where the CMS stores the credential itself, and they live in
//! [`crate::http::password_auth`] for that reason — a deployment that signs its users in
//! through an identity provider has no such routes rather than routes that refuse to work.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::{require_admin, AppState, AuthenticatedUser};
use crate::models::error::HttpError;
use crate::models::user::{UpdateUserRequest, UserId};

/// Routes reachable without a token. Only login qualifies.
/// Routes that need a token (the auth middleware has already run).
pub fn public_routes<R: Storage>() -> Router<AppState<R>> {
    // Everything here is account administration, which needs a token; the auth middleware
    // wraps it. Nothing in this module is reachable without one.
    Router::new()
}

/// Routes that require an authenticated caller (the auth middleware has already run).
pub fn protected_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/auth/me", get(me))
        .route("/auth/users", get(list_users::<R>))
        .route(
            "/auth/users/{id}",
            patch(update_user::<R>).delete(delete_user::<R>),
        )
}

async fn me(Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>) -> impl IntoResponse {
    Json(user.to_response())
}

async fn list_users<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    Ok(Json(module.auth_service.list_users().await?))
}

async fn update_user<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(request): Json<UpdateUserRequest>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    // A grant for a collection that does not exist would be invisible in every screen and
    // silently inert, so it is refused rather than stored.
    if let Some(overrides) = request.collection_permissions.as_ref() {
        let known = module.collection_service.get_all_collections().await?;
        for name in overrides.keys() {
            if !known.iter().any(|collection| collection.as_str() == name) {
                return Err(HttpError::BadRequest(&format!(
                    "unknown collection '{name}'"
                )));
            }
        }
    }
    if let Some(overrides) = request.single_page_permissions.as_ref() {
        let known = module.single_page_service.get_all_page_names().await?;
        for name in overrides.keys() {
            if !known.iter().any(|page| page.as_str() == name) {
                return Err(HttpError::BadRequest(&format!("unknown single page '{name}'")));
            }
        }
    }

    Ok(Json(
        module
            .auth_service
            .update_user(&UserId::from(id.as_str()), request)
            .await?,
    ))
}

async fn delete_user<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    module.auth_service.delete_user(&UserId::from(id.as_str())).await?;
    // Empty body, like the other mutations.
    Ok(StatusCode::OK)
}

