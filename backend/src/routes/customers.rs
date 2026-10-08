use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::tenant_extractors::{TrustedTenantAdmin, TrustedTenantUser};
use crate::state::AppState;

pub fn customer_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v2/customers",
            get(list_customers).post(create_customer),
        )
        .route(
            "/api/v2/customers/duplicate-candidates",
            post(find_duplicate_candidates),
        )
        .route(
            "/api/v2/customers/migration-exceptions",
            get(list_migration_exceptions),
        )
        .route(
            "/api/v2/customers/migration-exceptions/{id}/resolve",
            post(resolve_migration_exception),
        )
        .route(
            "/api/v2/customers/{id}",
            get(get_customer).delete(anonymize_customer),
        )
        .route("/api/v2/customers/{id}/contacts", post(add_contact))
        .route(
            "/api/v2/customers/{id}/external-identities",
            post(add_external_identity),
        )
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct LimitQuery {
    limit: Option<u32>,
}

async fn list_customers(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Query(query): Query<LimitQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "customer",
            "list_customers",
            serde_json::json!({"limit": query.limit.unwrap_or(100)}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn get_customer(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "customer",
            "get_customer",
            serde_json::json!({"customerId": id}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn create_customer(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute("customer", "create_customer", body, tenant_user.context())
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn add_contact(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "customer",
            "add_contact",
            serde_json::json!({"customerId": id, "contact": body}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn add_external_identity(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let provider = body
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let external_subject = body
        .get("externalSubject")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    state
        .registry
        .execute(
            "customer",
            "add_external_identity",
            serde_json::json!({
                "customerId": id,
                "provider": provider,
                "externalSubject": external_subject
            }),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn find_duplicate_candidates(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "customer",
            "find_duplicate_candidates",
            body,
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn list_migration_exceptions(
    State(state): State<Arc<AppState>>,
    tenant_admin: TrustedTenantAdmin,
    Query(query): Query<LimitQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "customer",
            "list_migration_exceptions",
            serde_json::json!({"limit": query.limit.unwrap_or(100)}),
            tenant_admin.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn resolve_migration_exception(
    State(state): State<Arc<AppState>>,
    tenant_admin: TrustedTenantAdmin,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let customer_id = body
        .get("customerId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    state
        .registry
        .execute(
            "customer",
            "resolve_migration_exception",
            serde_json::json!({"exceptionId": id, "customerId": customer_id}),
            tenant_admin.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn anonymize_customer(
    State(state): State<Arc<AppState>>,
    tenant_admin: TrustedTenantAdmin,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "customer",
            "anonymize_customer",
            serde_json::json!({"customerId": id}),
            tenant_admin.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}
