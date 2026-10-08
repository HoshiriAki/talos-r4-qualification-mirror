use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::application::WarehouseAuthorityError;
use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::services::logistics_service;
use crate::services::warehouse_routing_service;
use crate::state::AppState;

pub fn pricing_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/pricing/estimate", get(estimate_pricing))
        .route("/api/pricing/config", get(get_config).put(update_config))
        .route(
            "/api/pricing/dynamic-prices",
            get(list_dynamic_prices).post(create_dynamic_price),
        )
        .route(
            "/api/pricing/dynamic-prices/{dateKey}",
            put(update_dynamic_price).delete(delete_dynamic_price_handler),
        )
        .route("/api/pricing/warehouse-route", get(warehouse_route))
        .route("/api/pricing/occupancy", get(occupancy_coefficients))
        .route("/api/pricing/provinces", get(provinces))
}

// ── Query / body types ───────────────────────────────────────────

#[derive(Deserialize)]
struct EstimateQuery {
    #[serde(alias = "startDate")]
    start_date: Option<String>,
    #[serde(alias = "endDate")]
    end_date: Option<String>,
    province: Option<String>,
    #[serde(alias = "modelId")]
    model_id: Option<String>,
}

#[derive(Deserialize)]
struct WarehouseRouteQuery {
    province: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────────────

/// GET /pricing/estimate — requires authentication
/// Estimate pricing for a hypothetical order.
async fn estimate_pricing(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<EstimateQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let payload = serde_json::json!({
        "startDate": q.start_date.unwrap_or_default(),
        "endDate": q.end_date.unwrap_or_default(),
        "province": q.province.unwrap_or_default(),
        "modelId": q.model_id.unwrap_or_default(),
    });
    let estimate = state
        .registry
        .execute("pricing", "estimate_pricing", payload, &ctx)
        .map_err(AppError::from_error_payload)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "estimate": estimate,
    })))
}

/// GET /pricing/config — any authenticated user
async fn get_config(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let result = state
        .registry
        .execute("pricing", "get_pricing_config", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

/// PUT /pricing/config — admin only
async fn update_config(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let mut payload = body.clone();
    if let Some(obj) = payload.as_object_mut()
        && !obj.contains_key("updatedBy")
    {
        obj.insert(
            "updatedBy".to_string(),
            serde_json::Value::String(_admin.0.username.clone()),
        );
    }
    let result = state
        .registry
        .execute("pricing", "update_pricing_config", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

/// GET /pricing/dynamic-prices — admin only
async fn list_dynamic_prices(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let result = state
        .registry
        .execute("pricing", "get_dynamic_prices", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

/// POST /pricing/dynamic-prices — admin only
async fn create_dynamic_price(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let mut payload = body.clone();
    if let Some(obj) = payload.as_object_mut()
        && !obj.contains_key("updatedBy")
    {
        obj.insert(
            "updatedBy".to_string(),
            serde_json::Value::String(_admin.0.username.clone()),
        );
    }
    let result = state
        .registry
        .execute("pricing", "upsert_dynamic_price", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

/// PUT /pricing/dynamic-prices/{dateKey} — admin only
async fn update_dynamic_price(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Path(date_key): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let mut payload = body.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("dateKey".to_string(), serde_json::Value::String(date_key));
        if !obj.contains_key("updatedBy") {
            obj.insert(
                "updatedBy".to_string(),
                serde_json::Value::String(_admin.0.username.clone()),
            );
        }
    }
    let result = state
        .registry
        .execute("pricing", "upsert_dynamic_price", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

/// DELETE /pricing/dynamic-prices/{dateKey} — admin only
async fn delete_dynamic_price_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Path(date_key): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "pricing",
            "delete_dynamic_price",
            serde_json::json!({"dateKey": date_key}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

/// GET /pricing/warehouse-route — authenticated
async fn warehouse_route(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(query): Query<WarehouseRouteQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let province = query.province.as_deref().unwrap_or("").trim();

    if province.is_empty() {
        return Err(AppError::BadRequest("缺少 province 参数".to_string()));
    }

    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    let route = state
        .application_services()
        .warehouse_authority()
        .resolve_route(&ctx, province)
        .map_err(warehouse_authority_error)?;

    match route {
        Some(route) => Ok(Json(serde_json::json!({
            "ok": true,
            "route": route,
        }))),
        None => Err(AppError::NotFound("未找到该省份的仓库路由".to_string())),
    }
}

/// GET /pricing/occupancy — authenticated
async fn occupancy_coefficients(_auth: AuthUser) -> Result<Json<serde_json::Value>, AppError> {
    let coeffs = warehouse_routing_service::get_occupancy_coefficients();

    Ok(Json(serde_json::json!({
        "ok": true,
        "coefficients": coeffs,
    })))
}

/// GET /pricing/provinces — authenticated
async fn provinces(_auth: AuthUser) -> Result<Json<serde_json::Value>, AppError> {
    let provinces = logistics_service::get_all_provinces();

    Ok(Json(serde_json::json!({
        "ok": true,
        "provinces": provinces,
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
