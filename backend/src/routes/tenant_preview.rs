//! Explicit, allowlisted HTTP adapter for read-only tenant preview.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, Namespace, PlatformCapability,
    RequestId, Revision, SimulationId, TenantId, TenantScope,
};

use crate::{
    auth_contract::AuthUserInfo, error::AppError, middleware::tenant_extractors::PlatformUser,
    registry::make_platform_ctx, state::AppState,
};

pub fn tenant_preview_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/tenant-preview/sessions", post(create_session))
        .route("/api/tenant-workspaces/sessions", post(create_session))
        .route(
            "/api/tenant-preview/sessions/{id}",
            get(get_session).delete(end_session),
        )
        .route(
            "/api/tenant-workspaces/sessions/{id}",
            get(get_session).delete(end_session),
        )
        .route(
            "/api/tenant-preview/sessions/{id}/dashboard/summary",
            get(dashboard_summary),
        )
        .route("/api/tenant-preview/sessions/{id}/orders", get(list_orders))
        .route(
            "/api/tenant-preview/sessions/{id}/orders/{order_id}",
            get(get_order),
        )
        .route(
            "/api/tenant-preview/sessions/{id}/devices",
            get(list_devices),
        )
        .route(
            "/api/tenant-preview/sessions/{id}/devices/{serial_no}",
            get(get_device),
        )
        .route(
            "/api/tenant-preview/sessions/{id}/audit-logs",
            get(list_audit_logs),
        )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBody {
    tenant_id: String,
    ttl_minutes: Option<i64>,
    mode: Option<String>,
    simulation_id: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct OrdersQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    offset: Option<u32>,
    limit: Option<u32>,
    keyword: Option<String>,
    order_no: Option<String>,
    address: Option<String>,
    start_date_from: Option<String>,
    start_date_to: Option<String>,
    start_date: Option<String>,
    end_date_from: Option<String>,
    end_date_to: Option<String>,
    end_date: Option<String>,
    included_date: Option<String>,
    delivery_date_from: Option<String>,
    delivery_date_to: Option<String>,
    delivery_date: Option<String>,
    pickup_methods: Option<Vec<String>>,
    status: Option<String>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DevicesQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    keyword: Option<String>,
    rental_status: Option<String>,
    notes: Option<String>,
    warning_status: Option<String>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AuditQuery {
    actor_identity_id: Option<String>,
    actor_username: Option<String>,
    action_type: Option<String>,
    entity_type: Option<String>,
    keyword: Option<String>,
    date: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

fn order_page_window(q: &OrdersQuery) -> Result<(u32, u32), AppError> {
    match q.offset {
        Some(offset) => {
            let limit = q.limit.unwrap_or(50).clamp(1, 200);
            if offset % limit != 0 {
                return Err(AppError::BadRequest(
                    "preview order offset must align with limit".into(),
                ));
            }
            Ok((offset / limit + 1, limit))
        }
        None => Ok((q.page.unwrap_or(1), q.page_size.unwrap_or(30).clamp(1, 200))),
    }
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, AppError> {
    let mode = body.mode.as_deref().unwrap_or("preview");
    let required = match mode {
        "preview" => PlatformCapability::TenantPreviewCreate,
        "diagnostics" => PlatformCapability::TenantDiagnosticsRead,
        "simulation" => PlatformCapability::TenantSimulationRead,
        _ => return Err(AppError::BadRequest("invalid workspace mode".into())),
    };
    if !admin.has_platform_capability(required) {
        return Err(AppError::Forbidden);
    }
    let command = match mode {
        "preview" => "workspace.create_preview",
        "diagnostics" => "workspace.create_diagnostics",
        "simulation" => "workspace.create_simulation",
        _ => unreachable!("workspace mode was validated"),
    };
    platform_execute(
        &state,
        &admin,
        command,
        json!({
            "tenantId": body.tenant_id,
            "ttlMinutes": body.ttl_minutes,
            "mode": mode,
            "simulationId": body.simulation_id
        }),
    )
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let result = platform_execute(&state, &admin, "session.get", json!({ "id": id }))?;
    require_resolved_workspace_capability(&admin, &result.0)?;
    Ok(result)
}

async fn end_session(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let loaded = platform_execute(&state, &admin, "session.get", json!({ "id": id.clone() }))?;
    require_resolved_workspace_capability(&admin, &loaded.0)?;
    platform_execute(&state, &admin, "session.end", json!({ "id": id }))
}

async fn dashboard_summary(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    preview_execute(
        &state,
        &admin,
        &id,
        "tenant_preview",
        "dashboard.summary",
        json!({}),
    )
}

async fn list_orders(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    Query(q): Query<OrdersQuery>,
) -> Result<Json<Value>, AppError> {
    let (page, page_size) = order_page_window(&q)?;
    let filter = json!({
        "keyword": q.keyword.unwrap_or_default(),
        "orderNo": q.order_no.unwrap_or_default(),
        "address": q.address.unwrap_or_default(),
        "startDateFrom": q.start_date_from.unwrap_or_default(),
        "startDateTo": q.start_date_to.unwrap_or_default(),
        "startDate": q.start_date.unwrap_or_default(),
        "endDateFrom": q.end_date_from.unwrap_or_default(),
        "endDateTo": q.end_date_to.unwrap_or_default(),
        "endDate": q.end_date.unwrap_or_default(),
        "includedDate": q.included_date.unwrap_or_default(),
        "deliveryDateFrom": q.delivery_date_from.unwrap_or_default(),
        "deliveryDateTo": q.delivery_date_to.unwrap_or_default(),
        "deliveryDate": q.delivery_date.unwrap_or_default(),
        "pickupMethods": q.pickup_methods.unwrap_or_default(),
        "status": q.status.unwrap_or_default(),
    });
    preview_execute(
        &state,
        &admin,
        &id,
        "order",
        "list_orders",
        json!({
        "page": page,
        "pageSize": page_size,
            "filter": filter,
            "sortBy": q.sort_by,
            "sortOrder": q.sort_order,
        }),
    )
}

async fn get_order(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, order_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    preview_execute(
        &state,
        &admin,
        &id,
        "order",
        "get_order",
        json!({ "id": order_id }),
    )
}

async fn list_devices(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    Query(q): Query<DevicesQuery>,
) -> Result<Json<Value>, AppError> {
    preview_execute(
        &state,
        &admin,
        &id,
        "device",
        "list_devices_paged",
        json!({
            "page": q.page.unwrap_or(1),
            "pageSize": q.page_size.unwrap_or(30),
            "keyword": q.keyword,
            "rentalStatus": q.rental_status,
            "notes": q.notes,
            "warningStatus": q.warning_status,
            "sortBy": q.sort_by,
            "sortOrder": q.sort_order,
        }),
    )
}

async fn get_device(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, serial_no)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    preview_execute(
        &state,
        &admin,
        &id,
        "device",
        "get_device",
        json!({ "serialNo": serial_no }),
    )
}

async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Value>, AppError> {
    preview_execute(
        &state,
        &admin,
        &id,
        "audit",
        "list_audit_logs",
        json!({
            "actorIdentityId": q.actor_identity_id.unwrap_or_default(),
            "actorUsername": q.actor_username.unwrap_or_default(),
            "actionType": q.action_type.unwrap_or_default(),
            "entityType": q.entity_type.unwrap_or_default(),
            "keyword": q.keyword.unwrap_or_default(),
            "date": q.date.unwrap_or_default(),
            "startDate": q.start_date.unwrap_or_default(),
            "endDate": q.end_date.unwrap_or_default(),
            "limit": q.limit.unwrap_or(100),
            "offset": q.offset.unwrap_or(0),
            "sortBy": q.sort_by,
            "sortOrder": q.sort_order,
        }),
    )
}

fn platform_execute(
    state: &Arc<AppState>,
    admin: &AuthUserInfo,
    command: &str,
    payload: Value,
) -> Result<Json<Value>, AppError> {
    let capability = match command {
        "session.create" | "workspace.create_preview" => PlatformCapability::TenantPreviewCreate,
        "workspace.create_diagnostics" => PlatformCapability::TenantDiagnosticsRead,
        "workspace.create_simulation" => PlatformCapability::TenantSimulationRead,
        "session.get" | "session.end" | "session.resolve" => {
            if !has_any_workspace_read_capability(admin) {
                return Err(AppError::Forbidden);
            }
            PlatformCapability::TenantPreviewRead
        }
        _ => PlatformCapability::TenantPreviewRead,
    };
    if !matches!(command, "session.get" | "session.end" | "session.resolve")
        && !admin.has_platform_capability(capability)
    {
        return Err(AppError::Forbidden);
    }
    let ctx =
        make_platform_ctx(admin, state.http_client.clone()).map_err(|_| AppError::Forbidden)?;
    state
        .registry
        .execute("tenant_preview", command, payload, &ctx)
        .map(Json)
        .map_err(preview_error)
}

fn preview_execute(
    state: &Arc<AppState>,
    admin: &AuthUserInfo,
    session_id: &str,
    module: &str,
    command: &str,
    payload: Value,
) -> Result<Json<Value>, AppError> {
    // Session id is an opaque locator only. Ownership, tenant activity, expiry,
    // and revocation are revalidated server-side for every data-plane request.
    let correlation = RequestId::new(format!("preview:{session_id}:{}", uuid::Uuid::new_v4()))
        .map_err(AppError::Internal)?;
    let platform = preview_platform_ctx(admin, correlation.clone(), state)?;
    let resolved = state
        .registry
        .execute(
            "tenant_preview",
            "session.resolve",
            json!({ "id": session_id }),
            &platform,
        )
        .map_err(preview_error)?;
    require_resolved_workspace_capability(admin, &resolved)?;
    let tenant_id = resolved
        .get("tenantId")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("preview resolver returned no tenant".into()))?;
    let mode = resolved
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("preview");
    let simulation_id = resolved.get("simulationId").and_then(Value::as_str);
    let simulation_revision = if mode == "simulation" {
        let simulation_id = simulation_id
            .ok_or_else(|| AppError::Internal("workspace simulation id missing".into()))?;
        Some(resolve_workspace_simulation_revision(
            state,
            &platform,
            simulation_id,
            tenant_id,
        )?)
    } else {
        None
    };
    let ctx = workspace_data_ctx(
        admin,
        session_id,
        tenant_id,
        mode,
        simulation_id,
        simulation_revision.as_deref(),
        correlation,
        state,
    )?;
    let result = state
        .registry
        .execute(module, command, payload, &ctx)
        .map_err(preview_error)?;
    state
        .registry
        .execute(
            "tenant_preview",
            "session.resolve",
            json!({ "id": session_id }),
            &platform,
        )
        .map_err(preview_error)?;
    Ok(Json(result))
}

fn resolve_workspace_simulation_revision(
    state: &Arc<AppState>,
    platform: &ExecutionContext,
    simulation_id: &str,
    expected_tenant_id: &str,
) -> Result<String, AppError> {
    let resolved = state
        .registry
        .execute(
            "tenant_simulation",
            "session.get",
            json!({ "id": simulation_id }),
            platform,
        )
        .map_err(workspace_simulation_error)?;
    let session = resolved.get("session").unwrap_or(&resolved);
    let tenant_id = session
        .get("tenantId")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("simulation resolver returned no tenant".into()))?;
    if tenant_id != expected_tenant_id {
        return Err(AppError::CodedConflict {
            code: "SIMULATION_SESSION_TENANT_MISMATCH".into(),
            message: "workspace simulation tenant does not match the resolved workspace".into(),
        });
    }
    let status = session
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("simulation resolver returned no status".into()))?;
    if status != "active" {
        return Err(AppError::CodedConflict {
            code: "SIMULATION_SESSION_INACTIVE".into(),
            message: "workspace simulation is no longer active".into(),
        });
    }
    session
        .get("baseRevision")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        })
        .filter(|value| !value.is_empty() && value != "null")
        .ok_or_else(|| AppError::Internal("simulation resolver returned no base revision".into()))
}

