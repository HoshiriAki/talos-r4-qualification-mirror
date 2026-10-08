//! Unbound legacy keys cannot be safely assigned a tenant by inference.
//! Preserve route addresses, but require explicit machine-client provisioning.
use crate::{error::AppError, middleware::auth::AdminUser, state::AppState};
use axum::{
    Router,
    routing::{delete, get, put},
};
use std::sync::Arc;

pub fn api_key_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/keys", get(unbound).post(unbound))
        .route("/api/keys/{id}", delete(unbound))
        .route("/api/keys/{id}/toggle", put(unbound))
}

async fn unbound(_admin: AdminUser) -> Result<(), AppError> {
    Err(AppError::Gone {
        code:"API_LEGACY_UNBOUND".into(),
        message:"Legacy keys have no tenant authority. Provision a scoped client at /api/machine-clients.".into(),
    })
}
