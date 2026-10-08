use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use std::sync::Arc;

use crate::application::DashboardReadAuthorityError;
use crate::error::AppError;
use crate::middleware::tenant_extractors::TenantUser;
use crate::registry::make_ctx;
use crate::state::AppState;

async fn dashboard_stats(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let stats = state
        .application_services()
        .dashboards()
        .overview(&ctx)
        .map_err(dashboard_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true, "data": stats })))
}

#[derive(serde::Deserialize)]
struct DailyCountsQuery {
    #[serde(default = "default_days")]
    days: i64,
}

fn default_days() -> i64 {
    15
}

async fn daily_order_counts(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<DailyCountsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let days = q.days.clamp(1, 60);
    let counts = state
        .application_services()
        .dashboards()
        .daily_order_counts(&ctx, days)
        .map_err(dashboard_authority_error)?;
    Ok(Json(
        serde_json::json!({ "ok": true, "data": { "days": days, "counts": counts } }),
    ))
}

#[derive(serde::Deserialize)]
struct TrendQuery {
    #[serde(default = "default_granularity")]
    granularity: String,
    #[serde(default = "default_trend_days")]
    days: i64,
}

fn default_granularity() -> String {
    "day".to_string()
}
fn default_trend_days() -> i64 {
    30
}

#[derive(serde::Deserialize)]
struct DaysQuery {
    #[serde(default = "default_trend_days")]
    days: i64,
}

async fn order_trend(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<TrendQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .order_trend(&ctx, &q.granularity, q.days)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

async fn revenue_trend(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<TrendQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .revenue_trend(&ctx, &q.granularity, q.days)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

async fn cancel_trend(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<DaysQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .cancel_trend(&ctx, q.days)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

async fn device_status_distribution(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .device_status_distribution(&ctx)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

#[derive(serde::Deserialize)]
struct ProvinceStatsQuery {
    #[serde(default = "default_province_type")]
    r#type: String,
    #[serde(default = "default_trend_days")]
    days: i64,
}

fn default_province_type() -> String {
    "pie".to_string()
}

async fn province_stats(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<ProvinceStatsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .province_stats(&ctx, &q.r#type, q.days)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

#[derive(serde::Deserialize)]
struct ModelRankingQuery {
    #[serde(default = "default_trend_days")]
    days: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    10
}

async fn model_ranking(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<ModelRankingQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .model_ranking(&ctx, q.days, q.limit)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

async fn warehouse_stats(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .dashboards()
        .warehouse_stats(&ctx)
        .map_err(dashboard_authority_error)?;
    Ok(Json(result))
}

fn dashboard_authority_error(error: DashboardReadAuthorityError) -> AppError {
    match error {
        DashboardReadAuthorityError::Persistence { code } => AppError::ServiceError {
            code: code.into(),
            message: "dashboard persistence unavailable".into(),
        },
    }
}

pub fn dashboard_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/dashboard/stats", get(dashboard_stats))
        .route("/api/dashboard/daily-counts", get(daily_order_counts))
        .route("/api/dashboard/order-trend", get(order_trend))
        .route("/api/dashboard/revenue-trend", get(revenue_trend))
        .route("/api/dashboard/cancel-trend", get(cancel_trend))
        .route(
            "/api/dashboard/device-status-distribution",
            get(device_status_distribution),
        )
        .route("/api/dashboard/province-stats", get(province_stats))
        .route("/api/dashboard/model-ranking", get(model_ranking))
        .route("/api/dashboard/warehouse-stats", get(warehouse_stats))
}