fn workspace_simulation_error(raw: String) -> AppError {
    let Ok(payload) = serde_json::from_str::<system_core::ErrorPayload>(&raw) else {
        return AppError::Internal("tenant simulation command failed".into());
    };
    match payload.code.as_str() {
        "SIMULATION_NOT_FOUND" | "NOT_FOUND" => AppError::CodedNotFound {
            code: "SIMULATION_SESSION_NOT_FOUND".into(),
            message: payload.message,
        },
        "SESSION_NOT_EXECUTABLE" => AppError::CodedConflict {
            code: payload.code,
            message: payload.message,
        },
        code if code.starts_with("VAL_") => AppError::BadRequest(payload.message),
        code if code.starts_with("AUTH_") || code.starts_with("EXEC_") => AppError::Forbidden,
        _ => {
            tracing::error!(code = %payload.code, "tenant simulation resolution failed");
            AppError::ServiceError {
                code: payload.code,
                message: "tenant simulation command failed".into(),
            }
        }
    }
}

fn preview_platform_ctx(
    admin: &AuthUserInfo,
    correlation: RequestId,
    state: &Arc<AppState>,
) -> Result<ExecutionContext, AppError> {
    if !has_any_workspace_read_capability(admin) {
        return Err(AppError::Forbidden);
    }
    ExecutionContext::new(
        ActorIdentity::with_authority(admin.id.clone(), admin.authority.clone())
            .map_err(AppError::Internal)?,
        TenantScope::platform(),
        DataScope::platform(Revision::new("control-plane-current").map_err(AppError::Internal)?),
        ExecutionMode::Normal,
        correlation,
        None,
        state.http_client.clone(),
    )
    .map_err(AppError::Internal)
}

