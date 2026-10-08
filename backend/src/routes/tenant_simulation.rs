//! Explicit, allowlisted HTTP adapter for the tenant simulation plane.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
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

pub fn tenant_simulation_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/tenant-simulations", post(create_session))
        .route(
            "/api/tenant-simulations/{id}",
            get(get_session).delete(discard_session),
        )
        .route(
            "/api/tenant-simulations/{id}/reference-configs",
            get(list_reference_configs).post(create_reference_config),
        )
        .route(
            "/api/tenant-simulations/{id}/reference-configs/{key}",
            get(get_reference_config)
                .put(put_reference_config)
                .delete(delete_reference_config),
        )
        .route(
            "/api/tenant-simulations/{id}/pricing-config",
            get(get_pricing_config).put(put_pricing_config),
        )
        .route(
            "/api/tenant-simulations/{id}/pricing-estimate",
            get(estimate_pricing),
        )
        .route(
            "/api/tenant-simulations/{id}/commands/record-effect",
            post(record_effect),
        )
        .route("/api/tenant-simulations/{id}/diff", get(evaluate_diff))
        .route(
            "/api/tenant-simulations/{id}/diff/{evaluation_id}",
            get(get_diff),
        )
        .route(
            "/api/tenant-simulations/{id}/diff/{evaluation_id}/export",
            get(export_diff),
        )
        .route("/api/tenant-simulations/{id}/effects", get(list_effects))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateSessionBody {
    tenant_id: String,
    scenario_name: String,
    change_intent: String,
    ttl_minutes: Option<i64>,
    #[serde(default)]
    planned_absent_keys: Vec<String>,
    idempotency_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReferenceValueBody {
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateReferenceBody {
    key: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordEffectBody {
    key: String,
    value: Value,
    effect: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PricingConfigBody {
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PricingEstimateQuery {
    start_date: String,
    end_date: String,
    model_id: Option<String>,
    province: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct PageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct DiffPageQuery {
    cursor: Option<i64>,
    limit: Option<u32>,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    headers: HeaderMap,
    Json(body): Json<CreateSessionBody>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let idempotency_key = mutation_key(&headers)?;
    if body
        .idempotency_key
        .as_deref()
        .is_some_and(|value| value != idempotency_key)
    {
        return Err(AppError::CodedConflict {
            code: "IDEMPOTENCY_CONFLICT".into(),
            message: "body idempotency key does not match Idempotency-Key header".into(),
        });
    }
    let value = platform_execute(
        &state,
        &admin,
        "session.create",
        json!({
            "tenantId": body.tenant_id,
            "scenarioName": body.scenario_name,
            "changeIntent": body.change_intent,
            "ttlMinutes": body.ttl_minutes,
            "plannedAbsentKeys": body.planned_absent_keys,
            "idempotencyKey": idempotency_key,
        }),
    )?;
    let status = if value.get("status").and_then(Value::as_str) == Some("provisioning") {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(value)))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    platform_execute(&state, &admin, "session.get", json!({ "id": id })).map(Json)
}

async fn discard_session(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let value = platform_execute(&state, &admin, "session.discard", json!({ "id": id }))?;
    Ok((StatusCode::ACCEPTED, Json(value)))
}

async fn list_reference_configs(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>, AppError> {
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "reference_config.list",
        json!({ "id": id, "cursor": query.cursor, "limit": query.limit }),
    )
    .map(Json)
}

async fn get_reference_config(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, key)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "reference_config.get",
        json!({ "id": id, "key": key }),
    )
    .map(Json)
}

async fn create_reference_config(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateReferenceBody>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let key = mutation_key(&headers)?;
    let value = simulation_execute(
        &state,
        &admin,
        &id,
        Some(key.clone()),
        "reference_config.put",
        json!({ "id": id, "key": body.key, "value": body.value, "idempotencyKey": key }),
    )?;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn put_reference_config(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, resource_key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<ReferenceValueBody>,
) -> Result<Json<Value>, AppError> {
    let key = mutation_key(&headers)?;
    simulation_execute(
        &state,
        &admin,
        &id,
        Some(key.clone()),
        "reference_config.put",
        json!({ "id": id, "key": resource_key, "value": body.value, "idempotencyKey": key }),
    )
    .map(Json)
}

async fn delete_reference_config(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, resource_key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let key = mutation_key(&headers)?;
    simulation_execute(
        &state,
        &admin,
        &id,
        Some(key.clone()),
        "reference_config.delete",
        json!({ "id": id, "key": resource_key, "idempotencyKey": key }),
    )
    .map(Json)
}

async fn record_effect(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<RecordEffectBody>,
) -> Result<Json<Value>, AppError> {
    let key = mutation_key(&headers)?;
    simulation_execute(
        &state,
        &admin,
        &id,
        Some(key.clone()),
        "reference_config.record_effect",
        json!({
            "id": id,
            "key": body.key,
            "value": body.value,
            "effect": body.effect,
            "idempotencyKey": key,
        }),
    )
    .map(Json)
}

async fn get_pricing_config(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "pricing_config.get",
        json!({"id":id}),
    )
    .map(Json)
}

async fn put_pricing_config(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PricingConfigBody>,
) -> Result<Json<Value>, AppError> {
    let key = mutation_key(&headers)?;
    simulation_execute(
        &state,
        &admin,
        &id,
        Some(key.clone()),
        "pricing_config.put",
        json!({"id":id,"value":body.value,"idempotencyKey":key}),
    )
    .map(Json)
}

async fn estimate_pricing(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    Query(query): Query<PricingEstimateQuery>,
) -> Result<Json<Value>, AppError> {
    if query.model_id.is_some() || query.province.is_some() {
        return Err(AppError::BadRequest(
            "modelId and province are not in MVP4 simulation pricing scope".into(),
        ));
    }
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "pricing_config.estimate",
        json!({"id":id,"startDate":query.start_date,"endDate":query.end_date}),
    )
    .map(Json)
}

async fn evaluate_diff(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "diff.evaluate",
        json!({ "id": id }),
    )
    .map(Json)
}

async fn get_diff(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, evaluation_id)): Path<(String, String)>,
    Query(query): Query<DiffPageQuery>,
) -> Result<Json<Value>, AppError> {
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "diff.get",
        json!({
            "id": id,
            "evaluationId": evaluation_id,
            "cursor": query.cursor,
            "limit": query.limit,
        }),
    )
    .map(Json)
}

