//! 通知系统 REST 端点
//! 发送/模板管理：受信租户管理员上下文；站内信读取：受信租户用户上下文
//! POST /api/notify/send
//! POST /api/notify/list
//! POST /api/notify/mark-read
//! POST /api/notify/templates
//! POST /api/notify/trigger

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::tenant_extractors::{TrustedTenantAdmin, TrustedTenantUser};
use crate::state::AppState;

pub fn notify_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Admin-only
        .route("/api/notify/send", post(notify_send))
        .route("/api/notify/templates", post(notify_templates))
        .route("/api/notify/templates/upsert", post(notify_upsert_template))
        // Auth-required (reading own messages, triggering events)
        .route("/api/notify/list", post(notify_list))
        .route("/api/notify/mark-read", post(notify_mark_read))
        .route("/api/notify/trigger", post(notify_trigger))
}

// ══════════════════════════════════════════════════════════════════
// Admin endpoints
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendBody {
    event_type: String,
    channel: Option<String>,
    recipient: Option<String>,
    variables: Option<serde_json::Value>,
}

async fn notify_send(
    State(state): State<Arc<AppState>>,
    admin: TrustedTenantAdmin,
    Json(body): Json<SendBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let payload = serde_json::json!({
        "eventType": body.event_type,
        "channel": body.channel,
        "recipient": body.recipient,
        "variables": body.variables,
    });
    let result = state
        .registry
        .execute("notify", "send", payload, admin.context())
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplatesBody {
    #[serde(default)]
    action: String, // "list" or "upsert"
    id: Option<String>,
    event_type: Option<String>,
    channel: Option<String>,
    subject_template: Option<String>,
    body_template: Option<String>,
    is_enabled: Option<bool>,
}

async fn notify_templates(
    State(state): State<Arc<AppState>>,
    admin: TrustedTenantAdmin,
    Json(_body): Json<TemplatesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = state
        .registry
        .execute(
            "notify",
            "get_templates",
            serde_json::json!({}),
            admin.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertTemplateBody {
    id: Option<String>,
    event_type: String,
    channel: String,
    subject_template: String,
    body_template: String,
    is_enabled: Option<bool>,
}

async fn notify_upsert_template(
    State(state): State<Arc<AppState>>,
    admin: TrustedTenantAdmin,
    Json(body): Json<UpsertTemplateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let payload = serde_json::json!({
        "id": body.id,
        "eventType": body.event_type,
        "channel": body.channel,
        "subjectTemplate": body.subject_template,
        "bodyTemplate": body.body_template,
        "isEnabled": body.is_enabled,
    });
    let result = state
        .registry
        .execute("notify", "upsert_template", payload, admin.context())
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// Auth-required endpoints
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListBody {
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn notify_list(
    State(state): State<Arc<AppState>>,
    auth: TrustedTenantUser,
    Json(body): Json<ListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let payload = serde_json::json!({
        "page": body.page,
        "pageSize": body.page_size,
    });
    let result = state
        .registry
        .execute("notify", "list", payload, auth.context())
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkReadBody {
    message_id: Option<String>,
}

async fn notify_mark_read(
    State(state): State<Arc<AppState>>,
    auth: TrustedTenantUser,
    Json(body): Json<MarkReadBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let payload = serde_json::json!({
        "messageId": body.message_id,
    });
    let result = state
        .registry
        .execute("notify", "mark_read", payload, auth.context())
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TriggerBody {
    event_type: String,
    order_id: Option<String>,
    variables: Option<serde_json::Value>,
}

async fn notify_trigger(
    State(state): State<Arc<AppState>>,
    admin: TrustedTenantAdmin,
    Json(body): Json<TriggerBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let payload = serde_json::json!({
        "eventType": body.event_type,
        "orderId": body.order_id,
        "variables": body.variables,
    });
    let result = state
        .registry
        .execute("notify", "trigger", payload, admin.context())
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}
