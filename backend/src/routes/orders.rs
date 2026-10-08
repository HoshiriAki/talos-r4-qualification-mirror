use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::application::DeviceCandidateQueryError;
use crate::error::AppError;
use crate::middleware::tenant_extractors::{TenantAdmin, TenantUser};
use crate::registry::make_ctx;
use crate::services::{
    audit_service, excel_import_service::*, multipart_import, order_compatibility_support,
    order_export_service,
};
use crate::state::AppState;
use crate::utils::http as http_utils;

const ORDER_BULK_ITEMS_MAX: usize = 500;
const ORDER_BATCH_SHIP_ORDERS_MAX: usize = 200;
const ORDER_BATCH_SHIP_DEVICES_PER_ORDER_MAX: usize = 200;
const ORDER_BATCH_SHIP_DEVICE_REFS_MAX: usize = 1_000;
const ORDER_EXPORT_IDS_MAX: usize = 500;
const ORDER_EXPORT_ROWS_MAX: usize = 2_000;
const ORDER_IMPORT_ROWS_MAX: usize = 2_000;

fn enforce_item_limit(label: &str, len: usize, max: usize) -> Result<(), AppError> {
    if len > max {
        return Err(AppError::BadRequest(format!("{label} 数量超过上限 {max}")));
    }
    Ok(())
}

pub fn order_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/users", get(list_orders))
        .route("/users/export", get(export_orders).post(export_orders_post))
        .route("/users/import-orders", post(import_orders))
        .route(
            "/users/import-devices-by-orderno",
            post(import_devices_by_orderno),
        )
        .route(
            "/users/import-notes-by-orderno",
            post(import_notes_by_orderno),
        )
        .route("/users/bulk-delete", post(bulk_delete_orders))
        .route("/users/batch-notes", put(batch_update_notes))
        .route("/users/bulk-update", put(bulk_update_orders))
        .route(
            "/users/{id}",
            get(get_order_by_id)
                .put(update_order)
                .delete(delete_order_handler),
        )
        .route("/users/{id}/devices", post(link_device_to_order_handler))
        .route(
            "/users/{id}/devices/{serial_no}",
            delete(detach_device_from_order_handler),
        )
        .route("/users/submit-batch-ship", post(submit_batch_ship_handler))
        .route("/users/batch-order-nos", post(batch_order_nos_handler))
        .route("/users/transition", post(transition_order))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OrderListQuery {
    keyword: Option<String>,
    #[serde(alias = "orderNo")]
    order_no: Option<String>,
    address: Option<String>,
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
    status: Option<String>,
    page: Option<i64>,
    #[serde(alias = "pageSize")]
    page_size: Option<i64>,
    offset: Option<i64>,
    limit: Option<i64>,
    #[serde(alias = "sortBy")]
    sort_by: Option<String>,
    #[serde(alias = "sortOrder")]
    sort_order: Option<String>,
}

