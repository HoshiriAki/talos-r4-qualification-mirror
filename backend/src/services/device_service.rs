#![allow(dead_code)]
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::Serialize;

use crate::application::is_valid_serial_no;
use crate::error::AppError;
use crate::utils::constants::{self, txt};
use crate::utils::time;
use chrono::{Datelike, Timelike};

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub id: String,
    #[serde(rename = "serialNo")]
    pub serial_no: String,
    #[serde(rename = "rentalStatus")]
    pub rental_status: String,
    pub notes: String,
    #[serde(rename = "fallbackReturnNode")]
    pub fallback_return_node: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    #[serde(rename = "currentWarehouseId")]
    pub current_warehouse_id: String,
    #[serde(rename = "expectedWarehouseId")]
    pub expected_warehouse_id: String,
    #[serde(rename = "expectedAvailableDate")]
    pub expected_available_date: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "returnNode")]
    pub return_node: String,
    #[serde(rename = "returnNodeSource")]
    pub return_node_source: String,
    #[serde(rename = "warningStatus")]
    pub warning_status: String,
    #[serde(rename = "warningReason")]
    pub warning_reason: String,
}

pub fn row_to_device(row: &rusqlite::Row) -> rusqlite::Result<Device> {
    Ok(Device {
        id: row.get(0)?,
        serial_no: row.get(1)?,
        rental_status: row.get(2)?,
        notes: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        fallback_return_node: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        model_id: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        current_warehouse_id: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        expected_warehouse_id: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        expected_available_date: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        created_at: row.get(9)?,
        return_node: String::new(),
        return_node_source: String::new(),
        warning_status: String::new(),
        warning_reason: String::new(),
    })
}

// ── Validation ──────────────────────────────────────────────────

fn validate_device(input: &serde_json::Value) -> Option<String> {
    let serial_no = input
        .get("serialNo")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if serial_no.is_empty() {
        return Some("缺少字段: serialNo".to_string());
    }
    if !is_valid_serial_no(serial_no) {
        return Some("设备序列号只能包含英文字母和数字".to_string());
    }
    let rental_status = input
        .get("rentalStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if rental_status.is_empty() {
        return Some("缺少字段: rentalStatus".to_string());
    }
    if !constants::DEVICE_STATUS.contains(&rental_status) {
        return Some(format!("设备状态无效: {}", rental_status));
    }
    None
}

// ── Queries ─────────────────────────────────────────────────────

const DEVICE_COLUMNS: &str = "id, serialNo, rentalStatus, notes, fallbackReturnNode, modelId, currentWarehouseId, expectedWarehouseId, expectedAvailableDate, createdAt";

pub fn find_device_by_serial_no_for_tenant(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_no: &str,
) -> Result<Option<Device>, AppError> {
    let conn = pool.get()?;
    let sql = format!(
        "SELECT {} FROM devices WHERE serialNo = ?1 AND tenant_id = ?2 LIMIT 1",
        DEVICE_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![serial_no, tenant_id], row_to_device)?;
    Ok(rows.next().transpose()?)
}

/// Legacy order-service lookup. Direct HTTP routes must use the tenant-scoped variant.
pub fn find_device_by_serial_no(
    pool: &Pool<SqliteConnectionManager>,
    serial_no: &str,
) -> Result<Option<Device>, AppError> {
    let conn = pool.get()?;
    let sql = format!(
        "SELECT {} FROM devices WHERE serialNo = ?1 LIMIT 1",
        DEVICE_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![serial_no], row_to_device)?;
    Ok(rows.next().transpose()?)
}

