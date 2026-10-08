//! 设备损坏 + 维修 REST 端点
//! POST /api/damage/report|assess|adjudicate|get|list
//! POST /api/repair/create|start|complete|return|get|list

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn damage_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/damage/report", post(damage_report))
        .route("/api/damage/assess", post(damage_assess))
        .route("/api/damage/adjudicate", post(damage_adjudicate))
        .route("/api/damage/get", post(damage_get))
        .route("/api/damage/list", post(damage_list))
        .route("/api/repair/create", post(repair_create))
        .route("/api/repair/start", post(repair_start))
        .route("/api/repair/complete", post(repair_complete))
        .route("/api/repair/return", post(repair_return))
        .route("/api/repair/get", post(repair_get))
        .route("/api/repair/list", post(repair_list))
}

// ── Damage (read: AuthUser; write: AdminUser) ───────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportBody {
    order_id: String,
    device_serial_no: String,
    appearance_ok: bool,
    accessories_ok: bool,
    function_ok: bool,
    damage_description: Option<String>,
}

async fn damage_report(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<ReportBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state.registry.execute("damage", "report", serde_json::json!({
        "orderId": b.order_id, "deviceSerialNo": b.device_serial_no,
        "appearanceOk": b.appearance_ok, "accessoriesOk": b.accessories_ok,
        "functionOk": b.function_ok, "damageDescription": b.damage_description.unwrap_or_default(),
    }), &ctx).map(Json).map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssessBody {
    damage_id: String,
    estimated_damage_amount: f64,
    liability: String,
    notes: Option<String>,
}

async fn damage_assess(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<AssessBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "damage",
            "assess",
            serde_json::json!({
                "damageId": b.damage_id, "estimatedDamageAmount": b.estimated_damage_amount,
                "liability": b.liability, "notes": b.notes.unwrap_or_default(),
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdjudicateBody {
    damage_id: String,
}

async fn damage_adjudicate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<AdjudicateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "damage",
            "adjudicate",
            serde_json::json!({"damageId": b.damage_id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetDamageBody {
    order_id: Option<String>,
    device_serial_no: Option<String>,
}

async fn damage_get(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<GetDamageBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "damage",
            "get",
            serde_json::json!({
                "orderId": b.order_id, "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DamageListBody {
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn damage_list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<DamageListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "damage",
            "list",
            serde_json::json!({
                "status": b.status, "page": b.page, "pageSize": b.page_size,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Repair (all mutations: AdminUser) ───────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRepairBody {
    damage_report_id: String,
    repair_description: Option<String>,
    vendor: Option<String>,
}

async fn repair_create(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<CreateRepairBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "repair",
            "create_repair_order",
            serde_json::json!({
                "damageReportId": b.damage_report_id,
                "repairDescription": b.repair_description.unwrap_or_default(),
                "vendor": b.vendor.unwrap_or_default(),
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRepairBody {
    repair_id: String,
    repair_cost: Option<f64>,
}

async fn repair_start(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<UpdateRepairBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "repair",
            "start_repair",
            serde_json::json!({"repairId": b.repair_id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

async fn repair_complete(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<UpdateRepairBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "repair",
            "complete_repair",
            serde_json::json!({
                "repairId": b.repair_id, "repairCost": b.repair_cost,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

async fn repair_return(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<UpdateRepairBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "repair",
            "return_to_stock",
            serde_json::json!({"repairId": b.repair_id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetRepairBody {
    repair_id: String,
}

async fn repair_get(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<GetRepairBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "repair",
            "get",
            serde_json::json!({"repairId": b.repair_id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepairListBody {
    status: Option<String>,
    device_serial_no: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn repair_list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<RepairListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state.registry.execute("repair", "list", serde_json::json!({
        "status": b.status, "deviceSerialNo": b.device_serial_no, "page": b.page, "pageSize": b.page_size,
    }), &ctx).map(Json).map_err(AppError::from_error_payload)?;
    Ok(r)
}