async fn list_orders(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<OrderListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let filter = serde_json::json!({
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

    if let Some(offset) = q.offset {
        let payload = serde_json::json!({
            "limit": q.limit.unwrap_or(50),
            "offset": offset,
            "filter": filter,
            "sortBy": q.sort_by.clone(),
            "sortOrder": q.sort_order.clone(),
        });
        let result = state
            .registry
            .execute(
                "order_read_compatibility",
                "list_orders_offset",
                payload,
                &ctx,
            )
            .map(Json)
            .map_err(AppError::from_error_payload)?;
        return Ok(result);
    }

    let payload = serde_json::json!({
        "page": q.page.unwrap_or(1),
        "pageSize": q.page_size.unwrap_or(20),
        "filter": filter,
        "sortBy": q.sort_by,
        "sortOrder": q.sort_order,
    });

    let result = state
        .registry
        .execute("order_read_compatibility", "list_orders", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

async fn get_order_by_id(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "order_read_compatibility",
            "get_order",
            serde_json::json!({"id": id}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

async fn update_order(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    tenant_user: TenantUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let object = body
        .as_object()
        .ok_or_else(|| AppError::BadRequest("订单更新必须是对象".to_string()))?;
    let allowed = [
        "startDate",
        "endDate",
        "deliveryDate",
        "address",
        "province",
        "pickupMethods",
        "notes",
        "status",
        "reason",
        "expectedVersion",
    ];
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(AppError::BadRequest(format!(
            "不支持通用更新字段: {field}; 请使用命名订单命令"
        )));
    }

    let before = state
        .registry
        .execute("order", "get_order", serde_json::json!({"id": &id}), &ctx)
        .ok();
    let expected_version = object
        .get("expectedVersion")
        .and_then(serde_json::Value::as_i64)
        .filter(|version| *version > 0)
        .ok_or_else(|| AppError::BadRequest("expectedVersion 必须是正整数".to_string()))?;

    let mut result = before
        .clone()
        .unwrap_or_else(|| serde_json::json!({ "id": id }));
    let groups = [
        (
            "change_draft_dates",
            ["startDate", "endDate", "deliveryDate"].as_slice(),
        ),
        (
            "change_draft_address",
            ["address", "province", "pickupMethods"].as_slice(),
        ),
        ("change_notes", ["notes"].as_slice()),
    ];
    for (command, fields) in groups {
        let mut payload = serde_json::Map::new();
        payload.insert("orderId".into(), serde_json::Value::String(id.clone()));
        payload.insert("expectedVersion".into(), expected_version.into());
        for field in fields {
            if let Some(value) = object.get(*field) {
                payload.insert((*field).into(), value.clone());
            }
        }
        if payload.len() > 2 {
            result = state
                .registry
                .execute("order", command, serde_json::Value::Object(payload), &ctx)
                .map_err(AppError::from_error_payload)?;
        }
    }

    if let Some(status) = object.get("status").and_then(serde_json::Value::as_str) {
        let command = match status.trim() {
            "submitted" => "submit_order",
            "cancelled" => "cancel_order",
            other => {
                return Err(AppError::BadRequest(format!(
                    "状态 {other} 不能通过通用更新写入; 请使用对应生命周期命令"
                )));
            }
        };
        result = state
            .registry
            .execute(
                "order",
                command,
                serde_json::json!({
                    "orderId": id,
                    "expectedVersion": expected_version,
                    "reason": object.get("reason").and_then(serde_json::Value::as_str).unwrap_or(""),
                }),
                &ctx,
            )
            .map_err(AppError::from_error_payload)?;
    }
    let result = Json(result);

    let order_no = result
        .0
        .get("orderNo")
        .and_then(|v| v.as_str())
        .unwrap_or(&id);
    let _ = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_update",
        "order",
        &id,
        order_no,
        &serde_json::json!({ "orderId": id, "before": before, "after": result.0 }),
        &user,
    );

    Ok(result)
}

async fn delete_order_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    tenant_admin: TenantAdmin,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());

    let before = state
        .registry
        .execute("order", "get_order", serde_json::json!({"id": &id}), &ctx)
        .ok();
    let expected_version = lifecycle_version(&state, &ctx, &id)?;

    let result = state
        .registry
        .execute(
            "order",
            "cancel_order",
            serde_json::json!({"orderId": &id, "expectedVersion": expected_version, "reason": "compatibility delete mapped to cancellation"}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;

    let order_no = before
        .as_ref()
        .and_then(|v| v.get("orderNo"))
        .and_then(|v| v.as_str())
        .unwrap_or(&id);
    let _ = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_cancel_via_delete_compatibility",
        "order",
        &id,
        order_no,
        &serde_json::json!({ "orderId": id, "before": before }),
        &user,
    );

    Ok(result)
}

#[derive(Deserialize)]
struct BulkDeleteBody {
    ids: Option<Vec<String>>,
}

async fn bulk_delete_orders(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Json(body): Json<BulkDeleteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ids = body.ids.unwrap_or_default();
    if ids.is_empty() {
        return Err(AppError::BadRequest("ids 必须是非空数组".to_string()));
    }
    enforce_item_limit("ids", ids.len(), ORDER_BULK_ITEMS_MAX)?;

    let unique_ids: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        ids.into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && seen.insert(s.clone()))
            .collect()
    };
    if unique_ids.is_empty() {
        return Err(AppError::BadRequest("ids 必须是非空数组".to_string()));
    }

    let mut deleted_items: Vec<serde_json::Value> = Vec::new();
    let mut failed_items: Vec<serde_json::Value> = Vec::new();

    for id in &unique_ids {
        let ctx = make_ctx(&user, state.http_client.clone());
        let expected_version = match lifecycle_version(&state, &ctx, id) {
            Ok(version) => version,
            Err(error) => {
                failed_items
                    .push(serde_json::json!({"id": id, "code": 409, "reason": error.to_string()}));
                continue;
            }
        };
        match state.registry.execute(
            "order",
            "cancel_order",
            serde_json::json!({"orderId": id, "expectedVersion": expected_version, "reason": "bulk delete compatibility mapped to cancellation"}),
            &ctx,
        ) {
            Ok(_) => {
                let _ = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
                    "order_cancel_via_bulk_delete_compatibility",
                    "order",
                    id,
                    id,
                    &serde_json::json!({ "orderId": id }),
                    &user,
                );
                deleted_items.push(serde_json::json!({ "id": id }));
            }
            Err(e) => {
                failed_items.push(serde_json::json!({
                    "id": id,
                    "code": 400,
                    "reason": e,
                }));
            }
        }
    }

    Ok(Json(serde_json::json!({
        "ok": failed_items.is_empty(),
        "successCount": deleted_items.len(),
        "failCount": failed_items.len(),
        "deletedItems": deleted_items,
        "failedItems": failed_items,
    })))
}

#[derive(Deserialize)]
struct BatchNotesBody {
    ids: Option<Vec<String>>,
    notes: Option<String>,
}

async fn batch_update_notes(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<BatchNotesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ids = body.ids.unwrap_or_default();
    if ids.is_empty() {
        return Err(AppError::BadRequest("ids 必须是非空数组".to_string()));
    }
    enforce_item_limit("ids", ids.len(), ORDER_BULK_ITEMS_MAX)?;
    let notes = body.notes.unwrap_or_default();

    let mut seen = std::collections::HashSet::new();
    let normalized_ids: Vec<String> = ids
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect();
    if normalized_ids.is_empty() {
        return Err(AppError::BadRequest("ids 必须是非空数组".to_string()));
    }

    let ctx = make_ctx(&user, state.http_client.clone());
    let mut results = Vec::new();
    for id in &normalized_ids {
        let expected_version = match lifecycle_version(&state, &ctx, id) {
            Ok(version) => version,
            Err(error) => {
                results
                    .push(serde_json::json!({"id": id, "ok": false, "error": error.to_string()}));
                continue;
            }
        };
        match state.registry.execute(
            "order",
            "change_notes",
            serde_json::json!({"orderId": id, "expectedVersion": expected_version, "notes": notes}),
            &ctx,
        ) {
            Ok(_) => results.push(serde_json::json!({"id": id, "ok": true})),
            Err(error) => results.push(serde_json::json!({"id": id, "ok": false, "error": error})),
        }
    }
    let updated = results
        .iter()
        .filter(|result| result.get("ok").and_then(serde_json::Value::as_bool) == Some(true))
        .count();
    Ok(Json(
        serde_json::json!({ "updated": updated, "results": results }),
    ))
}

#[derive(Deserialize)]
struct LinkDeviceBody {
    #[serde(alias = "serialNo")]
    serial_no: Option<String>,
}

