//! 押金 + 退款 REST 端点
//! 读取：AuthUser；资金变动：AdminUser
//! POST /api/finance/deposit/calculate|collect|release|forfeit|get
//! POST /api/finance/refund/request|approve|reject|execute|list

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn finance_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/finance/deposit/calculate", post(deposit_calculate))
        .route("/api/finance/deposit/collect", post(deposit_collect))
        .route("/api/finance/deposit/release", post(deposit_release))
        .route("/api/finance/deposit/forfeit", post(deposit_forfeit))
        .route("/api/finance/deposit/get", post(deposit_get))
        .route("/api/finance/refund/request", post(refund_request))
        .route("/api/finance/refund/approve", post(refund_approve))
        .route("/api/finance/refund/reject", post(refund_reject))
        .route("/api/finance/refund/execute", post(refund_execute))
        .route("/api/finance/refund/list", post(refund_list))
}

// ══════════════════════════════════════════════════════════════════
// 读取端点 (AuthUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalculateBody {
    order_id: String,
}

async fn deposit_calculate(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<CalculateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let payload = serde_json::json!({"orderId": body.order_id});
    let result = state
        .registry
        .execute("deposit", "calculate", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetDepositBody {
    order_id: String,
}

async fn deposit_get(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<GetDepositBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let payload = serde_json::json!({"orderId": body.order_id});
    let result = state
        .registry
        .execute("deposit", "get", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefundListBody {
    order_id: Option<String>,
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn refund_list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<RefundListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let payload = serde_json::json!({
        "orderId": body.order_id,
        "status": body.status,
        "page": body.page,
        "pageSize": body.page_size,
    });
    let result = state
        .registry
        .execute("refund", "list", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// 资金变动端点 (AdminUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectBody {
    order_id: String,
    amount: f64,
}

async fn deposit_collect(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<CollectBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"orderId": body.order_id, "amount": body.amount});
    let result = state
        .registry
        .execute("deposit", "collect", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseBody {
    order_id: String,
    reason: Option<String>,
}

async fn deposit_release(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<ReleaseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload =
        serde_json::json!({"orderId": body.order_id, "reason": body.reason.unwrap_or_default()});
    let result = state
        .registry
        .execute("deposit", "release", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForfeitBody {
    order_id: String,
    amount: f64,
    reason: Option<String>,
}

async fn deposit_forfeit(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<ForfeitBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"orderId": body.order_id, "amount": body.amount, "reason": body.reason.unwrap_or_default()});
    let result = state
        .registry
        .execute("deposit", "forfeit", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefundRequestBody {
    order_id: String,
    amount: f64,
    reason: Option<String>,
}

async fn refund_request(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<RefundRequestBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"orderId": body.order_id, "amount": body.amount, "reason": body.reason.unwrap_or_default()});
    let result = state
        .registry
        .execute("refund", "request", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApproveBody {
    refund_id: String,
}

async fn refund_approve(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<ApproveBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"refundId": body.refund_id});
    let result = state
        .registry
        .execute("refund", "approve", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RejectBody {
    refund_id: String,
    reason: Option<String>,
}

async fn refund_reject(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<RejectBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload =
        serde_json::json!({"refundId": body.refund_id, "reason": body.reason.unwrap_or_default()});
    let result = state
        .registry
        .execute("refund", "reject", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteBody {
    refund_id: String,
}

async fn refund_execute(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<ExecuteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"refundId": body.refund_id});
    let result = state
        .registry
        .execute("refund", "execute", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}
