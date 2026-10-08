//! Barcode REST endpoints
//! POST /api/barcode/generate
//! POST /api/barcode/batch-generate
//! POST /api/barcode/lookup
//! POST /api/barcode/scan
//! GET  /api/barcode/scan-history
//! GET  /api/barcode/scan-stats
//!
//! Permissions: generate/batch-generate=AdminUser, all others=AuthUser

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn barcode_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Admin-only (generate)
        .route("/api/barcode/generate", post(barcode_generate))
        .route("/api/barcode/batch-generate", post(barcode_batch_generate))
        // Auth
        .route("/api/barcode/lookup", post(barcode_lookup))
        .route("/api/barcode/scan", post(scan_event))
        .route("/api/barcode/scan-history", get(scan_history))
        .route("/api/barcode/scan-stats", get(scan_stats))
}

// ── Generate ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BarcodeGenerateBody {
    device_serial_no: String,
}

async fn barcode_generate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<BarcodeGenerateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "barcode",
            "generate",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Batch Generate ──────────────────────────────────────────────────────

async fn barcode_batch_generate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute("barcode", "batch_generate", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Lookup ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BarcodeLookupBody {
    barcode_text: String,
}

async fn barcode_lookup(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<BarcodeLookupBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "barcode",
            "lookup",
            serde_json::json!({
                "barcodeText": b.barcode_text,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Scan Event ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanEventBody {
    device_serial_no: String,
    barcode_text: Option<String>,
    scan_type: String,
    warehouse_id: Option<String>,
    notes: Option<String>,
}

async fn scan_event(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ScanEventBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "barcode",
            "scan_event",
            serde_json::json!({
                "deviceSerialNo": b.device_serial_no,
                "barcodeText": b.barcode_text,
                "scanType": b.scan_type,
                "warehouseId": b.warehouse_id,
                "notes": b.notes,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Scan History ────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanHistoryQuery {
    #[serde(rename = "deviceSerialNo")]
    device_serial_no: Option<String>,
    #[serde(rename = "scanType")]
    scan_type: Option<String>,
    #[serde(rename = "startDate")]
    start_date: Option<String>,
    #[serde(rename = "endDate")]
    end_date: Option<String>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
}

async fn scan_history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ScanHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "barcode",
            "scan_history",
            serde_json::json!({
                "deviceSerialNo": q.device_serial_no,
                "scanType": q.scan_type,
                "startDate": q.start_date,
                "endDate": q.end_date,
                "page": q.page,
                "pageSize": q.page_size,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Scan Stats ──────────────────────────────────────────────────────────

async fn scan_stats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute("barcode", "scan_stats", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
