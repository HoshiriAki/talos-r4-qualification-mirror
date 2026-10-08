//! HTTP adapter for the R3 Damage / Repair / Settlement module.
//!
//! The adapter has no business or persistence authority: every command,
//! including reads, is executed through the named `r3_settlement` Registry
//! module using the trusted tenant context extracted by middleware.

use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

use crate::error::AppError;
use crate::middleware::tenant_extractors::TrustedTenantUser;
use crate::observability::{
    MetricsSink, RuntimeMetrics, SettlementBlocker, SettlementOutcome, classify_settlement_error,
};
use crate::state::AppState;

pub fn r3_settlement_routes() -> Router<Arc<AppState>> {
    Router::new().route(
        "/api/v3/rental-settlement/commands/{command}",
        post(execute),
    )
}

async fn execute(
    State(state): State<Arc<AppState>>,
    Extension(metrics): Extension<Arc<RuntimeMetrics>>,
    tenant_user: TrustedTenantUser,
    Path(command): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let command = command.trim();
    let result = state
        .registry
        .execute("r3_settlement", command, payload, tenant_user.context());
    let (outcome, blocker) = match &result {
        Ok(_) => (SettlementOutcome::Succeeded, SettlementBlocker::None),
        Err(error) => classify_settlement_error(error),
    };
    metrics.settlement(command, outcome, blocker);
    result.map(Json).map_err(AppError::from_error_payload)
}
