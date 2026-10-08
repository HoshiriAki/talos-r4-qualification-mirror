use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::tenant_extractors::TrustedTenantUser;
use crate::state::AppState;

pub fn order_lifecycle_v2_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v2/orders/{id}/lifecycle", get(get_lifecycle))
        .route("/api/v2/orders/{id}/lifecycle/history", get(list_history))
        .route(
            "/api/v2/orders/{id}/lifecycle/actions/{action}",
            post(apply_action),
        )
        .route(
            "/api/v2/order-lifecycle/migration-exceptions",
            get(list_migration_exceptions),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActionBody {
    expected_version: i64,
    #[serde(default)]
    reason: String,
}

async fn get_lifecycle(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "order_lifecycle_v2",
            "get_lifecycle",
            serde_json::json!({ "orderId": id }),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn list_history(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "order_lifecycle_v2",
            "list_history",
            serde_json::json!({ "orderId": id }),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn apply_action(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<ActionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "order_lifecycle_v2",
            action.trim(),
            serde_json::json!({
                "orderId": id,
                "expectedVersion": body.expected_version,
                "reason": body.reason,
            }),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn list_migration_exceptions(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "order_lifecycle_v2",
            "list_migration_exceptions",
            serde_json::json!({}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}
