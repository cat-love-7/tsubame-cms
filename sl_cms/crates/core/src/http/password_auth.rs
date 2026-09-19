//! The password endpoints, for a deployment that stores credentials itself.
//!
//! These are the routes that cannot exist where sign-in belongs to an identity provider: there is
//! no password for the CMS to check, none for it to complete a reset with, and none for it to
//! change on someone's behalf. A backend that has an [`LocalCredentials`] implementation composes
//! them (the on-premises one does); a backend that does not registers the same paths answering 501,
//! so a client that guessed them learns which deployment this is instead of "wrong URL".
//!
//! What is left in [`crate::http::auth`] — who am I, list accounts, create one, change a role or a
//! permission, remove one, reset a password — is shared, because an administrator manages accounts
//! in both deployments; only the credential's value differs.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{Router, post};

use crate::app_module::Storage;
use crate::http::{AppState, AuthenticatedUser};
use crate::models::error::HttpError;
use crate::models::user::{ChangePasswordRequest, CompletePasswordResetRequest, LoginRequest};
use crate::repositories::local_credentials::LocalCredentials;

/// Reachable without a token: signing in, and finishing a reset (the link *is* the credential).
pub fn public_routes<R: Storage + LocalCredentials>() -> Router<AppState<R>> {
    Router::new()
        .route("/auth/login", post(login::<R>))
        .route("/auth/password-reset", post(complete_password_reset::<R>))
}

/// Account routes that touch a credential; the caller needs to be an administrator.
pub fn protected_routes<R: Storage + LocalCredentials>() -> Router<AppState<R>> {
    Router::new()
        // Self-service: any authenticated account may change its own password.
        .route("/auth/me/password", post(change_own_password::<R>))
}

async fn login<R: Storage + LocalCredentials>(
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

/// Set a new password with an issued link. Public: the signature is the credential.
async fn complete_password_reset<R: Storage + LocalCredentials>(
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

async fn change_own_password<R: Storage + LocalCredentials>(
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

/// The same paths a deployment without local passwords does *not* have, answering 501.
///
/// Leaving them unregistered would answer 404, which a client that guessed the path would read
/// as "wrong URL" rather than "this deployment signs users in elsewhere". The message says
/// which deployment this is, so the answer is actionable.
pub fn unavailable_public<R: Storage>(message: &'static str) -> Router<AppState<R>> {
    Router::new()
        .route("/auth/login", post(unavailable::<R>(message)))
        .route("/auth/password-reset", post(unavailable::<R>(message)))
}

/// The one account route that touches a credential's *value*: changing your own password.
pub fn unavailable_protected<R: Storage>(message: &'static str) -> Router<AppState<R>> {
    Router::new().route("/auth/me/password", post(unavailable::<R>(message)))
}

fn unavailable<R: Storage>(
    message: &'static str,
) -> impl Fn() -> std::future::Ready<Result<StatusCode, HttpError>> + Clone + Send + Sync + 'static
{
    move || std::future::ready(Err(HttpError::new(501, message)))
}