async fn link_device_to_order_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    tenant_user: TenantUser,
    Json(body): Json<LinkDeviceBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let serial_no = body.serial_no.unwrap_or_default().trim().to_string();
    if serial_no.is_empty() {
        return Err(AppError::BadRequest("缺少字段: serialNo".to_string()));
    }

    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "order",
            "allocate_device",
            serde_json::json!({"orderId": &id, "deviceSerialNo": &serial_no}),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;

    let order = state
        .application_services()
        .order_queries()
        .get_by_id(&ctx, &id)
        .map_err(|error| {
            tracing::error!(
                repository_error_code = error.code(),
                "order audit lookup failed"
            );
            AppError::Internal("order read repository unavailable".into())
        })?;
    let order_no = order
        .as_ref()
        .map(|order| order.order_no.as_str())
        .unwrap_or(&id)
        .to_string();

    if let Err(e) = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_attach_device",
        "order_device",
        result
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        &format!("{} -> {}", order_no, serial_no),
        &serde_json::json!({
            "allocationId": result.get("id"),
            "orderId": id,
            "orderNo": order_no,
            "serialNo": serial_no,
        }),
        &user,
    ) {
        tracing::warn!("Failed to write audit log: {}", e);
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "allocation": result,
    })))
}

async fn detach_device_from_order_handler(
    State(state): State<Arc<AppState>>,
    Path((id, serial_no)): Path<(String, String)>,
    tenant_user: TenantUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "order",
            "release_device",
            serde_json::json!({"orderId": &id, "deviceSerialNo": &serial_no}),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;

    let order = state
        .application_services()
        .order_queries()
        .get_by_id(&ctx, &id)
        .map_err(|error| {
            tracing::error!(
                repository_error_code = error.code(),
                "order audit lookup failed"
            );
            AppError::Internal("order read repository unavailable".into())
        })?;
    let order_no = order
        .as_ref()
        .map(|order| order.order_no.as_str())
        .unwrap_or(&id)
        .to_string();

    if let Err(e) = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_detach_device",
        "order_device",
        result
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        &format!("{} -/-> {}", order_no, serial_no),
        &serde_json::json!({
            "allocationId": result.get("id"),
            "orderId": id,
            "orderNo": order_no,
            "serialNo": serial_no,
        }),
        &user,
    ) {
        tracing::warn!("Failed to write audit log: {}", e);
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SubmitBatchShipBody {
    orders: Option<Vec<BatchShipOrder>>,
}

#[derive(Deserialize)]
struct BatchShipOrder {
    id: Option<String>,
    #[serde(alias = "trackingNo", default)]
    tracking_no: Option<String>,
    #[serde(alias = "deviceSerialNos", default)]
    device_serial_nos: Option<Vec<String>>,
}

async fn submit_batch_ship_handler(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<SubmitBatchShipBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let orders = body.orders.unwrap_or_default();
    if orders.is_empty() {
        return Err(AppError::BadRequest("orders 必须是非空数组".to_string()));
    }
    enforce_item_limit("orders", orders.len(), ORDER_BATCH_SHIP_ORDERS_MAX)?;

    let mut total_device_refs = 0usize;
    for (index, item) in orders.iter().enumerate() {
        let device_count = item.device_serial_nos.as_ref().map_or(0, Vec::len);
        if device_count > ORDER_BATCH_SHIP_DEVICES_PER_ORDER_MAX {
            return Err(AppError::BadRequest(format!(
                "orders[{index}].deviceSerialNos 数量超过上限 {ORDER_BATCH_SHIP_DEVICES_PER_ORDER_MAX}"
            )));
        }
        total_device_refs = total_device_refs
            .checked_add(device_count)
            .ok_or_else(|| AppError::BadRequest("deviceSerialNos 数量溢出".to_string()))?;
        if total_device_refs > ORDER_BATCH_SHIP_DEVICE_REFS_MAX {
            return Err(AppError::BadRequest(format!(
                "批量发货设备引用总数超过上限 {ORDER_BATCH_SHIP_DEVICE_REFS_MAX}"
            )));
        }
    }

    let mut validation_results = Vec::new();
    for item in &orders {
        let order_id = item.id.as_deref().unwrap_or("").trim().to_string();
        let tracking_no = item.tracking_no.as_deref().unwrap_or("").trim().to_string();
        let device_serial_nos = item.device_serial_nos.clone().unwrap_or_default();

        if order_id.is_empty() {
            validation_results
                .push(serde_json::json!({ "orderId": "", "ok": false, "error": "缺少订单 ID" }));
            continue;
        }
        let payload = serde_json::json!({
            "orderId": order_id,
            "trackingNo": tracking_no,
            "deviceSerialNos": device_serial_nos,
        });
        match state
            .registry
            .execute("order", "validate_fixture_dispatch", payload, &ctx)
        {
            Ok(result) => validation_results.push(result),
            Err(error) => validation_results.push(serde_json::json!({
                "orderId": order_id,
                "ok": false,
                "error": error,
            })),
        }
    }

    if validation_results
        .iter()
        .any(|result| result.get("ok").and_then(serde_json::Value::as_bool) != Some(true))
    {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "successCount": 0,
            "failCount": orders.len(),
            "results": validation_results,
            "batchRejected": true,
        })));
    }

    let mut results = Vec::new();
    for item in &orders {
        let order_id = item.id.as_deref().unwrap_or("").trim();
        let payload = serde_json::json!({
            "orderId": order_id,
            "trackingNo": item.tracking_no.as_deref().unwrap_or("").trim(),
            "deviceSerialNos": item.device_serial_nos.clone().unwrap_or_default(),
        });
        match state
            .registry
            .execute("order", "fixture_dispatch", payload, &ctx)
        {
            Ok(result) => results.push(result),
            Err(error) => results.push(serde_json::json!({
                "orderId": order_id,
                "ok": false,
                "error": error,
            })),
        }
    }

    let success_count = results
        .iter()
        .filter(|r| r["ok"].as_bool().unwrap_or(false))
        .count();
    let fail_count = results.len() - success_count;

    if let Err(e) = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_batch_ship",
        "order",
        "",
        &format!("批量发货 {}/{}", success_count, orders.len()),
        &serde_json::json!({
            "totalOrders": orders.len(),
            "successCount": success_count,
            "failCount": fail_count,
            "results": results,
        }),
        &user,
    ) {
        tracing::warn!("Failed to write batch ship audit log: {}", e);
    }

    Ok(Json(serde_json::json!({
        "ok": fail_count == 0,
        "successCount": success_count,
        "failCount": fail_count,
        "results": results,
    })))
}

