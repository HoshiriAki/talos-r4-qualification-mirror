//! Work task REST endpoints

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::tenant_extractors::TrustedTenantUser;
use crate::state::AppState;

pub fn tasks_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks/{id}/actions", post(task_action))
        .route("/api/tasks/{id}/send-to-pc", post(send_to_pc))
}

#[derive(Deserialize)]
struct TaskQueryParams {
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskActionBody {
    action: String,
    #[serde(default)]
    expected_version: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendToPcBody {
    expected_version: i64,
}

async fn list_tasks(
    tenant_user: TrustedTenantUser,
    State(state): State<Arc<AppState>>,
    Query(query): Query<TaskQueryParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = state
        .registry
        .execute(
            "work_task",
            "list_tasks",
            serde_json::json!({
                "status": query.status,
                "limit": query.limit,
            }),
            tenant_user.context(),
        )
        .map_err(AppError::from_error_payload)?;
    Ok(Json(result))
}

async fn task_action(
    tenant_user: TrustedTenantUser,
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(body): Json<TaskActionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = state
        .registry
        .execute(
            "work_task",
            "task_action",
            serde_json::json!({
                "id": task_id,
                "action": body.action,
                "expectedVersion": body.expected_version,
            }),
            tenant_user.context(),
        )
        .map_err(AppError::from_error_payload)?;
    Ok(Json(result))
}

async fn send_to_pc(
    tenant_user: TrustedTenantUser,
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(body): Json<SendToPcBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = state
        .registry
        .execute(
            "work_task",
            "send_to_pc",
            serde_json::json!({
                "id": task_id,
                "expectedVersion": body.expected_version,
            }),
            tenant_user.context(),
        )
        .map_err(AppError::from_error_payload)?;
    Ok(Json(result))
}
