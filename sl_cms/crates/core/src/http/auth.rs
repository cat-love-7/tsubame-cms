//! Authentication endpoints.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::{require_admin, AppState, AuthenticatedUser};
use crate::models::error::HttpError;
use crate::models::user::{
    ChangePasswordRequest, CompletePasswordResetRequest, LoginRequest, NewUserRequest,
    ResetPasswordRequest, UpdateUserRequest, UserId,
};

/// Routes reachable without a token. Only login qualifies.
pub fn public_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/auth/login", post(login::<R>))
        // Completing a reset is public by design: the link *is* the credential, and the whole
        // point is that someone who cannot sign in can use it.
        .route("/auth/password-reset", post(complete_password_reset::<R>))
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
        .route("/auth/users/{id}/password", post(reset_password::<R>))
        .route(
            "/auth/users/{id}/password-reset-link",
            post(issue_password_reset::<R>),
        )
        // Self-service: any authenticated account may change its own password.
        .route("/auth/me/password", post(change_own_password::<R>))
}

async fn login<R: Storage>(
    State(module): State<AppState<R>>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .auth_service
            .login(&request.username, &request.password)
            .await?,
    ))
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

async fn create_user<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Json(request): Json<NewUserRequest>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let created = module.auth_service.create_user(request).await?;
    Ok((StatusCode::CREATED, Json(created)))
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

async fn reset_password<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(request): Json<ResetPasswordRequest>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    module
        .auth_service
        .set_password(&UserId::from(id.as_str()), &request.password)
        .await?;
    Ok(StatusCode::OK)
}

/// Issue a link an administrator passes on, so the account sets its own new password.
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

/// Set a new password with an issued link. Public: the signature is the credential.
async fn complete_password_reset<R: Storage>(
    State(module): State<AppState<R>>,
    Json(request): Json<CompletePasswordResetRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .auth_service
            .complete_password_reset(&request.token, &request.new_password)
            .await?,
    ))
}

async fn change_own_password<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, HttpError> {
    // Answers with a token for the new generation: the change ends this session along
    // with every other one.
    Ok(Json(
        module
            .auth_service
            .change_own_password(&user.id, &request.current_password, &request.new_password)
            .await?,
    ))
}