#[derive(Deserialize)]
struct BatchOrderNosBody {
    ids: Option<Vec<String>>,
}

async fn batch_order_nos_handler(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<BatchOrderNosBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ids = body.ids.unwrap_or_default();
    if ids.is_empty() {
        return Ok(Json(serde_json::json!({ "orders": [] })));
    }
    enforce_item_limit("ids", ids.len(), ORDER_BULK_ITEMS_MAX)?;

    let mut seen = std::collections::HashSet::new();
    let ids: Vec<String> = ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect();
    if ids.is_empty() {
        return Ok(Json(serde_json::json!({ "orders": [] })));
    }

    let ctx = make_ctx(&user, state.http_client.clone());
    let queries = state.application_services().order_queries();
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(order) = queries.get_by_id(&ctx, &id).map_err(|error| {
            tracing::error!(
                repository_error_code = error.code(),
                "batch order-number lookup failed"
            );
            AppError::Internal("order read repository unavailable".into())
        })? {
            rows.push(serde_json::json!({
                "id": order.id,
                "orderNo": order.order_no,
            }));
        }
    }

    Ok(Json(serde_json::json!({ "orders": rows })))
}

const ORDER_NO_KEYS: &[&str] = &["orderNo", "订单号", "orderno", "ORDERNO"];
const START_DATE_KEYS: &[&str] = &["startDate", "开始日期", "STARTDATE"];
const END_DATE_KEYS: &[&str] = &["endDate", "结束日期", "ENDDATE"];
const DELIVERY_DATE_KEYS: &[&str] = &["deliveryDate", "发货日期", "DELIVERYDATE"];
const PICKUP_METHODS_KEYS: &[&str] = &["pickupMethods", "取货方式", "PICKUPMETHODS"];
const ADDRESS_KEYS: &[&str] = &["address", "地址", "ADDRESS"];
const DEVICE_SERIAL_NO_KEYS: &[&str] = &[
    "serialNo",
    "设备序列号",
    "序列号",
    "serial",
    "sn",
    "SERIALNO",
];
const NOTES_KEYS: &[&str] = &["notes", "备注", "NOTES"];