fn build_device_filter_sql(
    tenant_id: &str,
    filter: &serde_json::Value,
) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>, String) {
    let keyword = filter
        .get("keyword")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let rental_status = filter
        .get("rentalStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let notes = filter
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let warning_status = filter
        .get("warningStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut clauses: Vec<String> = vec!["tenant_id = ?".to_string()];
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(tenant_id.to_string())];

    if !keyword.is_empty() {
        clauses.push("serialNo LIKE ?".to_string());
        values.push(Box::new(format!("%{}%", keyword)));
    }
    if !rental_status.is_empty() && constants::DEVICE_STATUS.contains(&rental_status.as_str()) {
        clauses.push("rentalStatus = ?".to_string());
        values.push(Box::new(rental_status));
    }
    if !notes.is_empty() {
        clauses.push("notes LIKE ?".to_string());
        values.push(Box::new(format!("%{}%", notes)));
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    (where_sql, values, warning_status)
}

fn filter_devices_by_warning_status(devices: Vec<Device>, warning_status: &str) -> Vec<Device> {
    if warning_status == txt::WARNING_NORMAL || warning_status == txt::WARNING_LOST {
        devices
            .into_iter()
            .filter(|d| d.warning_status == warning_status)
            .collect()
    } else {
        devices
    }
}

fn to_safe_positive_integer(value: Option<u32>, fallback: u32) -> u32 {
    match value {
        Some(n) if n > 0 => n,
        _ => fallback,
    }
}

fn get_device_return_node_map(
    pool: &Pool<SqliteConnectionManager>,
    serial_nos: &[String],
) -> Result<std::collections::HashMap<String, String>, AppError> {
    let mut map = std::collections::HashMap::new();
    if serial_nos.is_empty() {
        return Ok(map);
    }

    let placeholders: Vec<String> = serial_nos.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT od.serialNo, MAX(o.endDate) AS returnNode
         FROM order_devices od
         JOIN orders o ON o.id = od.orderId
         WHERE od.serialNo IN ({})
         GROUP BY od.serialNo",
        placeholders.join(",")
    );

    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = serial_nos
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;

    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }

    Ok(map)
}

fn calculate_device_warning(
    rental_status: &str,
    return_node: &str,
    now_epoch_ms: i64,
) -> (String, String) {
    if rental_status == txt::STATUS_REPAIR
        || rental_status == txt::STATUS_LOST
        || rental_status == txt::STATUS_SCRAPPED
    {
        return (txt::WARNING_NORMAL.to_string(), String::new());
    }

    let base_epoch_ms = match time::parse_date_time_to_epoch_ms(return_node) {
        Some(ms) => ms,
        None => return (txt::WARNING_NORMAL.to_string(), String::new()),
    };

    let deadline_epoch_ms = base_epoch_ms + 3 * 24 * 60 * 60 * 1000;
    if now_epoch_ms > deadline_epoch_ms && rental_status != txt::STATUS_CHECKED_IN {
        return (
            txt::WARNING_LOST.to_string(),
            "超过归还节点3天仍未入库".to_string(),
        );
    }

    (txt::WARNING_NORMAL.to_string(), String::new())
}

pub fn attach_device_warnings_with_pool(
    pool: &Pool<SqliteConnectionManager>,
    devices: Vec<Device>,
) -> Result<Vec<Device>, AppError> {
    if devices.is_empty() {
        return Ok(vec![]);
    }

    let serial_nos: Vec<String> = devices.iter().map(|d| d.serial_no.clone()).collect();
    let return_node_map = get_device_return_node_map(pool, &serial_nos)?;
    let now_epoch_ms = time::shanghai_now_epoch_ms();

    let result: Vec<Device> = devices
        .into_iter()
        .map(|mut device| {
            let order_return_node = return_node_map
                .get(&device.serial_no)
                .cloned()
                .unwrap_or_default();

            let (return_node, return_node_source) = if !order_return_node.is_empty() {
                (order_return_node, "order".to_string())
            } else {
                let fallback = device.fallback_return_node.trim().to_string();
                if !fallback.is_empty() {
                    (fallback, "fallback".to_string())
                } else {
                    let created_epoch = time::parse_date_time_to_epoch_ms(&device.created_at);
                    match created_epoch {
                        Some(ms) => (
                            time::format_epoch_ms_to_shanghai_iso(ms),
                            "createdAt".to_string(),
                        ),
                        None => (device.created_at.clone(), "createdAt".to_string()),
                    }
                }
            };

            let (warning_status, warning_reason) =
                calculate_device_warning(&device.rental_status, &return_node, now_epoch_ms);

            device.return_node = return_node;
            device.return_node_source = return_node_source;
            device.warning_status = warning_status;
            device.warning_reason = warning_reason;
            device
        })
        .collect();

    Ok(result)
}

pub fn query_devices(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    filter: &serde_json::Value,
) -> Result<Vec<Device>, AppError> {
    let (where_sql, values, warning_status) = build_device_filter_sql(tenant_id, filter);
    let sql = format!(
        "SELECT {} FROM devices {} ORDER BY createdAt DESC",
        DEVICE_COLUMNS, where_sql
    );

    let rows = {
        let conn = pool.get()?;
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
            row_to_device(row)
        })?
        .filter_map(|r| r.ok())
        .collect::<Vec<Device>>()
    };

    let devices = attach_device_warnings_with_pool(pool, rows)?;
    Ok(filter_devices_by_warning_status(devices, &warning_status))
}

