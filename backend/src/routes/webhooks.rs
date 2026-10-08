use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::error::AppError;
use crate::state::AppState;
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, RequestId, Revision, TenantId,
    TenantScope,
};

pub fn sf_express_routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/webhooks/sf-express", post(sf_express_webhook))
        .with_state(state)
}

async fn sf_express_webhook(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    body: String,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new("__system_webhook__").expect("static webhook scope is valid");
    let data_scope = DataScope::production(
        tenant_id.clone(),
        Revision::new("production-current").expect("static production revision is valid"),
    )
    .expect("webhook scope is valid");
    let ctx = ExecutionContext::new(
        ActorIdentity::system(),
        TenantScope::tenant(tenant_id),
        data_scope,
        ExecutionMode::Normal,
        RequestId::new(uuid::Uuid::new_v4().to_string()).expect("UUID request id is valid"),
        None,
        state.http_client.clone(),
    )
    .map_err(AppError::Internal)?;

    // 1. Verify signature
    let verify = state
        .registry
        .execute("sf_express", "verify_webhook", json!({"body": &body}), &ctx)
        .map_err(AppError::from_error_payload)?;

    if !verify
        .get("valid")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(Json(json!({
            "success": false,
            "errorCode": "SIGNATURE_MISMATCH",
            "errorMsg": "签名验证失败"
        })));
    }

    // 2. Process webhook
    let process = state
        .registry
        .execute(
            "sf_express",
            "process_webhook",
            json!({"body": &body}),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;

    // 3. Acknowledge the callback. SF Express expects this format.
    // The process result is logged but the webhook caller only gets ack.
    tracing::info!(
        "SF webhook processed: {}",
        serde_json::to_string(&process).unwrap_or_default()
    );

    Ok(Json(json!({
        "success": true,
        "errorCode": "",
        "errorMsg": "",
    })))
}
