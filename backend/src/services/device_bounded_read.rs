use std::collections::{HashMap, HashSet};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::types::ToSql;

use crate::error::AppError;
use crate::services::device_service::{self, Device};
use crate::utils::constants::{self, txt};
use crate::utils::time;

pub const DEVICE_LEGACY_LIST_ROWS_MAX: usize = 500;
pub const DEVICE_EXPORT_ROWS_MAX: usize = 500;
pub const DEVICE_WARNING_SCAN_ROWS_MAX: usize = 5_000;

const DEVICE_COLUMNS: &str = "id, serialNo, rentalStatus, notes, fallbackReturnNode, modelId, currentWarehouseId, expectedWarehouseId, expectedAvailableDate, createdAt";

fn normalized(value: Option<&str>) -> String {
    value.unwrap_or("").trim().to_string()
}

fn legacy_list_query(
    tenant_id: &str,
    rental_status: Option<&str>,
    model_id: Option<&str>,
    warehouse_id: Option<&str>,
    serial_no: Option<&str>,
) -> (String, Vec<String>) {
    let mut sql = String::from(
        "SELECT d.serialNo, d.modelId, dm.name AS modelName, d.currentWarehouseId, \
         w.name AS warehouseName, d.rentalStatus, d.notes, d.createdAt, d.createdAt AS updatedAt \
         FROM devices d \
         LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id \
         LEFT JOIN warehouses w ON w.id = d.currentWarehouseId AND w.tenant_id = d.tenant_id \
         WHERE d.tenant_id = ?",
    );
    let mut values = vec![tenant_id.to_string()];
    for (column, value) in [
        ("d.rentalStatus", normalized(rental_status)),
        ("d.modelId", normalized(model_id)),
        ("d.currentWarehouseId", normalized(warehouse_id)),
        ("d.serialNo", normalized(serial_no)),
    ] {
        if !value.is_empty() {
            sql.push_str(&format!(" AND {column} = ?"));
            values.push(value);
        }
    }
    sql.push_str(" ORDER BY d.serialNo LIMIT ?");
    values.push((DEVICE_LEGACY_LIST_ROWS_MAX + 1).to_string());
    (sql, values)
}

pub fn list_legacy_devices_bounded(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    rental_status: Option<&str>,
    model_id: Option<&str>,
    warehouse_id: Option<&str>,
    serial_no: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    let (sql, values) =
        legacy_list_query(tenant_id, rental_status, model_id, warehouse_id, serial_no);
    let limit = values
        .last()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| AppError::Internal("设备列表预算无效".to_string()))?;
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let mut refs: Vec<&dyn ToSql> = values[..values.len() - 1]
        .iter()
        .map(|value| value as &dyn ToSql)
        .collect();
    refs.push(&limit);
    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok(serde_json::json!({
            "serialNo": row.get::<_, String>(0)?,
            "modelId": row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            "modelName": row.get::<_, Option<String>>(2)?,
            "warehouseId": row.get::<_, Option<String>>(3)?,
            "warehouseName": row.get::<_, Option<String>>(4)?,
            "status": row.get::<_, String>(5)?,
            "notes": row.get::<_, Option<String>>(6)?,
            "createdAt": row.get::<_, String>(7)?,
            "updatedAt": row.get::<_, String>(8)?,
        }))
    })?;
    let devices = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if devices.len() > DEVICE_LEGACY_LIST_ROWS_MAX {
        return Err(AppError::BadRequest(format!(
            "无分页设备列表超过 {} 行；请使用 page/pageSize 分页查询",
            DEVICE_LEGACY_LIST_ROWS_MAX
        )));
    }
    Ok(serde_json::Value::Array(devices))
}

fn base_filter_sql(
    tenant_id: &str,
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
) -> (String, Vec<String>) {
    let mut clauses = vec!["tenant_id = ?".to_string()];
    let mut values = vec![tenant_id.to_string()];
    let keyword = normalized(keyword);
    if !keyword.is_empty() {
        clauses.push("serialNo LIKE ?".to_string());
        values.push(format!("%{keyword}%"));
    }
    let rental_status = normalized(rental_status);
    if !rental_status.is_empty() && constants::DEVICE_STATUS.contains(&rental_status.as_str()) {
        clauses.push("rentalStatus = ?".to_string());
        values.push(rental_status);
    }
    let notes = normalized(notes);
    if !notes.is_empty() {
        clauses.push("notes LIKE ?".to_string());
        values.push(format!("%{notes}%"));
    }
    (format!("WHERE {}", clauses.join(" AND ")), values)
}

fn count_base_matches(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
) -> Result<usize, AppError> {
    let (where_sql, values) = base_filter_sql(tenant_id, keyword, rental_status, notes);
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&format!("SELECT COUNT(*) FROM devices {where_sql}"))?;
    let refs: Vec<&dyn ToSql> = values.iter().map(|value| value as &dyn ToSql).collect();
    let count: i64 = stmt.query_row(refs.as_slice(), |row| row.get(0))?;
    usize::try_from(count).map_err(|_| AppError::Internal("设备数量无效".to_string()))
}

