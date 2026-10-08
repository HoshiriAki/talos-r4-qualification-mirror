//! Optical SOP inspection REST endpoints
//! POST /api/optical-sop/inspections/create
//! POST /api/optical-sop/inspections/update-step
//! POST /api/optical-sop/inspections/complete
//! GET  /api/optical-sop/inspections/get
//! GET  /api/optical-sop/inspections/list
//! GET  /api/optical-sop/inspections/stats
//!
//! Permissions: mutations=AdminUser, queries=AuthUser

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn optical_sop_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/optical-sop/inspections/create",
            post(inspection_create),
        )
        .route(
            "/api/optical-sop/inspections/update-step",
            post(inspection_update_step),
        )
        .route(
            "/api/optical-sop/inspections/complete",
            post(inspection_complete),
        )
        .route("/api/optical-sop/inspections/get", get(inspection_get))
        .route("/api/optical-sop/inspections/list", get(inspection_list))
        .route("/api/optical-sop/inspections/stats", get(inspection_stats))
}

// ── Create ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionCreateBody {
    #[serde(alias = "order_id")]
    order_id: String,
    #[serde(alias = "device_serial_no")]
    device_serial_no: String,
}

async fn inspection_create(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<InspectionCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "optical_sop",
            "inspection_create",
            serde_json::json!({
                "orderId": b.order_id,
                "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Update Step ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionUpdateStepBody {
    #[serde(alias = "inspection_id", alias = "inspectionId")]
    id: i64,
    step: String,
    #[serde(alias = "passed")]
    ok: bool,
    note: Option<String>,
}

async fn inspection_update_step(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<InspectionUpdateStepBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "optical_sop",
            "inspection_update_step",
            serde_json::json!({
                "id": b.id,
                "step": b.step,
                "ok": b.ok,
                "note": b.note,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Complete ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionCompleteBody {
    #[serde(alias = "inspection_id", alias = "inspectionId")]
    id: i64,
}

async fn inspection_complete(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<InspectionCompleteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "optical_sop",
            "inspection_complete",
            serde_json::json!({
                "id": b.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Get ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionGetQuery {
    #[serde(alias = "inspection_id", alias = "inspectionId")]
    id: i64,
}

async fn inspection_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<InspectionGetQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "optical_sop",
            "inspection_get",
            serde_json::json!({
                "id": q.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── List ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionListQuery {
    #[serde(rename = "orderId", alias = "order_id")]
    order_id: Option<String>,
    #[serde(rename = "deviceSerialNo", alias = "device_serial_no")]
    device_serial_no: Option<String>,
    #[serde(rename = "overallGrade", alias = "overall_grade")]
    overall_grade: Option<String>,
    page: Option<i64>,
    #[serde(rename = "pageSize", alias = "page_size")]
    page_size: Option<i64>,
}

async fn inspection_list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<InspectionListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "optical_sop",
            "inspection_list",
            serde_json::json!({
                "orderId": q.order_id,
                "deviceSerialNo": q.device_serial_no,
                "overallGrade": q.overall_grade,
                "page": q.page,
                "pageSize": q.page_size,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Stats ──────────────────────────────────────────────────────────────────

async fn inspection_stats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "optical_sop",
            "inspection_stats",
            serde_json::json!({}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
