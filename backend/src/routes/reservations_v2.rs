use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::error::AppError;
use crate::middleware::tenant_extractors::{TrustedTenantAdmin, TrustedTenantUser};
use crate::state::AppState;

pub fn reservation_v2_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v2/reservations/availability", get(availability))
        .route("/api/v2/reservations", post(create_from_order))
        .route("/api/v2/reservations/{id}", get(get_reservation))
        .route("/api/v2/reservations/{id}/confirm", post(confirm))
        .route("/api/v2/reservations/{id}/allocations", post(allocate))
        .route(
            "/api/v2/reservations/allocations/{id}/release",
            post(release_allocation),
        )
        .route("/api/v2/reservations/expire-due", post(expire_due))
        .route(
            "/api/v2/reservations/migration-exceptions",
            get(list_migration_exceptions),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvailabilityQuery {
    model_id: String,
    start_date: String,
    end_date: String,
    quantity: Option<u32>,
    device_serial_no: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateReservationBody {
    order_id: String,
    hold_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfirmBody {
    order_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AllocateBody {
    device_serial_no: String,
}

async fn availability(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Query(query): Query<AvailabilityQuery>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "availability",
        serde_json::json!({
            "modelId": query.model_id,
            "startDate": query.start_date,
            "endDate": query.end_date,
            "quantity": query.quantity.unwrap_or(1),
            "deviceSerialNo": query.device_serial_no,
        }),
        tenant_user.context(),
    )
}

async fn create_from_order(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Json(body): Json<CreateReservationBody>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "create_from_order",
        serde_json::json!({
            "orderId": body.order_id,
            "holdMinutes": body.hold_minutes.unwrap_or(30),
        }),
        tenant_user.context(),
    )
}

async fn get_reservation(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "get_reservation",
        serde_json::json!({"reservationId": id}),
        tenant_user.context(),
    )
}

async fn confirm(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
    Json(body): Json<ConfirmBody>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "confirm_reservation",
        serde_json::json!({"reservationId": id, "orderId": body.order_id}),
        tenant_user.context(),
    )
}

async fn allocate(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
    Json(body): Json<AllocateBody>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "allocate_device",
        serde_json::json!({
            "reservationId": id,
            "deviceSerialNo": body.device_serial_no,
        }),
        tenant_user.context(),
    )
}

async fn release_allocation(
    State(state): State<Arc<AppState>>,
    tenant_user: TrustedTenantUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "release_allocation",
        serde_json::json!({"allocationId": id}),
        tenant_user.context(),
    )
}

async fn expire_due(
    State(state): State<Arc<AppState>>,
    tenant_admin: TrustedTenantAdmin,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "expire_due",
        serde_json::json!({}),
        tenant_admin.context(),
    )
}

async fn list_migration_exceptions(
    State(state): State<Arc<AppState>>,
    tenant_admin: TrustedTenantAdmin,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "list_migration_exceptions",
        serde_json::json!({}),
        tenant_admin.context(),
    )
}

fn execute(
    state: &AppState,
    command: &str,
    payload: Value,
    context: &system_core::ExecutionContext,
) -> Result<Json<Value>, AppError> {
    state
        .registry
        .execute("reservation_v2", command, payload, context)
        .map(Json)
        .map_err(AppError::from_error_payload)
}