/// This host-private constructor consumes only the tenant returned by the
/// immediately preceding actor-bound session.resolve call.
fn workspace_data_ctx(
    admin: &AuthUserInfo,
    session_id: &str,
    tenant_id: &str,
    mode: &str,
    simulation_id: Option<&str>,
    simulation_revision: Option<&str>,
    correlation: RequestId,
    state: &Arc<AppState>,
) -> Result<ExecutionContext, AppError> {
    require_workspace_mode_capability(admin, mode)?;
    let tenant_id = TenantId::new(tenant_id).map_err(AppError::Internal)?;
    let (data_scope, execution_mode) = match mode {
        "preview" => (
            DataScope::production(
                tenant_id.clone(),
                Revision::new("preview-production-current").map_err(AppError::Internal)?,
            )
            .map_err(AppError::Internal)?,
            ExecutionMode::ReadOnlyPreview(
                system_core::PreviewSessionId::new(session_id).map_err(AppError::Internal)?,
            ),
        ),
        "diagnostics" => (
            DataScope::production(
                tenant_id.clone(),
                Revision::new("diagnostics-production-current").map_err(AppError::Internal)?,
            )
            .map_err(AppError::Internal)?,
            ExecutionMode::ReadOnlyPreview(
                system_core::PreviewSessionId::new(session_id).map_err(AppError::Internal)?,
            ),
        ),
        "simulation" => {
            let simulation_id = SimulationId::new(
                simulation_id
                    .ok_or_else(|| AppError::Internal("workspace simulation id missing".into()))?,
            )
            .map_err(AppError::Internal)?;
            let revision = simulation_revision.ok_or_else(|| {
                AppError::Internal("workspace simulation revision missing".into())
            })?;
            (
                DataScope::new(
                    tenant_id.clone(),
                    Namespace::Simulation(simulation_id.clone()),
                    Revision::new(revision.to_owned()).map_err(AppError::Internal)?,
                )
                .map_err(AppError::Internal)?,
                ExecutionMode::Simulation(simulation_id),
            )
        }
        _ => return Err(AppError::Internal("workspace mode is invalid".into())),
    };
    ExecutionContext::new(
        ActorIdentity::with_authority(admin.id.clone(), admin.authority.clone())
            .map_err(AppError::Internal)?,
        TenantScope::tenant(tenant_id.clone()),
        data_scope,
        execution_mode,
        correlation,
        None,
        state.http_client.clone(),
    )
    .map_err(AppError::Internal)
}

