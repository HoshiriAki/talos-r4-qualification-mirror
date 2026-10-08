use axum::extract::{Path, Query, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use std::collections::HashMap;
use std::sync::Arc;

use crate::application::WarehouseAuthorityError;
use crate::error::AppError;
use crate::middleware::tenant_extractors::{TenantAdmin, TenantUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn warehouse_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/warehouses",
            get(list_warehouses).post(create_warehouse),
        )
        .route("/api/warehouses/stats", get(warehouse_stats))
        .route(
            "/api/warehouses/{id}",
            get(get_warehouse)
                .patch(update_warehouse)
                .delete(delete_warehouse_route),
        )
        .route("/api/warehouses/{id}/devices", get(warehouse_devices))
        .route(
            "/api/warehouses/{id}/regions",
            get(list_region_rules).put(upsert_region_rule),
        )
        .route(
            "/api/warehouses/{id}/regions/{province}",
            delete(delete_region_rule_route),
        )
}

// ── Handlers ────────────────────────────────────────────────────

async fn list_warehouses(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    state
        .registry
        .execute("warehouse", "list_warehouses", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn get_warehouse(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    state
        .registry
        .execute(
            "warehouse",
            "get_warehouse",
            serde_json::json!({"id": id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn create_warehouse(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    state
        .registry
        .execute("warehouse", "create_warehouse", body, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn update_warehouse(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let mut payload = body;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("id".to_string(), serde_json::Value::String(id));
    }
    state
        .registry
        .execute("warehouse", "update_warehouse", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn delete_warehouse_route(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    state
        .registry
        .execute(
            "warehouse",
            "delete_warehouse",
            serde_json::json!({"id": id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn list_region_rules(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let rules = state
        .application_services()
        .warehouse_authority()
        .region_rules(&ctx, &id)
        .map_err(warehouse_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true, "regions": rules })))
}

async fn upsert_region_rule(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let rules = state
        .application_services()
        .warehouse_authority()
        .upsert_region_rule(&ctx, &id, &body)
        .map_err(warehouse_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true, "regions": rules })))
}

async fn delete_region_rule_route(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Path((id, province)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    state
        .application_services()
        .warehouse_authority()
        .delete_region_rule(&ctx, &id, &province)
        .map_err(warehouse_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Stats & Devices ──────────────────────────────────────────────

async fn warehouse_stats(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let warehouses = state
        .registry
        .execute(
            "warehouse",
            "get_warehouse_stats",
            serde_json::json!({}),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "warehouses": warehouses,
    })))
}

async fn warehouse_devices(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let page: i64 = params.get("page").and_then(|v| v.parse().ok()).unwrap_or(1);
    let page_size: i64 = params
        .get("pageSize")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let keyword = params.get("keyword").map(|s| s.as_str());

    let result = state
        .application_services()
        .warehouse_authority()
        .devices_paged(&ctx, &id, page, page_size, keyword)
        .map_err(warehouse_authority_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "data": result.data,
        "total": result.total,
        "page": result.page,
        "pageSize": result.page_size,
    })))
}

fn warehouse_authority_error(error: WarehouseAuthorityError) -> AppError {
    match error {
        WarehouseAuthorityError::InvalidInput(message) => AppError::BadRequest(message),
        WarehouseAuthorityError::NotFound => AppError::NotFound("仓库不存在".into()),
        WarehouseAuthorityError::DuplicateName => AppError::Conflict("仓库名称已存在".into()),
        WarehouseAuthorityError::Referenced(count) => {
            AppError::Conflict(format!("该仓库下有 {count} 台设备，无法删除"))
        }
        WarehouseAuthorityError::RegionRuleNotFound => AppError::NotFound("区域规则不存在".into()),
        WarehouseAuthorityError::Persistence { code } => AppError::ServiceError {
            code: code.into(),
            message: "warehouse persistence unavailable".into(),
        },
    }
}
