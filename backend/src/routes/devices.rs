use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::application::{
    DeviceImportCandidate, DeviceMutationAuthorityError, DeviceReadAuthorityError,
};
use crate::error::AppError;
use crate::middleware::tenant_extractors::{TenantAdmin, TenantUser};
use crate::registry::make_ctx;
use crate::services::{audit_service, excel_import_service, multipart_import};
use crate::state::AppState;

const DEVICE_BULK_ITEMS_MAX: usize = 500;

pub fn device_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/devices", get(list_devices).post(create_device))
        .route(
            "/devices/{serialNo}",
            get(get_device).put(update_device).delete(delete_device),
        )
        .route("/devices/checkin-scan", post(checkin_scan))
        .route("/devices/checkin-undo", post(undo_checkin))
        .route("/devices/bulk-delete", post(bulk_delete))
        .route("/devices/bulk-update", post(bulk_update))
        .route("/devices/export", post(export_devices))
        .route("/devices/import-excel", post(import_excel))
}

// ── Query params ────────────────────────────────────────────────

#[derive(Deserialize)]
struct DeviceListQuery {
    keyword: Option<String>,
    #[serde(rename = "rentalStatus")]
    rental_status: Option<String>,
    notes: Option<String>,
    #[serde(rename = "warningStatus")]
    warning_status: Option<String>,
    page: Option<u32>,
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
}

// ── Handlers ────────────────────────────────────────────────────

async fn list_devices(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Query(params): Query<DeviceListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let has_page = params.page.is_some() || params.page_size.is_some();
    if has_page {
        let ctx = make_ctx(&user, state.http_client.clone());
        let result = state
            .application_services()
            .device_reads()
            .paged(
                &ctx,
                params.keyword.as_deref(),
                params.rental_status.as_deref(),
                params.notes.as_deref(),
                params.warning_status.as_deref(),
                params.page,
                params.page_size,
            )
            .map_err(device_read_authority_error)?;
        return Ok(Json(serde_json::to_value(result)?));
    }

    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .device_reads()
        .legacy_unpaged(
            &ctx,
            params.rental_status.as_deref(),
            params.keyword.as_deref(),
            params.notes.as_deref(),
            params.warning_status.as_deref(),
        )
        .map_err(device_read_authority_error)?;
    Ok(Json(serde_json::to_value(result)?))
}