pub fn query_devices_paged(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    filter: &serde_json::Value,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<serde_json::Value, AppError> {
    let safe_page_size = (to_safe_positive_integer(page_size, 20)).min(100);
    let requested_page = to_safe_positive_integer(page, 1);
    let (where_sql, values, warning_status) = build_device_filter_sql(tenant_id, filter);

    if warning_status == txt::WARNING_NORMAL || warning_status == txt::WARNING_LOST {
        let all_rows = {
            let conn = pool.get()?;
            let sql = format!(
                "SELECT {} FROM devices {} ORDER BY createdAt DESC",
                DEVICE_COLUMNS, where_sql
            );
            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                values.iter().map(|v| v.as_ref()).collect();
            stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
                row_to_device(row)
            })?
            .filter_map(|r| r.ok())
            .collect::<Vec<Device>>()
        };

        let filtered = filter_devices_by_warning_status(
            attach_device_warnings_with_pool(pool, all_rows)?,
            &warning_status,
        );
        let total = filtered.len() as u32;
        let total_pages = total.div_ceil(safe_page_size).max(1);
        let safe_page = requested_page.min(total_pages);
        let offset = (safe_page - 1) * safe_page_size;

        let paged: Vec<Device> = filtered
            .into_iter()
            .skip(offset as usize)
            .take(safe_page_size as usize)
            .collect();

        return Ok(serde_json::json!({
            "devices": paged,
            "pagination": {
                "page": safe_page,
                "pageSize": safe_page_size,
                "total": total,
                "totalPages": total_pages,
            },
        }));
    }

    let (total, rows) = {
        let conn = pool.get()?;

        let count_sql = format!("SELECT COUNT(1) AS total FROM devices {}", where_sql);
        let count_params: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        let total: i64 = conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(count_params),
            |row| row.get(0),
        )?;

        let total_pages = (total as u32).div_ceil(safe_page_size).max(1);
        let safe_page = requested_page.min(total_pages);
        let offset = (safe_page - 1) * safe_page_size;

        let page_sql = format!(
            "SELECT {} FROM devices {} ORDER BY createdAt DESC LIMIT ? OFFSET ?",
            DEVICE_COLUMNS, where_sql
        );

        let mut stmt = conn.prepare(&page_sql)?;
        let mut page_values: Vec<Box<dyn rusqlite::types::ToSql>> = values;
        page_values.push(Box::new(safe_page_size as i64));
        page_values.push(Box::new(offset as i64));
        let page_params: Vec<&dyn rusqlite::types::ToSql> =
            page_values.iter().map(|v| v.as_ref()).collect();

        let rows: Vec<Device> = stmt
            .query_map(rusqlite::params_from_iter(page_params), |row| {
                row_to_device(row)
            })?
            .filter_map(|r| r.ok())
            .collect();

        (total as u32, rows)
    };

    Ok(serde_json::json!({
        "devices": attach_device_warnings_with_pool(pool, rows)?,
        "pagination": {
            "page": std::cmp::min(requested_page, total.div_ceil(safe_page_size).max(1)),
            "pageSize": safe_page_size,
            "total": total,
            "totalPages": total.div_ceil(safe_page_size).max(1),
        },
    }))
}

pub fn query_devices_by_serial_nos(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_nos: &[String],
) -> Result<Vec<Device>, AppError> {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<String> = serial_nos
        .iter()
        .map(|x| x.trim().to_string())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect();

    if unique.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = unique.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT {} FROM devices WHERE tenant_id = ? AND serialNo IN ({}) ORDER BY createdAt DESC",
        DEVICE_COLUMNS,
        placeholders.join(",")
    );

    let rows = {
        let conn = pool.get()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut query_values: Vec<&dyn rusqlite::types::ToSql> = vec![&tenant_id];
        query_values.extend(unique.iter().map(|s| s as &dyn rusqlite::types::ToSql));
        let param_refs = query_values;
        stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
            row_to_device(row)
        })?
        .filter_map(|r| r.ok())
        .collect::<Vec<Device>>()
    };

    attach_device_warnings_with_pool(pool, rows)
}

// ── Export helpers ──────────────────────────────────────────────