fn has_any_workspace_read_capability(admin: &AuthUserInfo) -> bool {
    [
        PlatformCapability::TenantPreviewRead,
        PlatformCapability::TenantDiagnosticsRead,
        PlatformCapability::TenantSimulationRead,
    ]
    .into_iter()
    .any(|capability| admin.has_platform_capability(capability))
}

fn require_workspace_mode_capability(admin: &AuthUserInfo, mode: &str) -> Result<(), AppError> {
    let capability = match mode {
        "preview" => PlatformCapability::TenantPreviewRead,
        "diagnostics" => PlatformCapability::TenantDiagnosticsRead,
        "simulation" => PlatformCapability::TenantSimulationRead,
        _ => return Err(AppError::Internal("workspace mode is invalid".into())),
    };
    if admin.has_platform_capability(capability) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn require_resolved_workspace_capability(
    admin: &AuthUserInfo,
    resolved: &Value,
) -> Result<(), AppError> {
    let session = resolved.get("session").unwrap_or(resolved);
    let mode = session
        .get("mode")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("workspace resolver returned no mode".into()))?;
    require_workspace_mode_capability(admin, mode)
}

fn preview_error(raw: String) -> AppError {
    let Ok(payload) = serde_json::from_str::<system_core::ErrorPayload>(&raw) else {
        return AppError::Internal("tenant preview command failed".into());
    };
    match payload.code.as_str() {
        "NOT_FOUND" => AppError::CodedNotFound {
            code: "PREVIEW_SESSION_NOT_FOUND".into(),
            message: payload.message,
        },
        "BIZ_ORDER_NOT_FOUND" | "BIZ_DEVICE_NOT_FOUND" => AppError::NotFound(payload.message),
        "PREVIEW_SESSION_EXPIRED" => AppError::Gone {
            code: payload.code,
            message: payload.message,
        },
        "PREVIEW_SESSION_INACTIVE" | "PREVIEW_TENANT_INACTIVE" => AppError::CodedConflict {
            code: payload.code,
            message: payload.message,
        },
        code if code.starts_with("VAL_") => AppError::BadRequest(payload.message),
        code if code.starts_with("AUTH_") || code.starts_with("EXEC_") => AppError::Forbidden,
        _ => {
            tracing::error!(code = %payload.code, "tenant preview command failed");
            AppError::ServiceError {
                code: payload.code,
                message: "tenant preview command failed".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_order_queries_map_to_bounded_pages() {
        let first = OrdersQuery {
            offset: Some(0),
            limit: Some(50),
            ..Default::default()
        };
        let second = OrdersQuery {
            offset: Some(50),
            limit: Some(50),
            ..Default::default()
        };
        let bounded = OrdersQuery {
            offset: Some(200),
            limit: Some(10_000),
            ..Default::default()
        };

        assert_eq!(order_page_window(&first).unwrap(), (1, 50));
        assert_eq!(order_page_window(&second).unwrap(), (2, 50));
        assert_eq!(order_page_window(&bounded).unwrap(), (2, 200));
    }

    #[test]
    fn unaligned_order_offsets_fail_closed() {
        let query = OrdersQuery {
            offset: Some(31),
            limit: Some(50),
            ..Default::default()
        };
        assert!(matches!(
            order_page_window(&query),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn unknown_preview_error_discards_module_message() {
        let error = preview_error(
            r#"{"category":"sys","code":"SYS_PROVIDER","message":"bearer secret-token","field":null,"context":null}"#
                .into(),
        );
        match error {
            AppError::ServiceError { code, message } => {
                assert_eq!(code, "SYS_PROVIDER");
                assert_eq!(message, "tenant preview command failed");
                assert!(!message.contains("secret-token"));
            }
            _ => panic!("unknown preview errors must become redacted ServiceError"),
        }
    }
}