fn tenant_return_node_map(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_nos: &[String],
) -> Result<HashMap<String, String>, AppError> {
    if serial_nos.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; serial_nos.len()].join(",");
    let sql = format!(
        "SELECT od.serialNo, MAX(o.endDate) AS returnNode \
         FROM order_devices od \
         JOIN orders o ON o.id = od.orderId AND o.tenant_id = od.tenant_id \
         WHERE od.tenant_id = ? AND o.tenant_id = ? AND od.serialNo IN ({placeholders}) \
         GROUP BY od.serialNo"
    );
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let mut refs: Vec<&dyn ToSql> = vec![&tenant_id, &tenant_id];
    refs.extend(serial_nos.iter().map(|serial| serial as &dyn ToSql));
    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (serial_no, return_node) = row?;
        map.insert(serial_no, return_node);
    }
    Ok(map)
}

fn calculate_warning(
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
    let Some(base_epoch_ms) = time::parse_date_time_to_epoch_ms(return_node) else {
        return (txt::WARNING_NORMAL.to_string(), String::new());
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

fn attach_tenant_warnings(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    devices: Vec<Device>,
) -> Result<Vec<Device>, AppError> {
    if devices.is_empty() {
        return Ok(Vec::new());
    }
    let serial_nos: Vec<String> = devices
        .iter()
        .map(|device| device.serial_no.clone())
        .collect();
    let return_node_map = tenant_return_node_map(pool, tenant_id, &serial_nos)?;
    let now_epoch_ms = time::shanghai_now_epoch_ms();
    Ok(devices
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
                    match time::parse_date_time_to_epoch_ms(&device.created_at) {
                        Some(ms) => (
                            time::format_epoch_ms_to_shanghai_iso(ms),
                            "createdAt".to_string(),
                        ),
                        None => (device.created_at.clone(), "createdAt".to_string()),
                    }
                }
            };
            let (warning_status, warning_reason) =
                calculate_warning(&device.rental_status, &return_node, now_epoch_ms);
            device.return_node = return_node;
            device.return_node_source = return_node_source;
            device.warning_status = warning_status;
            device.warning_reason = warning_reason;
            device
        })
        .collect())
}

fn filter_warning(devices: Vec<Device>, warning_status: &str) -> Vec<Device> {
    if warning_status == txt::WARNING_NORMAL || warning_status == txt::WARNING_LOST {
        devices
            .into_iter()
            .filter(|device| device.warning_status == warning_status)
            .collect()
    } else {
        devices
    }
}

