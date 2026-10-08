//! Credit REST endpoints
//! POST /api/credit/blacklist/add|remove|check
//! GET  /api/credit/blacklist/list
//! POST /api/credit/violations/record|appeal|review
//! GET  /api/credit/violations/list|get
//! GET  /api/credit/score
//! GET  /api/credit/score/history
//! POST /api/credit/score/recalculate
//! POST /api/credit/check-order
//!
//! Permissions: blacklist_add/remove=AdminUser, violation_review=AdminUser, credit_recalculate=AdminUser
//!              All others=AuthUser

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn credit_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Blacklist
        .route("/api/credit/blacklist/add", post(blacklist_add))
        .route("/api/credit/blacklist/remove", post(blacklist_remove))
        .route("/api/credit/blacklist/check", post(blacklist_check))
        .route("/api/credit/blacklist/list", get(blacklist_list))
        // Violation
        .route("/api/credit/violations/record", post(violation_record))
        .route("/api/credit/violations/appeal", post(violation_appeal))
        .route("/api/credit/violations/review", post(violation_review))
        .route("/api/credit/violations/list", get(violation_list))
        .route("/api/credit/violations/get", get(violation_get))
        // Credit
        .route("/api/credit/score", get(credit_get))
        .route("/api/credit/score/history", get(credit_history))
        .route("/api/credit/score/recalculate", post(credit_recalculate))
        // Pre-order check
        .route("/api/credit/check-order", post(check_before_order))
}

// ── Blacklist ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlacklistAddBody {
    customer_name: String,
    customer_phone: String,
    id_number: Option<String>,
    reason: String,
    severity: String,
}

async fn blacklist_add(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<BlacklistAddBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "blacklist_add",
            serde_json::json!({
                "customerName": b.customer_name,
                "customerPhone": b.customer_phone,
                "idNumber": b.id_number,
                "reason": b.reason,
                "severity": b.severity,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlacklistRemoveBody {
    id: i64,
    #[serde(rename = "removalReason")]
    removal_reason: String,
}

async fn blacklist_remove(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<BlacklistRemoveBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "blacklist_remove",
            serde_json::json!({
                "id": b.id,
                "removalReason": b.removal_reason,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlacklistCheckBody {
    customer_name: String,
    customer_phone: String,
}

async fn blacklist_check(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<BlacklistCheckBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "blacklist_check",
            serde_json::json!({
                "customerName": b.customer_name,
                "customerPhone": b.customer_phone,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlacklistListQuery {
    #[serde(rename = "isActive")]
    is_active: Option<bool>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
}

async fn blacklist_list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<BlacklistListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "blacklist_list",
            serde_json::json!({
                "isActive": q.is_active,
                "page": q.page,
                "pageSize": q.page_size,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Violation ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViolationRecordBody {
    customer_name: String,
    customer_phone: String,
    order_id: Option<i64>,
    violation_type: String,
    severity: String,
    description: String,
    evidence: Option<String>,
    financial_penalty: Option<f64>,
}

async fn violation_record(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ViolationRecordBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "violation_record",
            serde_json::json!({
                "customerName": b.customer_name,
                "customerPhone": b.customer_phone,
                "orderId": b.order_id,
                "violationType": b.violation_type,
                "severity": b.severity,
                "description": b.description,
                "evidence": b.evidence,
                "financialPenalty": b.financial_penalty,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViolationAppealBody {
    id: i64,
    #[serde(rename = "appealReason")]
    appeal_reason: String,
}

async fn violation_appeal(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ViolationAppealBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "violation_appeal",
            serde_json::json!({
                "id": b.id,
                "appealReason": b.appeal_reason,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViolationReviewBody {
    id: i64,
    status: String,
    #[serde(rename = "reviewNotes")]
    review_notes: String,
}

async fn violation_review(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<ViolationReviewBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "violation_review",
            serde_json::json!({
                "id": b.id,
                "status": b.status,
                "reviewNotes": b.review_notes,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViolationListQuery {
    #[serde(rename = "customerPhone")]
    customer_phone: Option<String>,
    status: Option<String>,
    #[serde(rename = "violationType")]
    violation_type: Option<String>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
}

async fn violation_list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ViolationListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "violation_list",
            serde_json::json!({
                "customerPhone": q.customer_phone,
                "status": q.status,
                "violationType": q.violation_type,
                "page": q.page,
                "pageSize": q.page_size,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViolationGetQuery {
    id: i64,
}

async fn violation_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ViolationGetQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "violation_get",
            serde_json::json!({
                "id": q.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Credit ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditGetQuery {
    #[serde(rename = "customerPhone")]
    customer_phone: String,
}

async fn credit_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<CreditGetQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "credit_get",
            serde_json::json!({
                "customerPhone": q.customer_phone,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditHistoryQuery {
    #[serde(rename = "customerPhone")]
    customer_phone: String,
}

async fn credit_history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<CreditHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "credit_history",
            serde_json::json!({
                "customerPhone": q.customer_phone,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditRecalculateBody {
    #[serde(rename = "customerPhone")]
    customer_phone: String,
}

async fn credit_recalculate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<CreditRecalculateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "credit_recalculate",
            serde_json::json!({
                "customerPhone": b.customer_phone,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Pre-order check ────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckBeforeOrderBody {
    customer_name: String,
    customer_phone: String,
}

async fn check_before_order(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<CheckBeforeOrderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "credit",
            "check_before_order",
            serde_json::json!({
                "customerName": b.customer_name,
                "customerPhone": b.customer_phone,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
