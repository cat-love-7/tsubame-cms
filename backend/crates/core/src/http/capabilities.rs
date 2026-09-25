//! `GET /auth/capabilities`: what this deployment can do.
//!
//! Public, because a client needs the answer before it can sign in — it is what tells the
//! client whether to show a sign-in form at all, or to send the user to the deployment's
//! identity provider.

use axum::routing::get;
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::AppState;
use crate::models::capabilities::Capabilities;

/// The route, answering what the composition root decided about this backend.
pub fn routes<R: Storage>(capabilities: Capabilities) -> Router<AppState<R>> {
    Router::new().route(
        "/auth/capabilities",
        get(move || {
            let capabilities = capabilities;
            async move { Json(capabilities) }
        }),
    )
}