fn valid_import_order_no(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

fn lifecycle_version(
    state: &AppState,
    ctx: &system_core::ExecutionContext,
    order_id: &str,
) -> Result<i64, AppError> {
    state
        .registry
        .execute(
            "order_lifecycle_v2",
            "get_lifecycle",
            serde_json::json!({"orderId": order_id}),
            ctx,
        )
        .map_err(AppError::from_error_payload)?
        .get("lifecycle")
        .and_then(|lifecycle| lifecycle.get("version"))
        .and_then(serde_json::Value::as_i64)
        .filter(|version| *version > 0)
        .ok_or_else(|| AppError::Conflict("订单生命周期版本不可用".to_string()))
}

fn find_order_by_order_no(
    state: &AppState,
    ctx: &system_core::ExecutionContext,
    order_no: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let result = state
        .registry
        .execute(
            "order_read_compatibility",
            "list_orders",
            serde_json::json!({
                "page": 1,
                "pageSize": 2,
                "filter": { "orderNo": order_no },
            }),
            ctx,
        )
        .map_err(AppError::from_error_payload)?;
    let users = result
        .get("users")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if users.len() > 1 {
        return Err(AppError::Conflict(
            "订单号匹配不唯一，导入已失败关闭".to_string(),
        ));
    }
    Ok(users.into_iter().next())
}

fn parse_pickup_methods_from_import(value: &str) -> Vec<String> {
    let text = value.trim();
    if text.is_empty() {
        return vec![];
    }

    if text.starts_with('[')
        && text.ends_with(']')
        && let Ok(arr) = serde_json::from_str::<Vec<String>>(text)
    {
        return arr
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    text.split(&[',', ',', ';', ';', '|', '\u{3001}'][..])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

async fn import_orders(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let upload = multipart_import::parse_excel_import_multipart(&mut multipart, true).await?;
    let approved_execution = upload.approved_execution;
    let parsed = parse_excel_rows_from_buffer(&upload.file_data, &upload.file_name)?;
    enforce_item_limit("导入行", parsed.rows.len(), ORDER_IMPORT_ROWS_MAX)?;

    let required = vec![
        RequiredHeaderGroup {
            name: "orderNo".to_string(),
            aliases: ORDER_NO_KEYS.iter().map(|s| s.to_string()).collect(),
        },
        RequiredHeaderGroup {
            name: "startDate".to_string(),
            aliases: START_DATE_KEYS.iter().map(|s| s.to_string()).collect(),
        },
        RequiredHeaderGroup {
            name: "endDate".to_string(),
            aliases: END_DATE_KEYS.iter().map(|s| s.to_string()).collect(),
        },
        RequiredHeaderGroup {
            name: "deliveryDate".to_string(),
            aliases: DELIVERY_DATE_KEYS.iter().map(|s| s.to_string()).collect(),
        },
        RequiredHeaderGroup {
            name: "pickupMethods".to_string(),
            aliases: PICKUP_METHODS_KEYS.iter().map(|s| s.to_string()).collect(),
        },
    ];
    validate_header_groups(&parsed.headers, &required)?;

    let mut summary = create_import_summary(false);
    let mut file_seen_order_nos = std::collections::HashSet::new();

    for (idx, row) in parsed.rows.iter().enumerate() {
        let row_no = idx + 2;
        let order_no = get_field_value(&parsed.headers, row, ORDER_NO_KEYS);
        if order_no.is_empty() {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": "", "reason": "缺少 orderNo" }),
            );
            continue;
        }
        if !valid_import_order_no(&order_no) {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "订单号只能为纯数字" }),
            );
            continue;
        }
        if file_seen_order_nos.contains(&order_no) {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "同一文件内 orderNo 重复" }),
            );
            continue;
        }

        let start_date_raw = get_raw_field_value_opt(&parsed.headers, row, START_DATE_KEYS);
        let end_date_raw = get_raw_field_value_opt(&parsed.headers, row, END_DATE_KEYS);
        let delivery_date_raw = get_raw_field_value_opt(&parsed.headers, row, DELIVERY_DATE_KEYS);

        let start_date = match normalize_optional_business_date(start_date_raw) {
            Ok(d) => d,
            Err(_) => {
                push_import_failure(
                    &mut summary,
                    serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "startDate 格式无效" }),
                );
                continue;
            }
        };
        let end_date = match normalize_optional_business_date(end_date_raw) {
            Ok(d) => d,
            Err(_) => {
                push_import_failure(
                    &mut summary,
                    serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "endDate 格式无效" }),
                );
                continue;
            }
        };
        let delivery_date = match normalize_optional_business_date(delivery_date_raw) {
            Ok(d) => d,
            Err(_) => {
                push_import_failure(
                    &mut summary,
                    serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "deliveryDate 格式无效" }),
                );
                continue;
            }
        };

        if start_date > end_date {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "startDate 不能晚于 endDate" }),
            );
            continue;
        }

        let pickup_methods_raw = get_field_value(&parsed.headers, row, PICKUP_METHODS_KEYS);
        let pickup_methods = parse_pickup_methods_from_import(&pickup_methods_raw);
        if pickup_methods.is_empty() {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "pickupMethods 不能为空" }),
            );
            continue;
        }

        use crate::utils::constants::PICKUP_METHODS;
        let invalid_method = pickup_methods
            .iter()
            .find(|m| !PICKUP_METHODS.contains(&m.as_str()));
        if let Some(inv) = invalid_method {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": format!("取货方式无效: {}", inv) }),
            );
            continue;
        }

        let address = get_field_value(&parsed.headers, row, ADDRESS_KEYS);
        let notes = get_field_value(&parsed.headers, row, NOTES_KEYS);

        let payload = serde_json::json!({
            "orderNo": order_no,
            "startDate": start_date,
            "endDate": end_date,
            "deliveryDate": delivery_date,
            "pickupMethods": pickup_methods,
            "address": address,
            "notes": notes,
        });

        if !approved_execution {
            file_seen_order_nos.insert(order_no.clone());
            push_import_success(
                &mut summary,
                Some(serde_json::json!({"rowNo": row_no, "orderNo": order_no, "validated": true})),
            );
            continue;
        }

        match state
            .registry
            .execute("order", "import_order", payload, &ctx)
        {
            Ok(result) => {
                if result.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
                    file_seen_order_nos.insert(order_no.clone());
                    push_import_success(&mut summary, None);
                } else {
                    push_import_failure(
                        &mut summary,
                        serde_json::json!({
                            "rowNo": row_no,
                            "orderNo": order_no,
                            "reason": result.get("error").and_then(serde_json::Value::as_str).unwrap_or("创建失败"),
                        }),
                    );
                }
            }
            Err(e) => {
                push_import_failure(
                    &mut summary,
                    serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": e }),
                );
            }
        }
    }

    if let Err(e) = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_import_orders",
        "import",
        "",
        "orders/import",
        &serde_json::json!({
            "totalRows": parsed.rows.len(),
            "successCount": summary.success_count,
            "failCount": summary.fail_count,
            "failedRows": &summary.failed_rows[..std::cmp::min(50, summary.failed_rows.len())],
        }),
        &user,
    ) {
        tracing::warn!("Failed to write audit log: {}", e);
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "mode": if approved_execution { "approved_execution" } else { "dry_run" },
        "approved": approved_execution,
        "successCount": summary.success_count,
        "failCount": summary.fail_count,
        "failedRows": summary.failed_rows,
    })))
}

