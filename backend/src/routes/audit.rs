use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::AdminUser;
use crate::registry::make_ctx;
use crate::state::AppState;
use crate::utils::http as http_utils;

// ── Route definition ──────────────────────────────────────────────

pub fn audit_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .route("/audit-logs/export", get(export_audit_logs))
}

// ── Query params ──────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct AuditQuery {
    #[serde(alias = "actionType")]
    action_type: Option<String>,
    #[serde(alias = "entityType")]
    entity_type: Option<String>,
    #[serde(alias = "actorUsername")]
    actor_username: Option<String>,
    keyword: Option<String>,
    date: Option<String>,
    limit: Option<String>,
    offset: Option<String>,
}

// ── GET /audit ────────────────────────────────────────────────────

async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Query(q): Query<AuditQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin, state.http_client.clone());
    let value = state
        .registry
        .execute(
            "audit",
            "list_audit_logs",
            serde_json::json!({
                "actionType": q.action_type.unwrap_or_default(),
                "entityType": q.entity_type.unwrap_or_default(),
                "actorUsername": q.actor_username.unwrap_or_default(),
                "keyword": q.keyword.unwrap_or_default(),
                "date": q.date.unwrap_or_default(),
                "limit": q
                    .limit
                    .as_deref()
                    .map(|value| value.parse().unwrap_or(100))
                    .unwrap_or(100),
                "offset": q
                    .offset
                    .as_deref()
                    .map(|value| value.parse().unwrap_or(0))
                    .unwrap_or(0),
            }),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;
    let result: system_admin::audit::ListAuditLogsOutput =
        serde_json::from_value(value).map_err(|error| {
            AppError::Internal(format!("Failed to decode audit log projection: {error}"))
        })?;
    let logs = result
        .logs
        .into_iter()
        .map(|log| {
            let detail =
                serde_json::from_str(&log.detail_json).unwrap_or_else(|_| serde_json::json!({}));
            serde_json::json!({
                "id": log.id,
                "actorIdentityId": log.actor_identity_id,
                "actorUsername": log.actor_username,
                "actionType": log.action_type,
                "entityType": log.entity_type,
                "entityId": log.entity_id,
                "entityLabel": log.entity_label,
                "detailJson": log.detail_json,
                "ip": log.ip,
                "userAgent": log.user_agent,
                "createdAt": log.created_at,
                "detail": detail,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({
        "logs": logs,
        "limit": result.limit,
        "total": result.total,
    })))
}

// ── GET /audit/export ─────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct AuditExportQuery {
    #[serde(alias = "actionType")]
    action_type: Option<String>,
    #[serde(alias = "entityType")]
    entity_type: Option<String>,
    #[serde(alias = "actorUsername")]
    actor_username: Option<String>,
    keyword: Option<String>,
    date: Option<String>,
}

async fn export_audit_logs(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Query(q): Query<AuditExportQuery>,
) -> Result<Response, AppError> {
    let ctx = make_ctx(&admin, state.http_client.clone());
    let value = state
        .registry
        .execute(
            "audit",
            "list_audit_logs",
            serde_json::json!({
                "actionType": q.action_type.unwrap_or_default(),
                "entityType": q.entity_type.unwrap_or_default(),
                "actorUsername": q.actor_username.unwrap_or_default(),
                "keyword": q.keyword.unwrap_or_default(),
                "date": q.date.unwrap_or_default(),
                "limit": 500,
                "offset": 0,
            }),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;
    let result: system_admin::audit::ListAuditLogsOutput =
        serde_json::from_value(value).map_err(|error| {
            AppError::Internal(format!("Failed to decode audit log projection: {error}"))
        })?;

    use rust_xlsxwriter::*;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("audit_logs")
        .map_err(|e| AppError::Internal(format!("Failed to set sheet name: {}", e)))?;

    let headers = [
        "行为类型",
        "实体类型",
        "实体ID",
        "实体标签",
        "操作人ID",
        "操作人用户名",
        "详情",
        "IP",
        "UserAgent",
        "创建时间",
    ];

    let header_format = Format::new().set_bold();
    for (col, header) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(0, col as u16, *header, &header_format)
            .map_err(|e| AppError::Internal(format!("Failed to write header: {}", e)))?;
    }

    for (row_idx, log) in result.logs.iter().enumerate() {
        let excel_row = (row_idx + 1) as u32;
        let values = [
            log.action_type.as_str(),
            log.entity_type.as_str(),
            log.entity_id.as_str(),
            log.entity_label.as_str(),
            log.actor_identity_id.as_str(),
            log.actor_username.as_str(),
            log.detail_json.as_str(),
            log.ip.as_str(),
            log.user_agent.as_str(),
            log.created_at.as_str(),
        ];
        for (col, value) in values.iter().enumerate() {
            worksheet
                .write_string(excel_row, col as u16, *value)
                .map_err(|e| AppError::Internal(format!("Failed to write cell: {}", e)))?;
        }
    }

    let buffer = workbook
        .save_to_buffer()
        .map_err(|e| AppError::Internal(format!("Failed to save workbook: {}", e)))?;

    let now = crate::utils::time::format_export_timestamp();
    let file_name = format!("audit-logs-{}.xlsx", now);

    Ok(http_utils::binary_response(
        StatusCode::OK,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &file_name,
        buffer,
    ))
}
