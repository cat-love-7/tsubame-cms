//! Authentication endpoints.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::{require_admin, AppState, AuthenticatedUser};
use crate::models::error::HttpError;
use crate::models::user::{LoginRequest, NewUserRequest};

/// Routes reachable without a token. Only login qualifies.
pub fn public_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new().route("/auth/login", post(login::<R>))
}

/// Routes that require an authenticated caller (the auth middleware has already run).
pub fn protected_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/auth/me", get(me))
        .route("/auth/users", get(list_users::<R>).post(create_user::<R>))
}

async fn login<R: Storage>(
    State(module): State<AppState<R>>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        module
            .auth_service
            .login(&request.email, &request.password)?,
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
    Ok(Json(module.auth_service.list_users()?))
}

async fn create_user<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Json(request): Json<NewUserRequest>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let created = module.auth_service.create_user(request)?;
    Ok((StatusCode::CREATED, Json(created)))
}