async fn import_devices_by_orderno(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = tenant_user.1;
    let ctx = make_ctx(&user, state.http_client.clone());
    let upload = multipart_import::parse_excel_import_multipart(&mut multipart, true).await?;
    let approved_execution = upload.approved_execution;
    let parsed = parse_excel_rows_from_buffer(&upload.file_data, &upload.file_name)?;
    enforce_item_limit("导入行", parsed.rows.len(), ORDER_IMPORT_ROWS_MAX)?;

    let required = vec![
        RequiredHeaderGroup {
            name: "orderNo".to_string(),
            aliases: ORDER_NO_KEYS.iter().map(|s| s.to_string()).collect(),
        },
        RequiredHeaderGroup {
            name: "serialNo".to_string(),
            aliases: DEVICE_SERIAL_NO_KEYS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        },
    ];
    validate_header_groups(&parsed.headers, &required)?;

    let device_candidate_queries = state.application_services().device_candidates();
    let device_candidates = device_candidate_queries
        .list_for_import(&ctx)
        .map_err(|error| match error {
            DeviceCandidateQueryError::TooManyCandidates { max } => {
                AppError::BadRequest(format!("当前租户设备匹配候选超过安全上限 {max}"))
            }
            DeviceCandidateQueryError::Repository(error) => {
                tracing::error!(
                    repository_error_code = error.code(),
                    "device import candidate repository unavailable"
                );
                AppError::Internal("device import candidate repository unavailable".into())
            }
        })?;

    let mut failed_rows: Vec<serde_json::Value> = Vec::new();
    let mut validated_rows: Vec<serde_json::Value> = Vec::new();

    for (idx, row) in parsed.rows.iter().enumerate() {
        let row_no = idx + 2;
        let order_no = get_field_value(&parsed.headers, row, ORDER_NO_KEYS);
        let input_serial_no = get_field_value(&parsed.headers, row, DEVICE_SERIAL_NO_KEYS);

        if order_no.is_empty() || input_serial_no.is_empty() {
            failed_rows.push(serde_json::json!({
                "rowNo": row_no,
                "orderNo": order_no,
                "serialNo": input_serial_no,
                "inputSerialNo": input_serial_no,
                "matchedSerialNo": "",
                "reason": "缺少 orderNo 或 serialNo",
            }));
            continue;
        }
        if !valid_import_order_no(&order_no) {
            failed_rows.push(serde_json::json!({
                "rowNo": row_no,
                "orderNo": order_no,
                "serialNo": input_serial_no,
                "inputSerialNo": input_serial_no,
                "matchedSerialNo": "",
                "reason": "订单号只能为纯数字",
            }));
            continue;
        }

        let match_result =
            device_candidate_queries.resolve_for_import(&input_serial_no, &device_candidates);
        if !match_result.ok {
            failed_rows.push(serde_json::json!({
                "rowNo": row_no,
                "orderNo": order_no,
                "serialNo": input_serial_no,
                "inputSerialNo": input_serial_no,
                "matchedSerialNo": match_result.matched_serial_no,
                "reason": match_result.reason,
            }));
            continue;
        }

        let matched_sn = match_result.matched_serial_no.clone();
        let order = find_order_by_order_no(&state, &ctx, &order_no)?;
        let order = match order {
            Some(o) => o,
            None => {
                failed_rows.push(serde_json::json!({
                    "rowNo": row_no,
                    "orderNo": order_no,
                    "serialNo": input_serial_no,
                    "inputSerialNo": input_serial_no,
                    "matchedSerialNo": matched_sn,
                    "reason": "订单号不存在",
                }));
                continue;
            }
        };

        let order_id = order["id"].as_str().unwrap_or("");
        let validation = state.registry.execute(
            "order",
            "validate_allocate_device",
            serde_json::json!({"orderId": order_id, "deviceSerialNo": matched_sn}),
            &ctx,
        );
        if let Err(reason) = validation {
            failed_rows.push(serde_json::json!({
                "rowNo": row_no,
                "orderNo": order_no,
                "serialNo": input_serial_no,
                "inputSerialNo": input_serial_no,
                "matchedSerialNo": matched_sn,
                "reason": reason,
            }));
            continue;
        }

        validated_rows.push(serde_json::json!({
            "rowNo": row_no,
            "orderId": order_id,
            "orderNo": order_no,
            "matchedSerialNo": matched_sn,
        }));
    }

    if !failed_rows.is_empty() {
        if let Err(e) = audit_service::write_audit_log_with_repository(
            state.audit_compatibility_repository(),
            "order_import_devices",
            "import",
            "",
            "orders/import-devices",
            &serde_json::json!({
                "totalRows": parsed.rows.len(),
                "successCount": 0,
                "failCount": failed_rows.len(),
                "batchRejected": true,
                "message": "预检查未通过，整批未写入",
                "failedRows": &failed_rows[..std::cmp::min(50, failed_rows.len())],
            }),
            &user,
        ) {
            tracing::warn!("Failed to write audit log: {}", e);
        }

        return Ok(Json(serde_json::json!({
            "ok": false,
            "mode": if approved_execution { "approved_execution" } else { "dry_run" },
            "approved": approved_execution,
            "message": "预检查未通过，整批未写入",
            "successCount": 0,
            "failCount": parsed.rows.len(),
            "successRows": [],
            "failedRows": failed_rows,
        })));
    }

    if !approved_execution {
        if let Err(e) = audit_service::write_audit_log_with_repository(
            state.audit_compatibility_repository(),
            "order_import_devices_dry_run",
            "import",
            "",
            "orders/import-devices",
            &serde_json::json!({
                "totalRows": parsed.rows.len(),
                "validatedRows": &validated_rows[..std::cmp::min(50, validated_rows.len())],
            }),
            &user,
        ) {
            tracing::warn!("Failed to write dry-run audit log: {}", e);
        }
        return Ok(Json(serde_json::json!({
            "ok": true,
            "mode": "dry_run",
            "approved": false,
            "message": "预检查通过，dry-run 未写入",
            "successCount": validated_rows.len(),
            "failCount": 0,
            "successRows": validated_rows,
            "failedRows": [],
        })));
    }

    let batch_rows: Vec<_> = validated_rows
        .iter()
        .map(|item| {
            serde_json::json!({
                "rowNo": item["rowNo"],
                "orderId": item["orderId"],
                "deviceSerialNo": item["matchedSerialNo"],
            })
        })
        .collect();
    if let Err(e) = state.registry.execute(
        "order",
        "allocate_devices_batch",
        serde_json::json!({"rows": batch_rows}),
        &ctx,
    ) {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "mode": "approved_execution",
            "approved": true,
            "message": "写入阶段失败，整批已回滚",
            "batchRejected": true,
            "rolledBack": true,
            "successCount": 0,
            "failCount": parsed.rows.len(),
            "successRows": [],
            "failedRows": [{
                "reason": e,
            }],
        })));
    }

    let success_rows: Vec<_> = validated_rows
        .iter()
        .map(|item| {
            serde_json::json!({
                "rowNo": item["rowNo"],
                "orderNo": item["orderNo"],
                "matchedSerialNo": item["matchedSerialNo"],
            })
        })
        .collect();

    if let Err(e) = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_import_devices",
        "import",
        "",
        "orders/import-devices",
        &serde_json::json!({
            "totalRows": parsed.rows.len(),
            "successCount": success_rows.len(),
            "failCount": 0,
            "batchRejected": false,
            "message": "",
            "successRows": &success_rows[..std::cmp::min(50, success_rows.len())],
        }),
        &user,
    ) {
        tracing::warn!("Failed to write audit log: {}", e);
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "mode": "approved_execution",
        "approved": true,
        "message": "",
        "successCount": success_rows.len(),
        "failCount": 0,
        "successRows": success_rows,
        "failedRows": [],
    })))
}

