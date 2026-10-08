use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::tenant_extractors::TrustedTenantUser;
use crate::state::AppState;

pub fn order_v2_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v2/orders", get(list_orders))
        .route("/api/v2/orders/{id}", get(get_order))
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct OrderV2ListQuery {
    keyword: Option<String>,
    #[serde(alias = "orderNo")]
    order_no: Option<String>,
    address: Option<String>,
    province: Option<String>,
    #[serde(alias = "startDateFrom")]
    start_date_from: Option<String>,
    #[serde(alias = "startDateTo")]
    start_date_to: Option<String>,
    #[serde(alias = "startDate")]
    start_date: Option<String>,
    #[serde(alias = "endDateFrom")]
    end_date_from: Option<String>,
    #[serde(alias = "endDateTo")]
    end_date_to: Option<String>,
    #[serde(alias = "endDate")]
    end_date: Option<String>,
    #[serde(alias = "includedDate")]
    included_date: Option<String>,
    #[serde(alias = "deliveryDateFrom")]
    delivery_date_from: Option<String>,
    #[serde(alias = "deliveryDateTo")]
    delivery_date_to: Option<String>,
    #[serde(alias = "deliveryDate")]
    delivery_date: Option<String>,
    #[serde(alias = "pickupMethods")]
    pickup_methods: Option<Vec<String>>,
    #[serde(alias = "serialNo")]
    serial_no: Option<String>,
    #[serde(alias = "trackingNo")]
    tracking_no: Option<String>,
    status: Option<String>,
    page: Option<u32>,
    #[serde(alias = "pageSize")]
    page_size: Option<u32>,
    #[serde(alias = "sortBy")]
    sort_by: Option<String>,
    #[serde(alias = "sortOrder")]
    sort_order: Option<String>,
}

async fn list_orders(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Query(query): Query<OrderV2ListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let filter = serde_json::json!({
        "keyword": query.keyword.unwrap_or_default(),
        "orderNo": query.order_no.unwrap_or_default(),
        "address": query.address.unwrap_or_default(),
        "province": query.province.unwrap_or_default(),
        "startDateFrom": query.start_date_from.unwrap_or_default(),
        "startDateTo": query.start_date_to.unwrap_or_default(),
        "startDate": query.start_date.unwrap_or_default(),
        "endDateFrom": query.end_date_from.unwrap_or_default(),
        "endDateTo": query.end_date_to.unwrap_or_default(),
        "endDate": query.end_date.unwrap_or_default(),
        "includedDate": query.included_date.unwrap_or_default(),
        "deliveryDateFrom": query.delivery_date_from.unwrap_or_default(),
        "deliveryDateTo": query.delivery_date_to.unwrap_or_default(),
        "deliveryDate": query.delivery_date.unwrap_or_default(),
        "pickupMethods": query.pickup_methods.unwrap_or_default(),
        "serialNo": query.serial_no.unwrap_or_default(),
        "trackingNo": query.tracking_no.unwrap_or_default(),
        "status": query.status.unwrap_or_default(),
    });
    state
        .registry
        .execute(
            "order_query_v2",
            "list_orders",
            serde_json::json!({
                "page": query.page.unwrap_or(1),
                "pageSize": query.page_size.unwrap_or(30),
                "filter": filter,
                "sortBy": query.sort_by,
                "sortOrder": query.sort_order,
            }),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn get_order(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .registry
        .execute(
            "order_query_v2",
            "get_order",
            serde_json::json!({"id": id}),
            tenant_user.context(),
        )
        .map(Json)
        .map_err(AppError::from_error_payload)
}