fn load_base_devices(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<Device>, AppError> {
    let (where_sql, values) = base_filter_sql(tenant_id, keyword, rental_status, notes);
    let mut sql =
        format!("SELECT {DEVICE_COLUMNS} FROM devices {where_sql} ORDER BY createdAt DESC");
    let mut numeric_values = Vec::new();
    if let Some(limit) = limit {
        sql.push_str(" LIMIT ?");
        numeric_values.push(
            i64::try_from(limit)
                .map_err(|_| AppError::BadRequest("设备读取上限无效".to_string()))?,
        );
        if let Some(offset) = offset {
            sql.push_str(" OFFSET ?");
            numeric_values.push(
                i64::try_from(offset)
                    .map_err(|_| AppError::BadRequest("设备读取偏移无效".to_string()))?,
            );
        }
    }
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let mut refs: Vec<&dyn ToSql> = values.iter().map(|value| value as &dyn ToSql).collect();
    refs.extend(numeric_values.iter().map(|value| value as &dyn ToSql));
    let rows = stmt.query_map(refs.as_slice(), device_service::row_to_device)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn enforce_warning_scan_budget(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
) -> Result<(), AppError> {
    let count = count_base_matches(pool, tenant_id, keyword, rental_status, notes)?;
    if count > DEVICE_WARNING_SCAN_ROWS_MAX {
        return Err(AppError::BadRequest(format!(
            "预警状态查询需要扫描 {count} 台设备，超过上限 {DEVICE_WARNING_SCAN_ROWS_MAX}；请增加筛选条件"
        )));
    }
    Ok(())
}

pub fn query_devices_paged_bounded(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
    warning_status: Option<&str>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<serde_json::Value, AppError> {
    let safe_page_size = page_size.filter(|value| *value > 0).unwrap_or(20).min(100);
    let requested_page = page.filter(|value| *value > 0).unwrap_or(1);
    let warning_status = normalized(warning_status);
    if warning_status == txt::WARNING_NORMAL || warning_status == txt::WARNING_LOST {
        enforce_warning_scan_budget(pool, tenant_id, keyword, rental_status, notes)?;
        let rows = load_base_devices(pool, tenant_id, keyword, rental_status, notes, None, None)?;
        let filtered = filter_warning(
            attach_tenant_warnings(pool, tenant_id, rows)?,
            &warning_status,
        );
        let total = u32::try_from(filtered.len())
            .map_err(|_| AppError::Internal("设备分页总数溢出".to_string()))?;
        let total_pages = total.div_ceil(safe_page_size).max(1);
        let safe_page = requested_page.min(total_pages);
        let offset = (safe_page - 1) * safe_page_size;
        let devices: Vec<Device> = filtered
            .into_iter()
            .skip(offset as usize)
            .take(safe_page_size as usize)
            .collect();
        return Ok(serde_json::json!({
            "devices": devices,
            "pagination": {"page": safe_page, "pageSize": safe_page_size, "total": total, "totalPages": total_pages},
        }));
    }

    let total = count_base_matches(pool, tenant_id, keyword, rental_status, notes)?;
    let total =
        u32::try_from(total).map_err(|_| AppError::Internal("设备分页总数溢出".to_string()))?;
    let total_pages = total.div_ceil(safe_page_size).max(1);
    let safe_page = requested_page.min(total_pages);
    let offset = ((safe_page - 1) * safe_page_size) as usize;
    let rows = load_base_devices(
        pool,
        tenant_id,
        keyword,
        rental_status,
        notes,
        Some(safe_page_size as usize),
        Some(offset),
    )?;
    let devices = attach_tenant_warnings(pool, tenant_id, rows)?;
    Ok(serde_json::json!({
        "devices": devices,
        "pagination": {"page": safe_page, "pageSize": safe_page_size, "total": total, "totalPages": total_pages},
    }))
}

pub fn export_devices_bounded(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
    warning_status: Option<&str>,
) -> Result<Vec<Device>, AppError> {
    let base_count = count_base_matches(pool, tenant_id, keyword, rental_status, notes)?;
    if base_count > DEVICE_EXPORT_ROWS_MAX {
        return Err(AppError::BadRequest(format!(
            "设备导出候选 {base_count} 行，超过上限 {DEVICE_EXPORT_ROWS_MAX}；请缩小筛选范围"
        )));
    }
    let rows = load_base_devices(
        pool,
        tenant_id,
        keyword,
        rental_status,
        notes,
        Some(DEVICE_EXPORT_ROWS_MAX + 1),
        None,
    )?;
    if rows.len() > DEVICE_EXPORT_ROWS_MAX {
        return Err(AppError::BadRequest(format!(
            "设备导出候选超过上限 {DEVICE_EXPORT_ROWS_MAX}；请缩小筛选范围"
        )));
    }
    Ok(filter_warning(
        attach_tenant_warnings(pool, tenant_id, rows)?,
        &normalized(warning_status),
    ))
}

pub fn export_devices_by_serial_nos_bounded(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    serial_nos: &[String],
    max_items: usize,
) -> Result<Vec<Device>, AppError> {
    if serial_nos.len() > max_items {
        return Err(AppError::BadRequest(format!(
            "serialNos 数量超过上限 {max_items}"
        )));
    }
    let mut seen = HashSet::new();
    let unique: Vec<String> = serial_nos
        .iter()
        .map(|serial| serial.trim().to_string())
        .filter(|serial| !serial.is_empty() && seen.insert(serial.clone()))
        .collect();
    if unique.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; unique.len()].join(",");
    let sql = format!(
        "SELECT {DEVICE_COLUMNS} FROM devices WHERE tenant_id = ? AND serialNo IN ({placeholders}) ORDER BY createdAt DESC"
    );
    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let mut refs: Vec<&dyn ToSql> = vec![&tenant_id];
    refs.extend(unique.iter().map(|serial| serial as &dyn ToSql));
    let rows = stmt.query_map(refs.as_slice(), device_service::row_to_device)?;
    let devices = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    attach_tenant_warnings(pool, tenant_id, devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_budgets_are_finite() {
        assert!((1..=500).contains(&DEVICE_LEGACY_LIST_ROWS_MAX));
        assert!((1..=500).contains(&DEVICE_EXPORT_ROWS_MAX));
        assert!((1..=5_000).contains(&DEVICE_WARNING_SCAN_ROWS_MAX));
    }

    #[test]
    fn legacy_query_always_binds_tenant_limit_and_compat_updated_at() {
        let (sql, values) = legacy_list_query("tenant-a", None, None, None, None);
        assert!(sql.contains("WHERE d.tenant_id = ?"));
        assert!(sql.contains("LIMIT ?"));
        assert!(sql.contains("d.createdAt AS updatedAt"));
        assert!(!sql.contains("d.updatedAt"));
        assert_eq!(values.first().map(String::as_str), Some("tenant-a"));
    }

    #[test]
    fn admission_filter_matches_legacy_invalid_status_semantics() {
        let (sql, values) =
            base_filter_sql("tenant-a", Some("SN"), Some("not-a-status"), Some("note"));
        assert!(!sql.contains("rentalStatus = ?"));
        assert_eq!(values, vec!["tenant-a", "%SN%", "%note%"]);
    }
}