async fn import_notes_by_orderno(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let upload = multipart_import::parse_excel_import_multipart(&mut multipart, true).await?;
    let approved_execution = upload.approved_execution;
    let parsed = parse_excel_rows_from_buffer(&upload.file_data, &upload.file_name)?;
    enforce_item_limit("导入行", parsed.rows.len(), ORDER_IMPORT_ROWS_MAX)?;

    let required = vec![RequiredHeaderGroup {
        name: "orderNo".to_string(),
        aliases: ORDER_NO_KEYS.iter().map(|s| s.to_string()).collect(),
    }];
    validate_header_groups(&parsed.headers, &required)?;

    let mut summary = create_import_summary(false);

    for (idx, row) in parsed.rows.iter().enumerate() {
        let row_no = idx + 2;
        let order_no = get_field_value(&parsed.headers, row, ORDER_NO_KEYS);

        if order_no.is_empty() {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": "", "reason": "缺少 orderNo" }),
            );
            continue;
        }
        if !valid_import_order_no(&order_no) {
            push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "订单号只能为纯数字" }),
            );
            continue;
        }

        let order = find_order_by_order_no(&state, &ctx, &order_no)?;
        let order = match order {
            Some(o) => o,
            None => {
                push_import_failure(
                    &mut summary,
                    serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": "订单号不存在" }),
                );
                continue;
            }
        };

        let notes = get_field_value(&parsed.headers, row, NOTES_KEYS);
        let order_id = order["id"].as_str().unwrap_or("");
        let expected_version = match lifecycle_version(&state, &ctx, order_id) {
            Ok(version) => version,
            Err(error) => {
                push_import_failure(
                    &mut summary,
                    serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": error.to_string() }),
                );
                continue;
            }
        };

        if !approved_execution {
            push_import_success(
                &mut summary,
                Some(serde_json::json!({"rowNo": row_no, "orderNo": order_no, "validated": true})),
            );
            continue;
        }

        match state.registry.execute(
            "order",
            "change_notes",
            serde_json::json!({"orderId": order_id, "expectedVersion": expected_version, "notes": notes}),
            &ctx,
        ) {
            Ok(_) => push_import_success(&mut summary, None),
            Err(error) => push_import_failure(
                &mut summary,
                serde_json::json!({ "rowNo": row_no, "orderNo": order_no, "reason": error }),
            ),
        }
    }

    if let Err(e) = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_import_notes",
        "import",
        "",
        "orders/import-notes",
        &serde_json::json!({
            "totalRows": parsed.rows.len(),
            "successCount": summary.success_count,
            "failCount": summary.fail_count,
            "failedRows": &summary.failed_rows[..std::cmp::min(50, summary.failed_rows.len())],
        }),
        &user,
    ) {
        tracing::warn!("Failed to write audit log: {}", e);
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "mode": if approved_execution { "approved_execution" } else { "dry_run" },
        "approved": approved_execution,
        "successCount": summary.success_count,
        "failCount": summary.fail_count,
        "failedRows": summary.failed_rows,
    })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ExportQuery {
    keyword: Option<String>,
    #[serde(alias = "orderNo")]
    order_no: Option<String>,
    address: Option<String>,
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
    fields: Option<Vec<String>>,
    #[serde(alias = "exportFields")]
    export_fields: Option<Vec<String>>,
    ids: Option<Vec<String>>,
}

async fn export_orders(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(q): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let orders = if let Some(ref ids) = q.ids {
        enforce_item_limit("ids", ids.len(), ORDER_EXPORT_IDS_MAX)?;
        if !ids.is_empty() {
            order_export_service::load_orders_by_ids(&state, &ctx, ids, ORDER_EXPORT_IDS_MAX)?
        } else {
            order_export_service::load_filtered_orders(
                &state,
                &ctx,
                build_export_filter(&q),
                ORDER_EXPORT_ROWS_MAX,
            )?
        }
    } else {
        order_export_service::load_filtered_orders(
            &state,
            &ctx,
            build_export_filter(&q),
            ORDER_EXPORT_ROWS_MAX,
        )?
    };

    let field_keys = q.fields.or(q.export_fields).unwrap_or_default();
    let buffer = order_compatibility_support::build_export_workbook(&orders, &field_keys)?;
    let file_name = order_compatibility_support::format_export_file_name();

    Ok(http_utils::binary_response(
        StatusCode::OK,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &file_name,
        buffer,
    ))
}

async fn export_orders_post(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<ExportQuery>,
) -> Result<Response, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let orders = if let Some(ref ids) = body.ids {
        enforce_item_limit("ids", ids.len(), ORDER_EXPORT_IDS_MAX)?;
        if !ids.is_empty() {
            order_export_service::load_orders_by_ids(&state, &ctx, ids, ORDER_EXPORT_IDS_MAX)?
        } else {
            order_export_service::load_filtered_orders(
                &state,
                &ctx,
                build_export_filter(&body),
                ORDER_EXPORT_ROWS_MAX,
            )?
        }
    } else {
        order_export_service::load_filtered_orders(
            &state,
            &ctx,
            build_export_filter(&body),
            ORDER_EXPORT_ROWS_MAX,
        )?
    };

    let field_keys = body.fields.or(body.export_fields).unwrap_or_default();
    let buffer = order_compatibility_support::build_export_workbook(&orders, &field_keys)?;
    let file_name = order_compatibility_support::format_export_file_name();

    Ok(http_utils::binary_response(
        StatusCode::OK,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &file_name,
        buffer,
    ))
}