pub fn format_device_export_file_name() -> String {
    let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);
    format!(
        "devices-{}-{:02}-{:02}-{:02}{:02}{:02}.xlsx",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

pub fn build_device_export_workbook(devices: &[Device]) -> Result<Vec<u8>, AppError> {
    use rust_xlsxwriter::*;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet().set_name("devices")?;

    let headers = ["序列号", "状态", "预警状态", "归还节点", "备注", "创建时间"];
    for (col, h) in headers.iter().enumerate() {
        worksheet.write(0, col as u16, *h)?;
    }

    for (row_idx, device) in devices.iter().enumerate() {
        let r = (row_idx + 1) as u32;
        worksheet.write(r, 0, device.serial_no.as_str())?;
        worksheet.write(r, 1, device.rental_status.as_str())?;
        worksheet.write(r, 2, device.warning_status.as_str())?;
        worksheet.write(r, 3, device.return_node.as_str())?;
        worksheet.write(r, 4, device.notes.as_str())?;
        worksheet.write(r, 5, device.created_at.as_str())?;
    }

    workbook
        .save_to_buffer()
        .map_err(|e| AppError::Internal(format!("Excel write error: {}", e)))
}

// ── CRUD ────────────────────────────────────────────────────────

pub fn create_device(
    pool: &Pool<SqliteConnectionManager>,
    scope: &system_core::DataScope,
    payload: &serde_json::Value,
) -> Result<Device, AppError> {
    if let Some(msg) = validate_device(payload) {
        return Err(AppError::BadRequest(msg));
    }

    let serial_no = payload
        .get("serialNo")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let exists = {
        let conn = pool.get()?;
        conn.query_row(
            "SELECT 1 FROM devices WHERE serialNo = ?1 LIMIT 1",
            params![serial_no],
            |_| Ok(()),
        )
        .is_ok()
    };
    if exists {
        return Err(AppError::Conflict("序列号已存在".to_string()));
    }

    let model = crate::services::model_service::detect_model_from_serial(pool, scope, &serial_no)?;
    let model_id = model.as_ref().map(|m| m.id.as_str()).unwrap_or("");

    let id = uuid::Uuid::new_v4().to_string();
    let notes = payload.get("notes").and_then(|v| v.as_str()).unwrap_or("");
    let current_wh = payload
        .get("currentWarehouseId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expected_wh = payload
        .get("expectedWarehouseId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expected_date = payload
        .get("expectedAvailableDate")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let rental_status = payload
        .get("rentalStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let now = time::shanghai_now_iso();

    {
        let conn = pool.get()?;
        conn.execute(
            "INSERT INTO devices (id, serialNo, rentalStatus, notes, modelId, currentWarehouseId, expectedWarehouseId, expectedAvailableDate, createdAt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, serial_no, rental_status, notes, model_id, current_wh, expected_wh, expected_date, now],
        )?;
    }

    let device = find_device_by_serial_no(pool, &serial_no)?
        .ok_or_else(|| AppError::Internal("创建后查找设备失败".to_string()))?;
    let mut devices = attach_device_warnings_with_pool(pool, vec![device])?;
    Ok(devices.remove(0))
}

pub fn update_device(
    pool: &Pool<SqliteConnectionManager>,
    serial_no: &str,
    patch: &serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    let existing = find_device_by_serial_no(pool, serial_no)?
        .ok_or_else(|| AppError::NotFound("设备不存在".to_string()))?;

    let new_serial_no = patch
        .get("serialNo")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(serial_no);

    let merged_rental_status = patch
        .get("rentalStatus")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.rental_status);
    let merged_notes = patch
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.notes);

    let merged_for_validation = serde_json::json!({
        "serialNo": new_serial_no,
        "rentalStatus": merged_rental_status,
        "notes": merged_notes,
    });
    if let Some(msg) = validate_device(&merged_for_validation) {
        return Err(AppError::BadRequest(msg));
    }

    {
        let conn = pool.get()?;
        conn.execute(
            "UPDATE devices SET serialNo = ?1, rentalStatus = ?2, notes = ?3 WHERE serialNo = ?4",
            params![new_serial_no, merged_rental_status, merged_notes, serial_no],
        )?;
    }

    let mut updated = existing.clone();
    updated.serial_no = new_serial_no.to_string();
    updated.rental_status = merged_rental_status.to_string();
    updated.notes = merged_notes.to_string();

    let mut updated_list = attach_device_warnings_with_pool(pool, vec![updated])?;
    let updated_device = updated_list.remove(0);

    Ok(serde_json::json!({
        "existing": existing,
        "updated": updated_device,
    }))
}

pub struct BulkUpdateResult {
    pub items: Vec<serde_json::Value>,
    pub failed: Vec<serde_json::Value>,
    pub updated_count: usize,
    pub fail_count: usize,
}

pub fn bulk_update_devices(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_nos: &[String],
    updates: &serde_json::Value,
) -> Result<BulkUpdateResult, AppError> {
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut failed: Vec<serde_json::Value> = Vec::new();

    for sn in serial_nos {
        let existing = match find_device_by_serial_no_for_tenant(pool, tenant_id, sn)? {
            Some(e) => e,
            None => {
                failed.push(serde_json::json!({ "serialNo": sn, "reason": "设备不存在" }));
                continue;
            }
        };

        let merged_status = updates
            .get("rentalStatus")
            .and_then(|v| v.as_str())
            .unwrap_or(&existing.rental_status);
        let merged_notes = updates
            .get("notes")
            .and_then(|v| v.as_str())
            .unwrap_or(&existing.notes);

        let merged = serde_json::json!({
            "serialNo": sn,
            "rentalStatus": merged_status,
            "notes": merged_notes,
        });
        if let Some(msg) = validate_device(&merged) {
            failed.push(serde_json::json!({ "serialNo": sn, "reason": msg }));
            continue;
        }

        {
            let conn = pool.get()?;
            let changed = conn.execute(
                "UPDATE devices SET rentalStatus = ?1, notes = ?2 WHERE serialNo = ?3 AND tenant_id = ?4",
                params![merged_status, merged_notes, sn, tenant_id],
            )?;
            if changed == 0 {
                failed.push(serde_json::json!({ "serialNo": sn, "reason": "设备不存在" }));
                continue;
            }
        }

        items.push(serde_json::json!({ "serialNo": sn }));
    }

    Ok(BulkUpdateResult {
        updated_count: items.len(),
        fail_count: failed.len(),
        items,
        failed,
    })
}

pub fn delete_device(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_no: &str,
) -> Result<Option<Device>, AppError> {
    let existing = find_device_by_serial_no_for_tenant(pool, tenant_id, serial_no)?;
    if existing.is_none() {
        return Err(AppError::NotFound("设备不存在".to_string()));
    }

    let conn = pool.get()?;
    let changes = conn.execute(
        "DELETE FROM devices WHERE serialNo = ?1 AND tenant_id = ?2",
        params![serial_no, tenant_id],
    )?;
    if changes == 0 {
        return Err(AppError::NotFound("设备不存在".to_string()));
    }

    Ok(existing)
}

pub fn checkin_by_scan(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_no: &str,
) -> Result<serde_json::Value, AppError> {
    let normalized = serial_no.trim();
    if normalized.is_empty() {
        return Err(AppError::BadRequest("序列号无效".to_string()));
    }
    if !is_valid_serial_no(normalized) {
        return Err(AppError::BadRequest("设备序列号格式无效".to_string()));
    }

    let mut conn = pool.get()?;
    let tx = conn.transaction()?;

    let existing = {
        let sql = format!(
            "SELECT {} FROM devices WHERE serialNo = ?1 AND tenant_id = ?2 LIMIT 1",
            DEVICE_COLUMNS
        );
        let mut stmt = tx.prepare(&sql)?;
        let mut rows = stmt.query_map(params![normalized, tenant_id], row_to_device)?;
        match rows.next() {
            Some(Ok(device)) => device,
            Some(Err(e)) => return Err(AppError::from(e)),
            None => {
                return Err(AppError::NotFound(
                    "设备不存在，请先手动新增设备".to_string(),
                ));
            }
        }
    };

    if existing.rental_status == txt::STATUS_CHECKED_IN {
        let device_with_warnings = attach_device_warnings_with_pool(pool, vec![existing.clone()])?;
        let device = device_with_warnings
            .first()
            .cloned()
            .unwrap_or(existing.clone());
        return Ok(serde_json::json!({
            "ok": true,
            "action": "already_in_stock",
            "beforeStatus": existing.rental_status,
            "afterStatus": existing.rental_status,
            "payload": {
                "ok": true,
                "action": "updated",
                "message": "设备已入库",
                "device": device,
            },
        }));
    }

    let before_status = existing.rental_status.clone();
    let changed = tx.execute(
        "UPDATE devices SET rentalStatus = ?1 WHERE serialNo = ?2 AND tenant_id = ?3",
        params![txt::STATUS_CHECKED_IN, normalized, tenant_id],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(
            "设备不存在，请先手动新增设备".to_string(),
        ));
    }
    tx.commit()?;

    let now_iso = time::shanghai_now_iso();
    let mut auto_completed_orders: Vec<serde_json::Value> = Vec::new();
    {
        let conn = pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT od.orderId FROM order_devices od JOIN orders o ON o.id = od.orderId WHERE od.serialNo = ?1 AND o.tenant_id = ?2 AND o.status IN ('reserved', 'active')",
        )?;
        let order_rows: Vec<String> = stmt
            .query_map(params![normalized, tenant_id], |row| {
                row.get::<_, String>(0)
            })?
            .filter_map(|r| r.ok())
            .collect();

        for order_id in &order_rows {
            let pending: i64 = conn.query_row(
                "SELECT COUNT(*) as cnt FROM order_devices od JOIN devices d ON d.serialNo = od.serialNo WHERE od.orderId = ?1 AND d.tenant_id = ?2 AND d.rentalStatus != ?3",
                params![order_id, tenant_id, txt::STATUS_CHECKED_IN],
                |row| row.get(0),
            )?;

            if pending == 0 {
                conn.execute(
                    "UPDATE orders SET status = 'completed', updatedAt = ?1 WHERE id = ?2 AND tenant_id = ?3",
                    params![now_iso, order_id, tenant_id],
                )?;
                auto_completed_orders.push(serde_json::json!({
                    "orderId": order_id,
                    "reason": "all_devices_checked_in",
                    "triggeredBySerialNo": normalized,
                }));
            }
        }
    }

    let mut updated = existing.clone();
    updated.rental_status = txt::STATUS_CHECKED_IN.to_string();
    let mut updated_list = attach_device_warnings_with_pool(pool, vec![updated])?;
    let device = updated_list.remove(0);

    let payload = if auto_completed_orders.is_empty() {
        serde_json::json!({
            "ok": true,
            "action": "updated",
            "message": "设备状态已更新为已入库",
            "device": device,
        })
    } else {
        serde_json::json!({
            "ok": true,
            "action": "updated",
            "message": "设备状态已更新为已入库",
            "device": device,
            "autoCompletedOrders": auto_completed_orders,
        })
    };

    let mut result = serde_json::json!({
        "ok": true,
        "action": "updated",
        "beforeStatus": before_status,
        "afterStatus": txt::STATUS_CHECKED_IN,
        "payload": payload,
    });
    if !auto_completed_orders.is_empty() {
        result["autoCompletedOrders"] = serde_json::json!(auto_completed_orders);
    }

    Ok(result)
}

