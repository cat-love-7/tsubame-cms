//! Account endpoints, shared by every deployment.
//!
//! An administrator manages accounts in both deployments - that is what makes them accounts rather
//! than rows in a table - so creating one, changing what it may do and removing it live here. What
//! is *not* here is anything to do with the credential's *value*: verifying a password, completing
//! a reset link, changing your own password. Those exist only where the CMS stores the credential
//! itself, and they live in [`crate::http::password_auth`] for that reason.
//!
//! A password *reset* is here even so, because it is administration in both: what comes back is
//! different (a link the owner completes, or a temporary password the provider already set), and
//! the answer says which.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::{AppState, AuthenticatedUser, require_admin};
use crate::models::error::HttpError;
use crate::models::user::{NewAccountRequest, UpdateUserRequest, UserId};

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
        .route("/auth/users", get(list_users::<R>).post(create_user::<R>))
        .route(
            "/auth/users/{id}",
            patch(update_user::<R>).delete(delete_user::<R>),
        )
        // Not `…/password-reset-link`: where an identity provider owns the credential there is no
        // link, and only the answer knows which of the two an administrator has to pass on.
        .route(
            "/auth/users/{id}/password-reset",
            post(issue_password_reset::<R>),
        )
}

async fn me(Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>) -> impl IntoResponse {
    Json(user.to_response())
}

/// Create an account. Nothing here chooses a credential: the provider issues one, or the account's
/// owner does by following a reset (`AuthService::create_user` says why).
async fn create_user<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Json(request): Json<NewAccountRequest>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let created = module.auth_service.create_user(request).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Give an account a new way in, and answer with what the administrator hands over.
async fn issue_password_reset<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    Ok(Json(
        module
            .auth_service
            .issue_password_reset(&UserId::from(id.as_str()))
            .await?,
    ))
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
        let known = module.collection_service.list_collections().await?;
        for name in overrides.keys() {
            if !known.iter().any(|collection| collection.as_str() == name) {
                return Err(HttpError::BadRequest(&format!(
                    "unknown collection '{name}'"
                )));
            }
        }
    }
    if let Some(overrides) = request.single_page_permissions.as_ref() {
        let known = module.single_page_service.list_page_names().await?;
        for name in overrides.keys() {
            if !known.iter().any(|page| page.as_str() == name) {
                return Err(HttpError::BadRequest(&format!(
                    "unknown single page '{name}'"
                )));
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
    module
        .auth_service
        .delete_user(&UserId::from(id.as_str()))
        .await?;
    // Empty body, like the other mutations.
    Ok(StatusCode::OK)
}