async fn export_diff(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path((id, evaluation_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let mut cursor: Option<i64> = None;
    let mut items = Vec::new();
    loop {
        let page = simulation_execute(
            &state,
            &admin,
            &id,
            None,
            "diff.get",
            json!({
                "id": id,
                "evaluationId": evaluation_id,
                "cursor": cursor,
                "limit": 500,
            }),
        )?;
        items.extend(
            page.get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        cursor = page.get("nextCursor").and_then(Value::as_i64);
        if cursor.is_none() {
            break;
        }
    }
    let value = json!({ "evaluationId": evaluation_id, "items": items });
    let body =
        serde_json::to_vec_pretty(&value).map_err(|error| AppError::Internal(error.to_string()))?;
    let filename = format!("simulation-{id}-diff-{evaluation_id}.json");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                .map_err(|error| AppError::Internal(error.to_string()))?,
        )
        .body(Body::from(body))
        .map_err(|error| AppError::Internal(error.to_string()))
}

async fn list_effects(
    State(state): State<Arc<AppState>>,
    PlatformUser(admin): PlatformUser,
    Path(id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>, AppError> {
    simulation_execute(
        &state,
        &admin,
        &id,
        None,
        "effects.list",
        json!({ "id": id, "cursor": query.cursor, "limit": query.limit }),
    )
    .map(Json)
}

fn mutation_key(headers: &HeaderMap) -> Result<String, AppError> {
    let value = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| (16..=128).contains(&value.len()))
        .filter(|value| value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
        .ok_or_else(|| {
            AppError::BadRequest(
                "Idempotency-Key must contain 16-128 visible ASCII characters".into(),
            )
        })?;
    Ok(value.to_owned())
}

fn platform_execute(
    state: &Arc<AppState>,
    admin: &AuthUserInfo,
    command: &str,
    payload: Value,
) -> Result<Value, AppError> {
    let capability = match command {
        "session.create" => PlatformCapability::TenantSimulationCreate,
        "session.discard" => PlatformCapability::TenantSimulationDiscard,
        _ => PlatformCapability::TenantSimulationRead,
    };
    if !admin.has_platform_capability(capability) {
        return Err(AppError::Forbidden);
    }
    let ctx =
        make_platform_ctx(admin, state.http_client.clone()).map_err(|_| AppError::Forbidden)?;
    state
        .registry
        .execute("tenant_simulation", command, payload, &ctx)
        .map_err(simulation_error)
}

fn simulation_execute(
    state: &Arc<AppState>,
    admin: &AuthUserInfo,
    session_id: &str,
    idempotency_key: Option<String>,
    command: &str,
    payload: Value,
) -> Result<Value, AppError> {
    let resolved = platform_execute(state, admin, "session.get", json!({ "id": session_id }))?;
    let session = resolved.get("session").unwrap_or(&resolved);
    let tenant = session
        .get("tenantId")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("simulation resolver returned no tenant".into()))?;
    let revision = session
        .get("baseRevision")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        })
        .filter(|value| !value.is_empty() && value != "null")
        .ok_or_else(|| {
            AppError::Internal("simulation resolver returned no base revision".into())
        })?;
    let simulation_id = SimulationId::new(session_id).map_err(AppError::Internal)?;
    let tenant_id = TenantId::new(tenant).map_err(AppError::Internal)?;
    let ctx = ExecutionContext::new(
        ActorIdentity::with_authority(admin.id.clone(), admin.authority.clone())
            .map_err(AppError::Internal)?,
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(
            tenant_id,
            Namespace::Simulation(simulation_id.clone()),
            Revision::new(revision).map_err(AppError::Internal)?,
        )
        .map_err(AppError::Internal)?,
        ExecutionMode::Simulation(simulation_id),
        RequestId::new(format!("simulation:{session_id}:{}", uuid::Uuid::new_v4()))
            .map_err(AppError::Internal)?,
        idempotency_key,
        state.http_client.clone(),
    )
    .map_err(AppError::Internal)?;
    let result = state
        .registry
        .execute("tenant_simulation", command, payload, &ctx)
        .map_err(simulation_error)?;
    // Post-validation prevents a result escaping after expiry/discard.
    platform_execute(state, admin, "session.get", json!({ "id": session_id }))?;
    Ok(result)
}

fn simulation_error(raw: String) -> AppError {
    let Ok(payload) = serde_json::from_str::<system_core::ErrorPayload>(&raw) else {
        return AppError::Internal("tenant simulation command failed".into());
    };
    match payload.code.as_str() {
        "SIMULATION_NOT_FOUND" | "NOT_FOUND" | "DIFF_EVALUATION_NOT_FOUND" => {
            AppError::CodedNotFound {
                code: payload.code,
                message: payload.message,
            }
        }
        "SIMULATION_PENDING" | "TERMINAL_EVIDENCE_PENDING" => AppError::CodedConflict {
            code: payload.code,
            message: payload.message,
        },
        "SESSION_NOT_EXECUTABLE" | "IDEMPOTENCY_CONFLICT" | "DIFF_EVALUATION_STALE" => {
            AppError::CodedConflict {
                code: payload.code,
                message: payload.message,
            }
        }
        "SIMULATION_QUOTA_EXCEEDED" | "SIMULATION_RATE_LIMITED" => AppError::RateLimited {
            retry_after_secs: 60,
        },
        code if code.starts_with("VAL_") || code == "UNPROVISIONED_RESOURCE_KEY" => {
            AppError::BadRequest(payload.message)
        }
        code if code.starts_with("AUTH_") || code.starts_with("EXEC_") => AppError::Forbidden,
        _ => {
            tracing::error!(code = %payload.code, "tenant simulation command failed");
            AppError::ServiceError {
                code: payload.code,
                message: "tenant simulation command failed".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_key_is_strict_and_secret_agnostic() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Idempotency-Key",
            HeaderValue::from_static("0123456789abcdef"),
        );
        assert_eq!(mutation_key(&headers).unwrap(), "0123456789abcdef");
        headers.insert("Idempotency-Key", HeaderValue::from_static("short"));
        assert!(mutation_key(&headers).is_err());
    }
}
