//! Contract REST endpoints
//! POST /api/contract/template/create|update
//! GET  /api/contract/template/list|get
//! POST /api/contract/generate|sign|verify
//! GET  /api/contract/list|get
//!
//! Permissions: template_create/update=AdminUser, all others=AuthUser

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn contract_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Templates
        .route("/api/contract/template/create", post(template_create))
        .route("/api/contract/template/update", post(template_update))
        .route("/api/contract/template/list", get(template_list))
        .route("/api/contract/template/get", get(template_get))
        // Contracts
        .route("/api/contract/generate", post(contract_generate))
        .route("/api/contract/sign", post(contract_sign))
        .route("/api/contract/verify", post(contract_verify))
        .route("/api/contract/list", get(contract_list))
        .route("/api/contract/get", get(contract_get))
}

// ── Templates ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateCreateBody {
    name: String,
    content_json: serde_json::Value,
}

async fn template_create(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<TemplateCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "template_create",
            serde_json::json!({
                "name": b.name,
                "contentJson": b.content_json,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateUpdateBody {
    id: i64,
    name: Option<String>,
    content_json: Option<serde_json::Value>,
    #[serde(rename = "isActive")]
    is_active: Option<bool>,
}

async fn template_update(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<TemplateUpdateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "template_update",
            serde_json::json!({
                "id": b.id,
                "name": b.name,
                "contentJson": b.content_json,
                "isActive": b.is_active,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateListQuery {
    is_active: Option<bool>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
}

async fn template_list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<TemplateListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "template_list",
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateGetQuery {
    id: i64,
}

async fn template_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<TemplateGetQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "template_get",
            serde_json::json!({
                "id": q.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

// ── Contracts ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractGenerateBody {
    order_id: String,
    template_id: Option<i64>,
    customer_name: String,
    customer_phone: String,
    device_value: f64,
    variables: Option<serde_json::Value>,
}

async fn contract_generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ContractGenerateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "contract_generate",
            serde_json::json!({
                "orderId": b.order_id,
                "templateId": b.template_id,
                "customerName": b.customer_name,
                "customerPhone": b.customer_phone,
                "deviceValue": b.device_value,
                "variables": b.variables,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractSignBody {
    id: i64,
    signer_name: String,
    signer_phone: String,
    signature_data: String,
}

async fn contract_sign(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ContractSignBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "contract_sign",
            serde_json::json!({
                "id": b.id,
                "signerName": b.signer_name,
                "signerPhone": b.signer_phone,
                "signatureData": b.signature_data,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractVerifyBody {
    id: i64,
}

async fn contract_verify(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ContractVerifyBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "contract_verify",
            serde_json::json!({
                "id": b.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractListQuery {
    order_id: Option<String>,
    customer_phone: Option<String>,
    status: Option<String>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
}

async fn contract_list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ContractListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "contract_list",
            serde_json::json!({
                "orderId": q.order_id,
                "customerPhone": q.customer_phone,
                "status": q.status,
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
struct ContractGetQuery {
    id: i64,
}

async fn contract_get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ContractGetQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "contract",
            "contract_get",
            serde_json::json!({
                "id": q.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