fn build_export_filter(q: &ExportQuery) -> serde_json::Value {
    serde_json::json!({
        "keyword": q.keyword.as_deref().unwrap_or(""),
        "orderNo": q.order_no.as_deref().unwrap_or(""),
        "address": q.address.as_deref().unwrap_or(""),
        "startDateFrom": q.start_date_from.as_deref().unwrap_or(""),
        "startDateTo": q.start_date_to.as_deref().unwrap_or(""),
        "startDate": q.start_date.as_deref().unwrap_or(""),
        "endDateFrom": q.end_date_from.as_deref().unwrap_or(""),
        "endDateTo": q.end_date_to.as_deref().unwrap_or(""),
        "endDate": q.end_date.as_deref().unwrap_or(""),
        "includedDate": q.included_date.as_deref().unwrap_or(""),
        "deliveryDateFrom": q.delivery_date_from.as_deref().unwrap_or(""),
        "deliveryDateTo": q.delivery_date_to.as_deref().unwrap_or(""),
        "deliveryDate": q.delivery_date.as_deref().unwrap_or(""),
        "pickupMethods": q.pickup_methods.as_deref().unwrap_or(&[]),
    })
}

#[derive(Deserialize)]
struct BulkUpdateBody {
    ids: Option<Vec<String>>,
    status: Option<String>,
    #[serde(alias = "pickupMethods")]
    pickup_methods: Option<Vec<String>>,
    notes: Option<String>,
}

async fn bulk_update_orders(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<BulkUpdateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let raw_ids = body.ids.unwrap_or_default();
    if raw_ids.is_empty() {
        return Err(AppError::BadRequest("ids 必须是非空数组".to_string()));
    }
    enforce_item_limit("ids", raw_ids.len(), ORDER_BULK_ITEMS_MAX)?;

    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let ids: Vec<String> = raw_ids
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect();
    if ids.is_empty() {
        return Err(AppError::BadRequest("ids 无有效值".to_string()));
    }

    let ctx = make_ctx(&user, state.http_client.clone());
    let mut updates = serde_json::Map::new();
    let mut results = Vec::new();

    if let Some(ref status) = body.status {
        let command = match status.as_str() {
            "submitted" => "submit_order",
            "cancelled" => "cancel_order",
            other => {
                return Err(AppError::BadRequest(format!(
                    "批量状态 {other} 不允许直接写入; 仅支持 submitted / cancelled 命名命令"
                )));
            }
        };
        let mut count = 0_u64;
        for id in &ids {
            let expected_version = match lifecycle_version(&state, &ctx, id) {
                Ok(version) => version,
                Err(error) => {
                    results.push(serde_json::json!({"id": id, "field": "status", "ok": false, "error": error.to_string()}));
                    continue;
                }
            };
            match state.registry.execute(
                "order",
                command,
                serde_json::json!({"orderId": id, "expectedVersion": expected_version, "reason": "bulk compatibility command"}),
                &ctx,
            ) {
                Ok(_) => {
                    count += 1;
                    results.push(serde_json::json!({"id": id, "field": "status", "ok": true}));
                }
                Err(error) => results.push(
                    serde_json::json!({"id": id, "field": "status", "ok": false, "error": error}),
                ),
            }
        }
        updates.insert(
            "status".to_string(),
            serde_json::Value::Number(count.into()),
        );
    }

    if let Some(ref methods) = body.pickup_methods {
        if methods.is_empty() {
            return Err(AppError::BadRequest(
                "pickupMethods 必须是非空数组".to_string(),
            ));
        }
        let mut n = 0_u64;
        for id in &ids {
            let expected_version = match lifecycle_version(&state, &ctx, id) {
                Ok(version) => version,
                Err(error) => {
                    results.push(serde_json::json!({"id": id, "field": "pickupMethods", "ok": false, "error": error.to_string()}));
                    continue;
                }
            };
            match state.registry.execute(
                "order",
                "change_draft_address",
                serde_json::json!({"orderId": id, "expectedVersion": expected_version, "pickupMethods": methods}),
                &ctx,
            ) {
                Ok(_) => {
                    n += 1;
                    results.push(serde_json::json!({"id": id, "field": "pickupMethods", "ok": true}));
                }
                Err(error) => results.push(serde_json::json!({"id": id, "field": "pickupMethods", "ok": false, "error": error})),
            }
        }
        updates.insert(
            "pickupMethods".to_string(),
            serde_json::Value::Number(n.into()),
        );
    }

    if let Some(ref notes) = body.notes {
        let mut n = 0_u64;
        for id in &ids {
            let expected_version = match lifecycle_version(&state, &ctx, id) {
                Ok(version) => version,
                Err(error) => {
                    results.push(serde_json::json!({"id": id, "field": "notes", "ok": false, "error": error.to_string()}));
                    continue;
                }
            };
            match state.registry.execute(
                "order",
                "change_notes",
                serde_json::json!({"orderId": id, "expectedVersion": expected_version, "notes": notes}),
                &ctx,
            ) {
                Ok(_) => {
                    n += 1;
                    results.push(serde_json::json!({"id": id, "field": "notes", "ok": true}));
                }
                Err(error) => results.push(
                    serde_json::json!({"id": id, "field": "notes", "ok": false, "error": error}),
                ),
            }
        }
        updates.insert("notes".to_string(), serde_json::Value::Number(n.into()));
    }

    let _ = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "order_bulk_update",
        "order",
        "",
        "",
        &serde_json::json!({
            "ids": ids,
            "status": body.status,
            "pickupMethods": body.pickup_methods,
            "hasNotes": body.notes.is_some(),
        }),
        &user,
    );

    let ok = results
        .iter()
        .all(|result| result.get("ok").and_then(serde_json::Value::as_bool) == Some(true));
    Ok(Json(
        serde_json::json!({ "ok": ok, "updates": updates, "results": results }),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionBody {
    order_id: String,
    to_status: String,
    #[serde(default)]
    reason: String,
}

async fn transition_order(
    tenant_user: TenantUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<TransitionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let payload = serde_json::json!({
        "orderId": body.order_id,
        "toStatus": body.to_status,
        "reason": body.reason,
    });

    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute("order", "transition", payload, &ctx)
        .map_err(AppError::BadRequest)?;

    Ok(Json(result))
}
