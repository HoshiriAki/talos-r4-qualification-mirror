//! Platform tenant-governance HTTP adapter.
//!
//! Routes authenticate and translate DTOs only. All governance behavior is
//! dispatched through ModuleRegistry and audited under a platform scope.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use system_core::PlatformCapability;

use crate::{
    error::AppError, middleware::tenant_extractors::PlatformUser, registry::make_platform_ctx,
    state::AppState,
};

pub fn tenant_governance_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/tenant-governance/tenants", get(list_tenants))
        .route("/api/tenant-governance/tenants/{id}", get(get_tenant))
        .route(
            "/api/tenant-governance/tenants/{id}/health",
            get(get_tenant_health),
        )
        .route(
            "/api/tenant-governance/audit-events",
            get(query_audit_events),
        )
        .route(
            "/api/tenant-governance/audit-events/{id}",
            get(get_audit_event),
        )
        .route(
            "/api/tenant-governance/change-intents",
            post(record_change_intent),
        )
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PageQuery {
    search: Option<String>,
    status: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AuditQuery {
    tenant_id: Option<String>,
    actor: Option<String>,
    command: Option<String>,
    result: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeIntentBody {
    target_tenant_id: String,
    reason: String,
    intended_outcome: String,
    #[serde(default)]
    impact: String,
    cost_minor: Option<i64>,
    currency: Option<String>,
}

async fn list_tenants(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        &admin,
        "tenant.list",
        json!({
            "search": query.search,
            "status": query.status,
            "limit": query.limit,
            "cursor": query.cursor
        }),
    )
}

async fn get_tenant(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    execute(&state, &admin, "tenant.get", json!({ "id": id }))
}

async fn get_tenant_health(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    execute(&state, &admin, "tenant.health", json!({ "id": id }))
}

async fn query_audit_events(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        &admin,
        "audit.query",
        json!({
            "tenantId": query.tenant_id,
            "actor": query.actor,
            "command": query.command,
            "result": query.result,
            "from": query.from,
            "to": query.to,
            "limit": query.limit,
            "cursor": query.cursor,
        }),
    )
}

async fn get_audit_event(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    execute(&state, &admin, "audit.get", json!({ "id": id }))
}

async fn record_change_intent(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Json(body): Json<ChangeIntentBody>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        &admin,
        "change_intent.record",
        json!({
            "targetTenantId": body.target_tenant_id,
            "reason": body.reason,
            "intendedOutcome": body.intended_outcome,
            "impact": body.impact,
            "costMinor": body.cost_minor,
            "currency": body.currency,
        }),
    )
}

fn execute(
    state: &Arc<AppState>,
    admin: &crate::auth_contract::AuthUserInfo,
    command: &str,
    payload: Value,
) -> Result<Json<Value>, AppError> {
    let capability = if command == "change_intent.record" {
        PlatformCapability::TenantGovernanceManage
    } else if command.starts_with("audit.") {
        PlatformCapability::AuditRead
    } else {
        PlatformCapability::TenantGovernanceRead
    };
    if !admin.has_platform_capability(capability) {
        return Err(AppError::Forbidden);
    }
    let ctx =
        make_platform_ctx(admin, state.http_client.clone()).map_err(|_| AppError::Forbidden)?;
    state
        .registry
        .execute("tenant_governance", command, payload, &ctx)
        .map(Json)
        .map_err(governance_error)
}

fn governance_error(raw: String) -> AppError {
    let payload = serde_json::from_str::<system_core::ErrorPayload>(&raw);
    match payload {
        Ok(payload) if payload.code == "NOT_FOUND" => {
            AppError::NotFound("resource not found".into())
        }
        Ok(payload) if payload.code.starts_with("VAL_") => AppError::BadRequest(payload.message),
        Ok(payload)
            if payload.code.starts_with("AUTH_") || payload.code == "EXEC_ACCESS_DENIED" =>
        {
            AppError::Forbidden
        }
        Ok(payload) => AppError::ServiceError {
            code: payload.code,
            message: payload.message,
        },
        Err(_) => AppError::Internal("tenant governance command failed".into()),
    }
}
