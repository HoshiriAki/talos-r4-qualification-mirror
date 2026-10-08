//! 设备全生命周期管理 REST 端点
//! POST /api/assets/procurement/record_purchase|set_value|get
//! POST /api/assets/depreciation/calculate|run_monthly|get
//! POST /api/assets/roa/calculate|list
//! POST /api/assets/repair/stats (repair aggregate)
//!
//! 权限: write=AdminUser, read=AuthUser

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn assets_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Procurement (write: AdminUser, read: AuthUser)
        .route(
            "/api/assets/procurement/record_purchase",
            post(procurement_record),
        )
        .route(
            "/api/assets/procurement/set_value",
            post(procurement_set_value),
        )
        .route("/api/assets/procurement/get", post(procurement_get))
        // Depreciation (write: AdminUser, read: AuthUser)
        .route(
            "/api/assets/depreciation/calculate",
            post(depreciation_calculate),
        )
        .route(
            "/api/assets/depreciation/run_monthly",
            post(depreciation_run_monthly),
        )
        .route("/api/assets/depreciation/get", post(depreciation_get))
        // ROA (read: AuthUser)
        .route("/api/assets/roa/calculate", post(roa_calculate))
        .route("/api/assets/roa/list", post(roa_list))
        // Repair aggregate (read: AuthUser)
        .route("/api/assets/repair/stats", post(repair_stats))
}

// ── Procurement ───────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordPurchaseBody {
    device_serial_no: String,
    purchase_price: f64,
    purchase_date: Option<String>,
    vendor: Option<String>,
    invoice_no: Option<String>,
    replacement_value: Option<f64>,
    notes: Option<String>,
}

async fn procurement_record(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<RecordPurchaseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "procurement",
            "record_purchase",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
                "purchasePrice": b.purchase_price,
                "purchaseDate": b.purchase_date.unwrap_or_default(),
                "vendor": b.vendor.unwrap_or_default(),
                "invoiceNo": b.invoice_no.unwrap_or_default(),
                "replacementValue": b.replacement_value.unwrap_or(0.0),
                "notes": b.notes.unwrap_or_default(),
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetValueBody {
    device_serial_no: String,
    replacement_value: f64,
}

async fn procurement_set_value(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<SetValueBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "procurement",
            "set_replacement_value",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no, "replacementValue": b.replacement_value,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetProcurementBody {
    device_serial_no: String,
}

async fn procurement_get(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<GetProcurementBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "procurement",
            "get",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Depreciation ──────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalcDepreciationBody {
    device_serial_no: String,
    useful_life_months: Option<i32>,
}

async fn depreciation_calculate(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<CalcDepreciationBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "depreciation",
            "calculate",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no, "usefulLifeMonths": b.useful_life_months,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunMonthlyBody {
    useful_life_months: Option<i32>,
}

async fn depreciation_run_monthly(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<RunMonthlyBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "depreciation",
            "run_monthly",
            serde_json::json!({
                "usefulLifeMonths": b.useful_life_months,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetDepreciationBody {
    device_serial_no: String,
}

async fn depreciation_get(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<GetDepreciationBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "depreciation",
            "get",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── ROA ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalculateRoaBody {
    device_serial_no: String,
}

async fn roa_calculate(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<CalculateRoaBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "roa",
            "calculate",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

async fn roa_list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(_b): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute("roa", "list", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Repair Aggregate (read: AuthUser) ──────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepairStatsBody {
    device_serial_no: String,
}

async fn repair_stats(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(b): Json<RepairStatsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "repair",
            "get_device_stats",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
