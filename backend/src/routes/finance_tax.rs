//! 发票 + 结算 + 税务 REST 端点
//! 读取：AuthUser；资金变动类：AdminUser
//! POST /api/finance/invoice/*
//! POST /api/finance/settlement/*
//! POST /api/finance/tax/*

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::AdminUser;
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn finance_tax_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Invoice routes
        .route("/api/finance/invoice/issue", post(invoice_issue))
        .route("/api/finance/invoice/void", post(invoice_void))
        .route("/api/finance/invoice/red_flush", post(invoice_red_flush))
        .route("/api/finance/invoice/get", post(invoice_get))
        .route("/api/finance/invoice/list", post(invoice_list))
        // Settlement routes
        .route(
            "/api/finance/settlement/generate",
            post(settlement_generate),
        )
        .route("/api/finance/settlement/confirm", post(settlement_confirm))
        .route("/api/finance/settlement/get", post(settlement_get))
        .route("/api/finance/settlement/list", post(settlement_list))
        .route("/api/finance/settlement/export", post(settlement_export))
        // Tax routes
        .route("/api/finance/tax/config", post(tax_config))
        .route("/api/finance/tax/upsert_config", post(tax_upsert_config))
        .route("/api/finance/tax/calculate", post(tax_calculate))
        .route("/api/finance/tax/export", post(tax_export))
}

// ══════════════════════════════════════════════════════════════════
// Invoice — 读取端点 (AuthUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvoiceGetBody {
    invoice_id: Option<String>,
    order_id: Option<String>,
}

async fn invoice_get(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<InvoiceGetBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "invoiceId": body.invoice_id,
        "orderId": body.order_id,
    });
    let result = state
        .registry
        .execute("invoice", "get", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvoiceListBody {
    status: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn invoice_list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<InvoiceListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "status": body.status,
        "dateFrom": body.date_from,
        "dateTo": body.date_to,
        "page": body.page,
        "pageSize": body.page_size,
    });
    let result = state
        .registry
        .execute("invoice", "list", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// Invoice — 资金变动端点 (AdminUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvoiceIssueBody {
    order_id: String,
    amount: f64,
    #[serde(default)]
    invoice_type: Option<String>,
    #[serde(default)]
    tax_rate: Option<f64>,
}

async fn invoice_issue(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<InvoiceIssueBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "orderId": body.order_id,
        "amount": body.amount,
        "invoiceType": body.invoice_type.unwrap_or_else(|| "普通发票".to_string()),
        "taxRate": body.tax_rate,
    });
    let result = state
        .registry
        .execute("invoice", "issue", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvoiceVoidBody {
    invoice_id: String,
}

async fn invoice_void(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<InvoiceVoidBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"invoiceId": body.invoice_id});
    let result = state
        .registry
        .execute("invoice", "void", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvoiceRedFlushBody {
    invoice_id: String,
    reason: Option<String>,
}

async fn invoice_red_flush(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<InvoiceRedFlushBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "invoiceId": body.invoice_id,
        "reason": body.reason.unwrap_or_default(),
    });
    let result = state
        .registry
        .execute("invoice", "red_flush", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// Settlement — 读取端点 (AuthUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementGetBody {
    period_type: String,
    period_key: String,
}

async fn settlement_get(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<SettlementGetBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "periodType": body.period_type,
        "periodKey": body.period_key,
    });
    let result = state
        .registry
        .execute("settlement", "get", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementListBody {
    period_type: Option<String>,
    confirmed: Option<bool>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn settlement_list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<SettlementListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "periodType": body.period_type,
        "confirmed": body.confirmed,
        "page": body.page,
        "pageSize": body.page_size,
    });
    let result = state
        .registry
        .execute("settlement", "list", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementExportBody {
    settlement_id: String,
}

async fn settlement_export(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<SettlementExportBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"settlementId": body.settlement_id});
    let result = state
        .registry
        .execute("settlement", "export", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// Settlement — 资金变动端点 (AdminUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementGenerateBody {
    period_type: String,
    #[serde(default)]
    period_key: Option<String>,
}

async fn settlement_generate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<SettlementGenerateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "periodType": body.period_type,
        "periodKey": body.period_key.unwrap_or_default(),
    });
    let result = state
        .registry
        .execute("settlement", "generate", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementConfirmBody {
    settlement_id: String,
}

async fn settlement_confirm(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<SettlementConfirmBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"settlementId": body.settlement_id});
    let result = state
        .registry
        .execute("settlement", "confirm", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// Tax — 读取端点 (AuthUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaxConfigBody {
    #[serde(default)]
    tax_type: Option<String>,
}

async fn tax_config(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<TaxConfigBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"taxType": body.tax_type});
    let result = state
        .registry
        .execute("tax", "get_config", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaxCalculateBody {
    amount: f64,
    #[serde(default)]
    tax_rate: Option<f64>,
}

async fn tax_calculate(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<TaxCalculateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "amount": body.amount,
        "taxRate": body.tax_rate,
    });
    let result = state
        .registry
        .execute("tax", "calculate", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaxExportBody {
    #[serde(default)]
    period_key: Option<String>,
}

async fn tax_export(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<TaxExportBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({"periodKey": body.period_key.unwrap_or_default()});
    let result = state
        .registry
        .execute("tax", "export", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════════
// Tax — 管理端点 (AdminUser)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaxUpsertConfigBody {
    #[serde(default)]
    tax_type: Option<String>,
    rate: f64,
    #[serde(default)]
    effective_from: Option<String>,
}

async fn tax_upsert_config(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<TaxUpsertConfigBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let payload = serde_json::json!({
        "taxType": body.tax_type.unwrap_or_else(|| "vat".to_string()),
        "rate": body.rate,
        "effectiveFrom": body.effective_from.unwrap_or_default(),
    });
    let result = state
        .registry
        .execute("tax", "upsert_config", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}