async fn get_device(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Path(serial_no): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "device",
            "get_device",
            serde_json::json!({"serialNo": serial_no}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

async fn create_device(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute("device", "create_device", body, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

async fn update_device(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Path(serial_no): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let mut payload = body.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("serialNo".to_string(), serde_json::Value::String(serial_no));
    }
    let result = state
        .registry
        .execute("device", "update_device", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

async fn delete_device(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Path(serial_no): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "device",
            "delete_device",
            serde_json::json!({"serialNo": serial_no}),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(result)
}

#[derive(Deserialize)]
struct CheckinScanBody {
    #[serde(rename = "serialNo")]
    serial_no: Option<String>,
}

async fn checkin_scan(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<CheckinScanBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let serial_no = body.serial_no.unwrap_or_default().trim().to_string();
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .device_mutations()
        .checkin(&ctx, &serial_no)
        .map_err(device_mutation_authority_error)?;

    if let Some(auto_orders) = result
        .get("autoCompletedOrders")
        .and_then(serde_json::Value::as_array)
    {
        for order in auto_orders {
            let order_id = order
                .get("orderId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let reason = order
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let triggered_by = order
                .get("triggeredBySerialNo")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let _ = audit_service::write_audit_log_with_repository(
                state.audit_compatibility_repository(),
                "order_auto_completed",
                "order",
                order_id,
                "",
                &serde_json::json!({
                    "orderId": order_id,
                    "reason": reason,
                    "triggeredBySerialNo": triggered_by,
                }),
                &user,
            );
        }
    }

    let mut payload = result
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::json!({ "ok": false }));
    if payload.get("device").is_some() {
        let serials = vec![serial_no.clone()];
        if let Some(device) = state
            .application_services()
            .device_reads()
            .export_by_serials(&ctx, &serials, 1)
            .map_err(device_read_authority_error)?
            .into_iter()
            .next()
        {
            payload["device"] = serde_json::to_value(device)?;
        }
    }

    Ok(Json(payload))
}

#[derive(Deserialize)]
struct UndoCheckinBody {
    #[serde(rename = "serialNo")]
    serial_no: Option<String>,
    #[serde(rename = "previousStatus")]
    previous_status: Option<String>,
}

async fn undo_checkin(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<UndoCheckinBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let serial_no = body.serial_no.unwrap_or_default().trim().to_string();
    let previous_status = body.previous_status.unwrap_or_default().trim().to_string();
    let ctx = make_ctx(&user, state.http_client.clone());
    state
        .application_services()
        .device_mutations()
        .undo_checkin(&ctx, &serial_no, &previous_status)
        .map_err(device_mutation_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct BulkDeleteBody {
    #[serde(rename = "serialNos")]
    serial_nos: Option<Vec<String>>,
}

async fn bulk_delete(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Json(body): Json<BulkDeleteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let raw = body.serial_nos.unwrap_or_default();
    if raw.is_empty() {
        return Err(AppError::BadRequest("serialNos 必须是非空数组".to_string()));
    }
    if raw.len() > DEVICE_BULK_ITEMS_MAX {
        return Err(AppError::BadRequest(format!(
            "serialNos 数量超过上限 {DEVICE_BULK_ITEMS_MAX}"
        )));
    }

    let mut seen = std::collections::HashSet::new();
    let serial_nos: Vec<String> = raw
        .iter()
        .map(|x| x.trim().to_string())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect();

    if serial_nos.is_empty() {
        return Err(AppError::BadRequest("serialNos 必须是非空数组".to_string()));
    }

    let mut deleted_items: Vec<serde_json::Value> = Vec::new();
    let mut failed_items: Vec<serde_json::Value> = Vec::new();

    for serial_no in &serial_nos {
        match state.registry.execute(
            "device",
            "delete_device",
            serde_json::json!({ "serialNo": serial_no }),
            &ctx,
        ) {
            Ok(_) => {
                deleted_items.push(serde_json::json!({ "serialNo": serial_no }));
            }
            Err(raw_error) => {
                let error = AppError::from_error_payload(raw_error);
                let (code, msg) = match &error {
                    AppError::NotFound(_) => (404, error.to_string()),
                    _ => (400, error.to_string()),
                };
                failed_items.push(serde_json::json!({
                    "serialNo": serial_no,
                    "code": code,
                    "reason": msg,
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
struct BulkUpdateBody {
    #[serde(rename = "serialNos")]
    serial_nos: Option<Vec<String>>,
    updates: Option<serde_json::Value>,
}

async fn bulk_update(
    State(state): State<Arc<AppState>>,
    tenant_admin: TenantAdmin,
    Json(body): Json<BulkUpdateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_admin.0, tenant_admin.1);
    let raw = body.serial_nos.unwrap_or_default();
    if raw.is_empty() {
        return Err(AppError::BadRequest("serialNos 必须是非空数组".to_string()));
    }
    if raw.len() > DEVICE_BULK_ITEMS_MAX {
        return Err(AppError::BadRequest(format!(
            "serialNos 数量超过上限 {DEVICE_BULK_ITEMS_MAX}"
        )));
    }

    let mut seen = std::collections::HashSet::new();
    let serial_nos: Vec<String> = raw
        .iter()
        .map(|x| x.trim().to_string())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect();

    if serial_nos.is_empty() {
        return Err(AppError::BadRequest("serialNos 必须是非空数组".to_string()));
    }

    let updates = body.updates.unwrap_or(serde_json::json!({}));
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .device_mutations()
        .bulk_update(&ctx, &serial_nos, &updates)
        .map_err(device_mutation_authority_error)?;

    Ok(Json(serde_json::json!({
        "ok": result.fail_count == 0,
        "updatedCount": result.updated_count,
        "failCount": result.fail_count,
        "updatedItems": result.items,
        "failedItems": result.failed,
    })))
}

#[derive(Deserialize)]
struct ExportBody {
    #[serde(rename = "serialNos")]
    serial_nos: Option<Vec<String>>,
    keyword: Option<String>,
    #[serde(rename = "rentalStatus")]
    rental_status: Option<String>,
    notes: Option<String>,
    #[serde(rename = "warningStatus")]
    warning_status: Option<String>,
}

async fn export_devices(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    Json(body): Json<ExportBody>,
) -> Result<Response, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let ctx = make_ctx(&user, state.http_client.clone());
    let device_reads = state.application_services().device_reads();
    let devices = if let Some(ref sns) = body.serial_nos {
        if sns.len() > DEVICE_BULK_ITEMS_MAX {
            return Err(AppError::BadRequest(format!(
                "serialNos 数量超过上限 {DEVICE_BULK_ITEMS_MAX}"
            )));
        }
        if !sns.is_empty() {
            device_reads
                .export_by_serials(&ctx, sns, DEVICE_BULK_ITEMS_MAX)
                .map_err(device_read_authority_error)?
        } else {
            device_reads
                .export_filtered(
                    &ctx,
                    body.keyword.as_deref(),
                    body.rental_status.as_deref(),
                    body.notes.as_deref(),
                    body.warning_status.as_deref(),
                )
                .map_err(device_read_authority_error)?
        }
    } else {
        device_reads
            .export_filtered(
                &ctx,
                body.keyword.as_deref(),
                body.rental_status.as_deref(),
                body.notes.as_deref(),
                body.warning_status.as_deref(),
            )
            .map_err(device_read_authority_error)?
    };

    let buffer = device_reads
        .build_export_workbook(&devices)
        .map_err(device_read_authority_error)?;
    let file_name = device_reads.export_file_name();

    let mut res = Response::new(axum::body::Body::from(buffer));
    *res.status_mut() = StatusCode::OK;
    res.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
    );
    res.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", file_name))
            .unwrap(),
    );
    Ok(res)
}

/// POST /devices/import-excel — import devices from Excel
async fn import_excel(
    State(state): State<Arc<AppState>>,
    tenant_user: TenantUser,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_tenant, user) = (tenant_user.0, tenant_user.1);
    let upload = multipart_import::parse_excel_import_multipart(&mut multipart, false).await?;
    let parsed =
        excel_import_service::parse_device_import_rows(&upload.file_data, &upload.file_name)?;
    let candidates = parsed
        .rows
        .into_iter()
        .map(|row| DeviceImportCandidate {
            row_no: row.row_no,
            serial_no: row.serial_no,
            notes: row.notes,
        })
        .collect();
    let ctx = make_ctx(&user, state.http_client.clone());
    let result = state
        .application_services()
        .device_mutations()
        .import_devices(&ctx, candidates, parsed.failed_rows)
        .map_err(device_mutation_authority_error)?;

    Ok(Json(serde_json::to_value(result)?))
}

fn device_mutation_authority_error(error: DeviceMutationAuthorityError) -> AppError {
    match error {
        DeviceMutationAuthorityError::InvalidInput(message) => AppError::BadRequest(message),
        DeviceMutationAuthorityError::NotFound(message) => AppError::NotFound(message),
        DeviceMutationAuthorityError::Persistence { code } => AppError::ServiceError {
            code: code.into(),
            message: "device mutation persistence unavailable".into(),
        },
    }
}

fn device_read_authority_error(error: DeviceReadAuthorityError) -> AppError {
    match error {
        DeviceReadAuthorityError::InvalidInput(message) => AppError::BadRequest(message),
        DeviceReadAuthorityError::Persistence { code } => AppError::ServiceError {
            code: code.into(),
            message: "device read persistence unavailable".into(),
        },
        DeviceReadAuthorityError::Export(message) => AppError::Internal(message),
    }
}