pub fn update_device_inventory(
    pool: &Pool<SqliteConnectionManager>,
    serial_no: &str,
    current_warehouse_id: Option<&str>,
    expected_warehouse_id: Option<&str>,
    expected_available_date: Option<&str>,
) -> Result<(), AppError> {
    let mut fields: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(cwid) = current_warehouse_id {
        fields.push("currentWarehouseId = ?".to_string());
        values.push(Box::new(cwid.to_string()));
    }
    if let Some(ewid) = expected_warehouse_id {
        fields.push("expectedWarehouseId = ?".to_string());
        values.push(Box::new(ewid.to_string()));
    }
    if let Some(ead) = expected_available_date {
        fields.push("expectedAvailableDate = ?".to_string());
        values.push(Box::new(ead.to_string()));
    }

    if fields.is_empty() {
        return Err(AppError::BadRequest("无可更新字段".to_string()));
    }

    values.push(Box::new(serial_no.to_string()));
    let sql = format!(
        "UPDATE devices SET {} WHERE serialNo = ?",
        fields.join(", ")
    );

    let conn = pool.get()?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    conn.execute(&sql, rusqlite::params_from_iter(param_refs))?;

    Ok(())
}

pub fn undo_checkin_scan(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_no: &str,
    previous_status: &str,
) -> Result<(), AppError> {
    let normalized = serial_no.trim();
    if normalized.is_empty() || previous_status.trim().is_empty() {
        return Err(AppError::BadRequest("序列号或原状态不能为空".to_string()));
    }
    let conn = pool.get()?;
    let rows = conn.execute(
        "UPDATE devices SET rentalStatus = ?1, updatedAt = datetime('now','loctime') WHERE serialNo = ?2 AND tenant_id = ?3",
        params![previous_status.trim(), normalized, tenant_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("设备不存在".to_string()));
    }
    Ok(())
}
