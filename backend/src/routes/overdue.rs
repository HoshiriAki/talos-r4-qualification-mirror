//! Overdue REST endpoints
//! POST /api/overdue/detect
//! POST /api/overdue/calc
//! POST /api/overdue/apply
//! POST /api/overdue/waive
//! GET  /api/overdue/list
//! GET  /api/overdue/get
//! GET  /api/overdue/config
//! PUT  /api/overdue/config
//! POST /api/overdue/escalate
//! GET  /api/overdue/escalation-history
//! GET  /api/overdue/stats
//! POST /api/overdue/check-order
//!
//! Permissions: detect/escalate=AdminUser, waive/config_upsert=AdminUser
//!              All others=AuthUser

use axum::extract::{Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn overdue_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/overdue/detect", post(overdue_detect))
        .route("/api/overdue/calc", post(overdue_calc))
        .route("/api/overdue/apply", post(overdue_apply))
        .route("/api/overdue/waive", post(overdue_waive))
        .route("/api/overdue/list", get(overdue_list))
        .route("/api/overdue/get", get(overdue_get))
        .route("/api/overdue/config", get(overdue_config_get))
        .route("/api/overdue/config", put(overdue_config_upsert))
        .route("/api/overdue/escalate", post(overdue_escalate))
        .route(
            "/api/overdue/escalation-history",
            get(overdue_escalation_history),
        )
        .route("/api/overdue/stats", get(overdue_stats))
        .route("/api/overdue/check-order", post(overdue_check_before_order))
}

// ── Detect ───────────────────────────────────────────────────────────────

async fn overdue_detect(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute("overdue", "overdue_detect", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Calc ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueCalcBody {
    order_id: i64,
}

async fn overdue_calc(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<OverdueCalcBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_calc",
            serde_json::json!({
                "orderId": b.order_id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Apply ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueApplyBody {
    overdue_id: i64,
}

async fn overdue_apply(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<OverdueApplyBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_apply",
            serde_json::json!({
                "overdueId": b.overdue_id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Waive ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueWaiveBody {
    overdue_id: i64,
    #[serde(rename = "waivedReason")]
    waived_reason: String,
}

async fn overdue_waive(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<OverdueWaiveBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_waive",
            serde_json::json!({
                "overdueId": b.overdue_id,
                "waivedReason": b.waived_reason,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── List ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueListQuery {
    status: Option<String>,
    #[serde(rename = "customerPhone")]
    customer_phone: Option<String>,
    #[serde(rename = "orderId")]
    order_id: Option<i64>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
}

async fn overdue_list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<OverdueListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_list",
            serde_json::json!({
                "status": q.status,
                "customerPhone": q.customer_phone,
                "orderId": q.order_id,
                "page": q.page,
                "pageSize": q.page_size,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Get ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueGetQuery {
    #[serde(rename = "overdueId")]
    overdue_id: i64,
}

async fn overdue_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<OverdueGetQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_get",
            serde_json::json!({
                "overdueId": q.overdue_id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Config Get ───────────────────────────────────────────────────────────

async fn overdue_config_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute("overdue", "overdue_config_get", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Config Upsert ────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueConfigUpsertBody {
    daily_rate: Option<f64>,
    max_days: Option<i64>,
    cap_multiplier: Option<f64>,
    grace_period_hours: Option<i64>,
}

async fn overdue_config_upsert(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<OverdueConfigUpsertBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_config_upsert",
            serde_json::json!({
                "dailyRate": b.daily_rate,
                "maxDays": b.max_days,
                "capMultiplier": b.cap_multiplier,
                "gracePeriodHours": b.grace_period_hours,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Escalate ─────────────────────────────────────────────────────────────

async fn overdue_escalate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute("overdue", "overdue_escalate", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Escalation History ───────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueEscalationHistoryQuery {
    #[serde(rename = "overdueId")]
    overdue_id: i64,
}

async fn overdue_escalation_history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<OverdueEscalationHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_escalation_history",
            serde_json::json!({
                "overdueId": q.overdue_id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Stats ────────────────────────────────────────────────────────────────

async fn overdue_stats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute("overdue", "overdue_stats", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Check Before Order ───────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverdueCheckBeforeOrderBody {
    #[serde(rename = "customerPhone")]
    customer_phone: String,
}

async fn overdue_check_before_order(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<OverdueCheckBeforeOrderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "overdue",
            "overdue_check_before_order",
            serde_json::json!({
                "customerPhone": b.customer_phone,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
