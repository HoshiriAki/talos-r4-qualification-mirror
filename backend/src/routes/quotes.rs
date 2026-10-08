use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::tenant_extractors::{TrustedTenantAdmin, TrustedTenantUser};
use crate::state::AppState;

pub fn quote_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v2/quotes", get(list_quotes).post(create_quote))
        .route("/api/v2/quotes/{id}", get(get_quote))
        .route("/api/v2/quotes/{id}/confirm", post(confirm_quote))
        .route("/api/v2/quotes/{id}/expire", post(expire_quote))
        .route("/api/v2/quotes/{id}/orders", post(create_order_from_quote))
        .route("/api/v2/accessories/{id}", put(upsert_accessory))
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct LimitQuery {
    limit: Option<u32>,
}

async fn list_quotes(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Query(query): Query<LimitQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "quote",
            "list_quotes",
            serde_json::json!({"limit": query.limit.unwrap_or(100)}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn get_quote(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "quote",
            "get_quote",
            serde_json::json!({"quoteId": id}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn create_quote(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute("quote", "create_quote", body, tenant_user.context())
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn confirm_quote(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "quote",
            "confirm_quote",
            serde_json::json!({"quoteId": id}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn expire_quote(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "quote",
            "expire_quote",
            serde_json::json!({"quoteId": id}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn create_order_from_quote(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let remark = body
        .get("remark")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    state
        .registry
        .execute(
            "quote",
            "create_order_from_quote",
            serde_json::json!({"quoteId": id, "remark": remark}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn upsert_accessory(
    State(state): State<Arc<AppState>>,
    tenant_admin: TrustedTenantAdmin,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut payload = body.as_object().cloned().unwrap_or_default();
    payload.insert("id".into(), serde_json::Value::String(id));
    state
        .registry
        .execute(
            "quote",
            "upsert_accessory",
            serde_json::Value::Object(payload),
            tenant_admin.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}
