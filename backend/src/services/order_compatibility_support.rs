use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;
use rusqlite::params;
use rust_xlsxwriter::*;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use crate::error::AppError;
use crate::services::{device_service, pricing_service};
use crate::utils::constants::{PICKUP_METHODS, txt};
use crate::utils::time;

// ── Regex ──────────────────────────────────────────────────────────

static ORDER_NO_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());

// ── Helpers ────────────────────────────────────────────────────────

fn normalize_date_string(s: &str) -> Result<String, String> {
    let val = serde_json::Value::String(s.trim().to_string());
    time::normalize_business_date(&val)
}

fn to_safe_positive_integer(value: &str, fallback: i64) -> i64 {
    let n: i64 = value.parse().unwrap_or(fallback);
    if n <= 0 { fallback } else { n }
}

pub fn is_valid_order_no(value: &str) -> bool {
    ORDER_NO_REGEX.is_match(value)
}

fn parse_pickup_methods(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        serde_json::Value::String(s) => serde_json::from_str::<Vec<String>>(s).unwrap_or_default(),
        _ => vec![],
    }
}

fn pickup_methods_to_string(methods: &[String]) -> String {
    methods.join("\u{3001}")
}

// ── Data structures ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: String,
    pub order_no: String,
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
    pub pickup_methods: Vec<String>,
    pub address: String,
    pub notes: String,
    pub device_serial_no: String,
    #[serde(default)]
    pub tracking_no: String,
    pub province: String,
    pub total_price: f64,
    pub send_warehouse_id: String,
    pub return_warehouse_id: String,
    #[serde(default)]
    pub model_id: String,
    pub devices: Vec<String>,
    #[serde(default)]
    pub accessories: Vec<String>,
    #[serde(default)]
    pub device_models: std::collections::HashMap<String, i64>,
    #[serde(default = "default_status")]
    pub status: String,
    pub created_at: String,
}

fn default_status() -> String {
    "active".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PagedOrders {
    pub users: Vec<Order>,
    pub pagination: Pagination,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceDetail {
    pub id: String,
    pub order_id: String,
    pub date_key: String,
    pub final_daily_price: f64,
    pub price_source: String,
    pub occupy_type: String,
    pub occupy_factor: f64,
    pub amount: f64,
    pub created_at: String,
}

// ── Order row mapping ──────────────────────────────────────────────

fn parse_accessories_json(raw: Option<String>) -> Vec<String> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

fn parse_device_models_json(raw: Option<String>) -> std::collections::HashMap<String, i64> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

fn row_to_order(row: &rusqlite::Row) -> rusqlite::Result<Order> {
    let pickup_json: String = row.get::<_, String>(5)?;
    let pickup_methods = serde_json::from_str::<Vec<String>>(&pickup_json).unwrap_or_default();

    let accessories_raw: Option<String> = row.get(14).ok();
    let stored_status: String = row
        .get::<_, Option<String>>(15)?
        .unwrap_or_else(|| "active".to_string());
    let device_models_raw: Option<String> = row.get::<_, Option<String>>(17).unwrap_or(None);
    let delivery_date: String = row.get(4)?;
    let today = time::shanghai_now_date_key();
    let effective_status = if !delivery_date.is_empty() && delivery_date > today {
        txt::STATUS_RESERVED.to_string()
    } else {
        stored_status
    };

    Ok(Order {
        id: row.get(0)?,
        order_no: row.get(1)?,
        start_date: row.get(2)?,
        end_date: row.get(3)?,
        delivery_date: row.get(4)?,
        pickup_methods,
        address: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        notes: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        device_serial_no: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        province: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
        total_price: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
        send_warehouse_id: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        return_warehouse_id: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
        model_id: String::new(),
        devices: vec![],
        accessories: parse_accessories_json(accessories_raw),
        device_models: parse_device_models_json(device_models_raw),
        tracking_no: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
        status: effective_status,
        created_at: row.get::<_, Option<String>>(13)?.unwrap_or_default(),
    })
}

/// Try to read an order row, falling back to the base 10 columns if extras don't exist.
fn row_to_order_fallback(row: &rusqlite::Row, col_count: usize) -> rusqlite::Result<Order> {
    if col_count >= 16 {
        return row_to_order(row);
    }
    // Base columns only (id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, deviceSerialNo, createdAt = 10 columns)
    let pickup_json: String = row.get(5)?;
    let pickup_methods = serde_json::from_str::<Vec<String>>(&pickup_json).unwrap_or_default();

    Ok(Order {
        id: row.get(0)?,
        order_no: row.get(1)?,
        start_date: row.get(2)?,
        end_date: row.get(3)?,
        delivery_date: row.get(4)?,
        pickup_methods,
        address: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        notes: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        device_serial_no: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        province: String::new(),
        total_price: 0.0,
        send_warehouse_id: String::new(),
        return_warehouse_id: String::new(),
        model_id: String::new(),
        devices: vec![],
        accessories: vec![],
        device_models: std::collections::HashMap::new(),
        tracking_no: String::new(),
        status: "active".to_string(),
        created_at: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
    })
}

// ── Device association helpers ─────────────────────────────────────

fn get_order_devices_map(
    pool: &Pool<SqliteConnectionManager>,
    order_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<String>>, AppError> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    if order_ids.is_empty() {
        return Ok(map);
    }

    let placeholders: Vec<String> = order_ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT orderId, serialNo FROM order_devices WHERE orderId IN ({}) ORDER BY createdAt ASC",
        placeholders.join(",")
    );

    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = order_ids
        .iter()
        .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for (order_id, serial_no) in rows.flatten() {
        map.entry(order_id).or_default().push(serial_no);
    }

    Ok(map)
}

fn attach_order_devices(
    pool: &Pool<SqliteConnectionManager>,
    mut orders: Vec<Order>,
) -> Result<Vec<Order>, AppError> {
    if orders.is_empty() {
        return Ok(orders);
    }

    let ids: Vec<String> = orders.iter().map(|o| o.id.clone()).collect();
    let devices_map = get_order_devices_map(pool, &ids)?;

    for order in &mut orders {
        let linked = devices_map.get(&order.id);
        order.devices = match linked {
            Some(devices) if !devices.is_empty() => devices.clone(),
            _ => {
                if !order.device_serial_no.is_empty() {
                    vec![order.device_serial_no.clone()]
                } else {
                    vec![]
                }
            }
        };
    }

    Ok(orders)
}

// ── Compatibility validation ───────────────────────────────────────

pub fn validate_user(input: &serde_json::Value) -> Option<String> {
    let order_no = input["orderNo"].as_str().unwrap_or("");
    let start_date = input["startDate"].as_str().unwrap_or("");
    let end_date = input["endDate"].as_str().unwrap_or("");
    let delivery_date = input["deliveryDate"].as_str().unwrap_or("");

    if order_no.is_empty() {
        return Some("缺少字段: orderNo".to_string());
    }
    if start_date.is_empty() {
        return Some("缺少字段: startDate".to_string());
    }
    if end_date.is_empty() {
        return Some("缺少字段: endDate".to_string());
    }
    if delivery_date.is_empty() {
        return Some("缺少字段: deliveryDate".to_string());
    }

    if !is_valid_order_no(order_no) {
        return Some("订单号只能为纯数字".to_string());
    }

    let pickup_methods = parse_pickup_methods(&input["pickupMethods"]);
    if pickup_methods.is_empty() {
        return Some("取货方式必须至少选择一项".to_string());
    }
    for method in &pickup_methods {
        if !PICKUP_METHODS.contains(&method.as_str()) {
            return Some(format!("取货方式无效: {}", method));
        }
    }

    // deviceSerialNo already validated via as_str() above
    None
}

#[derive(Debug)]
pub struct NormalizedDates {
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
}

pub fn normalize_order_date_fields(input: &serde_json::Value) -> Result<NormalizedDates, String> {
    let start = normalize_date_string(input["startDate"].as_str().unwrap_or(""))
        .map_err(|_| "startDate 格式无效，请填写正确日期".to_string())?;
    let end = normalize_date_string(input["endDate"].as_str().unwrap_or(""))
        .map_err(|_| "endDate 格式无效，请填写正确日期".to_string())?;
    let delivery = normalize_date_string(input["deliveryDate"].as_str().unwrap_or(""))
        .map_err(|_| "deliveryDate 格式无效，请填写正确日期".to_string())?;

    Ok(NormalizedDates {
        start_date: start,
        end_date: end,
        delivery_date: delivery,
    })
}

// ── Filter builder ─────────────────────────────────────────────────

pub struct FilterValues {
    pub where_sql: String,
    pub values: Vec<String>,
}

pub fn build_user_filter_sql(filter: &serde_json::Value) -> FilterValues {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut values: Vec<String> = Vec::new();

    let keyword = filter["keyword"].as_str().unwrap_or("").trim().to_string();
    if !keyword.is_empty() {
        let like = format!("%{}%", keyword);
        where_clauses.push(
            "(orderNo LIKE ? OR address LIKE ? OR notes LIKE ? OR deviceSerialNo LIKE ?
             OR EXISTS (SELECT 1 FROM order_devices od WHERE od.orderId = orders.id AND od.serialNo LIKE ?))"
                .to_string(),
        );
        values.push(like.clone());
        values.push(like.clone());
        values.push(like.clone());
        values.push(like.clone());
        values.push(like);
    }

    // orderNo
    if let Some(on) = filter["orderNo"].as_str() {
        let trimmed = on.trim();
        if !trimmed.is_empty() {
            where_clauses.push("orderNo LIKE ?".to_string());
            values.push(format!("%{}%", trimmed));
        }
    }

    // address
    if let Some(addr) = filter["address"].as_str() {
        let trimmed = addr.trim();
        if !trimmed.is_empty() {
            where_clauses.push("address LIKE ?".to_string());
            values.push(format!("%{}%", trimmed));
        }
    }

    // startDate range
    if let Some(sdf) = filter["startDateFrom"].as_str() {
        let trimmed = sdf.trim();
        if !trimmed.is_empty() {
            where_clauses.push("startDate >= ?".to_string());
            values.push(trimmed.to_string());
        }
    }
    if let Some(sdt) = filter["startDateTo"].as_str() {
        let trimmed = sdt.trim();
        if !trimmed.is_empty() {
            where_clauses.push("startDate <= ?".to_string());
            values.push(trimmed.to_string());
        }
    }
    if let Some(sd) = filter["startDate"].as_str() {
        let trimmed = sd.trim();
        if !trimmed.is_empty() {
            where_clauses.push("startDate = ?".to_string());
            values.push(trimmed.to_string());
        }
    }

    // endDate range
    if let Some(edf) = filter["endDateFrom"].as_str() {
        let trimmed = edf.trim();
        if !trimmed.is_empty() {
            where_clauses.push("endDate >= ?".to_string());
            values.push(trimmed.to_string());
        }
    }
    if let Some(edt) = filter["endDateTo"].as_str() {
        let trimmed = edt.trim();
        if !trimmed.is_empty() {
            where_clauses.push("endDate <= ?".to_string());
            values.push(trimmed.to_string());
        }
    }
    if let Some(ed) = filter["endDate"].as_str() {
        let trimmed = ed.trim();
        if !trimmed.is_empty() {
            where_clauses.push("endDate = ?".to_string());
            values.push(trimmed.to_string());
        }
    }

    // includedDate (overlap filter)
    if let Some(id) = filter["includedDate"].as_str() {
        let trimmed = id.trim();
        if !trimmed.is_empty()
            && let Ok(normalized) = normalize_date_string(trimmed)
            && !normalized.is_empty()
        {
            where_clauses.push("startDate <= ?".to_string());
            values.push(normalized.clone());
            where_clauses.push("endDate >= ?".to_string());
            values.push(normalized);
        }
    }

    // deliveryDate range
    if let Some(ddf) = filter["deliveryDateFrom"].as_str() {
        let trimmed = ddf.trim();
        if !trimmed.is_empty() {
            where_clauses.push("deliveryDate >= ?".to_string());
            values.push(trimmed.to_string());
        }
    }
    if let Some(ddt) = filter["deliveryDateTo"].as_str() {
        let trimmed = ddt.trim();
        if !trimmed.is_empty() {
            where_clauses.push("deliveryDate <= ?".to_string());
            values.push(trimmed.to_string());
        }
    }
    if let Some(dd) = filter["deliveryDate"].as_str() {
        let trimmed = dd.trim();
        if !trimmed.is_empty() {
            where_clauses.push("deliveryDate = ?".to_string());
            values.push(trimmed.to_string());
        }
    }

    // status: "reserved" is computed (deliveryDate > today), not stored in DB
    if let Some(s) = filter["status"].as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            if trimmed == "reserved" {
                let today = time::shanghai_now_date_key();
                if !today.is_empty() {
                    where_clauses.push("deliveryDate > ?".to_string());
                    values.push(today);
                }
            } else if trimmed == "active" {
                where_clauses.push("status = 'active'".to_string());
                let today = time::shanghai_now_date_key();
                if !today.is_empty() {
                    where_clauses.push("deliveryDate <= ?".to_string());
                    values.push(today);
                }
            } else {
                where_clauses.push("status = ?".to_string());
                values.push(trimmed.to_string());
            }
        }
    }

    // pickupMethods
    if let Some(pm) = filter["pickupMethods"].as_array() {
        let valid: Vec<String> = pm
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| PICKUP_METHODS.contains(&s.as_str()))
            .collect();
        if !valid.is_empty() {
            let clauses: Vec<String> = valid
                .iter()
                .map(|_| "pickupMethods LIKE ?".to_string())
                .collect();
            where_clauses.push(format!("({})", clauses.join(" OR ")));
            for method in &valid {
                values.push(format!("%{}%", method));
            }
        }
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    FilterValues { where_sql, values }
}

// ── Query functions ────────────────────────────────────────────────

pub fn query_orders(
    pool: &Pool<SqliteConnectionManager>,
    filter: &serde_json::Value,
) -> Result<Vec<Order>, AppError> {
    let FilterValues { where_sql, values } = build_user_filter_sql(filter);
    let sql = format!(
        "SELECT id, orderNo, startDate, endDate, deliveryDate, pickupMethods,
                address, notes, deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, createdAt, accessories, status
         FROM orders {} ORDER BY createdAt DESC",
        where_sql
    );

    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;

    let boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
        .into_iter()
        .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = boxed.iter().map(|b| b.as_ref()).collect();

    let rows: Vec<Order> = stmt
        .query_map(param_refs.as_slice(), row_to_order)?
        .filter_map(|r| match r {
            Ok(order) => Some(order),
            Err(e) => {
                eprintln!("[WARN] query_orders: failed to deserialize row: {e}");
                None
            }
        })
        .collect();

    attach_order_devices(pool, rows)
}

pub fn query_orders_paged(
    pool: &Pool<SqliteConnectionManager>,
    filter: &serde_json::Value,
    page: i64,
    page_size: i64,
) -> Result<PagedOrders, AppError> {
    let safe_page_size = to_safe_positive_integer(&page_size.to_string(), 20).clamp(1, 100);
    let requested_page = std::cmp::max(to_safe_positive_integer(&page.to_string(), 1), 1);

    let FilterValues { where_sql, values } = build_user_filter_sql(filter);

    // Count total
    let count_sql = format!("SELECT COUNT(1) AS total FROM orders {}", where_sql);
    let conn = pool.get()?;
    let mut count_stmt = conn.prepare(&count_sql)?;
    let count_boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
        .iter()
        .map(|v| Box::new(v.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let count_refs: Vec<&dyn rusqlite::types::ToSql> =
        count_boxed.iter().map(|b| b.as_ref()).collect();

    let total: i64 = count_stmt
        .query_row(count_refs.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    let total_pages = std::cmp::max(1, (total + safe_page_size - 1) / safe_page_size);
    let safe_page = std::cmp::min(requested_page, total_pages);
    let offset = (safe_page - 1) * safe_page_size;

    // Query data
    let data_sql = format!(
        "SELECT id, orderNo, startDate, endDate, deliveryDate, pickupMethods,
                address, notes, deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, createdAt, accessories, status
         FROM orders {} ORDER BY createdAt DESC LIMIT ? OFFSET ?",
        where_sql
    );

    let mut data_stmt = conn.prepare(&data_sql)?;
    let mut data_boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
        .into_iter()
        .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    data_boxed.push(Box::new(safe_page_size));
    data_boxed.push(Box::new(offset));
    let data_refs: Vec<&dyn rusqlite::types::ToSql> =
        data_boxed.iter().map(|b| b.as_ref()).collect();

    let rows: Vec<Order> = data_stmt
        .query_map(data_refs.as_slice(), row_to_order)?
        .filter_map(|r| r.ok())
        .collect();

    let users = attach_order_devices(pool, rows)?;

    Ok(PagedOrders {
        users,
        pagination: Pagination {
            page: safe_page,
            page_size: safe_page_size,
            total,
            total_pages,
        },
    })
}

pub fn query_orders_by_ids(
    pool: &Pool<SqliteConnectionManager>,
    ids: &[String],
) -> Result<Vec<Order>, AppError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }

    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT id, orderNo, startDate, endDate, deliveryDate, pickupMethods,
                address, notes, deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, createdAt
         FROM orders WHERE id IN ({}) ORDER BY createdAt DESC",
        placeholders.join(",")
    );

    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = ids
        .iter()
        .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows: Vec<Order> = stmt
        .query_map(param_refs.as_slice(), row_to_order)?
        .filter_map(|r| r.ok())
        .collect();

    attach_order_devices(pool, rows)
}

pub fn find_order_by_id(
    pool: &Pool<SqliteConnectionManager>,
    id: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare("SELECT * FROM orders WHERE id = ?1 LIMIT 1")?;
    let col_count = stmt.column_count();
    let mut rows = stmt.query(params![id])?;
    let row = rows.next()?;
    match row {
        Some(r) => {
            let order = row_to_order_fallback(r, col_count)?;
            Ok(Some(serde_json::to_value(order).unwrap_or_default()))
        }
        None => Ok(None),
    }
}

pub fn find_order_by_order_no(
    pool: &Pool<SqliteConnectionManager>,
    order_no: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare("SELECT * FROM orders WHERE orderNo = ?1 LIMIT 1")?;
    let col_count = stmt.column_count();
    let mut rows = stmt.query(params![order_no])?;
    let row = rows.next()?;
    match row {
        Some(r) => {
            let order = row_to_order_fallback(r, col_count)?;
            Ok(Some(serde_json::to_value(order).unwrap_or_default()))
        }
        None => Ok(None),
    }
}

pub fn find_overlapping_order_using_device(
    pool: &Pool<SqliteConnectionManager>,
    serial_no: &str,
    current_order_id: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT o.id, o.orderNo, o.startDate, o.endDate
         FROM order_devices od
         JOIN orders o ON o.id = od.orderId
         WHERE od.serialNo = ?1 AND od.orderId <> ?2
           AND NOT (o.endDate <= ?3 OR o.startDate >= ?4)
         LIMIT 1",
    )?;
    let result = stmt.query_row(
        params![serial_no, current_order_id, start_date, end_date],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "orderNo": row.get::<_, String>(1)?,
                "startDate": row.get::<_, String>(2)?,
                "endDate": row.get::<_, String>(3)?,
            }))
        },
    );

    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn find_device_inventory_conflicts(
    pool: &Pool<SqliteConnectionManager>,
    device_serials: &[String],
    start_date: &str,
    end_date: &str,
) -> Result<Vec<serde_json::Value>, AppError> {
    let mut conflicts = Vec::new();
    for serial_no in device_serials {
        if let Some(overlap) =
            find_overlapping_order_using_device(pool, serial_no, "", start_date, end_date)?
        {
            conflicts.push(serde_json::json!({
                "serialNo": serial_no,
                "orderNo": overlap["orderNo"],
                "startDate": overlap["startDate"],
                "endDate": overlap["endDate"],
            }));
        }
    }
    Ok(conflicts)
}

// ── Device helpers ─────────────────────────────────────────────────

fn get_shanghai_today_date() -> String {
    time::shanghai_now_date_key()
}

fn set_device_fallback_return_node_today(
    pool: &Pool<SqliteConnectionManager>,
    serial_no: &str,
) -> Result<(), AppError> {
    let normalized = serial_no.trim();
    if normalized.is_empty() {
        return Ok(());
    }
    let conn = pool.get()?;
    conn.execute(
        "UPDATE devices SET fallbackReturnNode = ?1 WHERE serialNo = ?2",
        params![get_shanghai_today_date(), normalized],
    )?;
    Ok(())
}

pub fn order_has_any_device_association(
    pool: &Pool<SqliteConnectionManager>,
    order_id: &str,
) -> Result<bool, AppError> {
    let conn = pool.get()?;

    // Check order_devices
    let mut stmt = conn.prepare("SELECT 1 FROM order_devices WHERE orderId = ?1 LIMIT 1")?;
    if stmt.exists(params![order_id])? {
        return Ok(true);
    }

    // Check legacy deviceSerialNo
    let mut stmt2 = conn.prepare("SELECT deviceSerialNo FROM orders WHERE id = ?1 LIMIT 1")?;
    let legacy: Option<String> = stmt2.query_row(params![order_id], |row| row.get(0)).ok();
    Ok(legacy.map(|s| !s.trim().is_empty()).unwrap_or(false))
}

#[derive(Debug)]
pub struct LinkPreviewResult {
    pub ok: bool,
    pub code: u16,
    pub error: Option<String>,
    pub normalized_serial_no: String,
    pub device: Option<device_service::Device>,
    pub order: Option<Order>,
}

#[derive(Debug, Serialize)]
pub struct LinkDeviceResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct DetachDeviceResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_id: Option<String>,
}

// ── CRUD ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<Order>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_breakdown: Option<Vec<pricing_service::DailyPriceEntry>>,
}

#[derive(Debug, Serialize)]
pub struct UpdateOrderResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Order>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Order>,
}

#[derive(Debug, Serialize)]
pub struct DeleteOrderResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Order>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_serial_nos: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub reserved: usize,
    pub activated: usize,
}

pub fn sync_order_statuses(pool: &Pool<SqliteConnectionManager>) -> Result<SyncResult, AppError> {
    let today = time::shanghai_now_date_key();
    let result = crate::repositories::MaintenanceCompatibilityRepository::new(pool.clone())
        .sync_order_statuses(&today)?;
    Ok(SyncResult {
        reserved: result.reserved,
        activated: result.activated,
    })
}

// ── Price details ──────────────────────────────────────────────────

pub fn get_order_price_details(
    pool: &Pool<SqliteConnectionManager>,
    order_id: &str,
) -> Result<Vec<PriceDetail>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, orderId, dateKey, finalDailyPrice, priceSource, occupyType, occupyFactor, amount, createdAt
         FROM order_price_details
         WHERE orderId = ?1
         ORDER BY dateKey ASC",
    )?;
    let rows = stmt
        .query_map(params![order_id], |row| {
            Ok(PriceDetail {
                id: row.get(0)?,
                order_id: row.get(1)?,
                date_key: row.get(2)?,
                final_daily_price: row.get(3)?,
                price_source: row.get(4)?,
                occupy_type: row.get(5)?,
                occupy_factor: row.get(6)?,
                amount: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ── Excel Export ───────────────────────────────────────────────────

const DEFAULT_EXPORT_FIELD_KEYS: &[&str] = &[
    "orderNo",
    "startDate",
    "endDate",
    "deliveryDate",
    "pickupMethods",
    "address",
    "notes",
    "devices",
    "createdAt",
];

struct ExportFieldDef {
    key: &'static str,
    label: &'static str,
}

const EXPORT_FIELD_DEFS: &[ExportFieldDef] = &[
    ExportFieldDef {
        key: "orderNo",
        label: "订单号",
    },
    ExportFieldDef {
        key: "startDate",
        label: "开始日期",
    },
    ExportFieldDef {
        key: "endDate",
        label: "结束日期",
    },
    ExportFieldDef {
        key: "deliveryDate",
        label: "发货日期",
    },
    ExportFieldDef {
        key: "pickupMethods",
        label: "取货方式",
    },
    ExportFieldDef {
        key: "address",
        label: "地址",
    },
    ExportFieldDef {
        key: "notes",
        label: "备注",
    },
    ExportFieldDef {
        key: "devices",
        label: "设备序列号",
    },
    ExportFieldDef {
        key: "createdAt",
        label: "创建时间",
    },
    ExportFieldDef {
        key: "updatedAt",
        label: "更新时间",
    },
];

fn resolve_export_field_definitions(fields: &[String]) -> Vec<&ExportFieldDef> {
    let defs_by_key: std::collections::HashMap<&str, &ExportFieldDef> =
        EXPORT_FIELD_DEFS.iter().map(|d| (d.key, d)).collect();

    let valid_requested: Vec<&str> = fields
        .iter()
        .map(|s| s.trim())
        .filter(|s| defs_by_key.contains_key(s))
        .collect();

    let selected: Vec<&&str> = if valid_requested.is_empty() {
        DEFAULT_EXPORT_FIELD_KEYS.iter().collect()
    } else {
        // Unique
        let mut seen = std::collections::HashSet::new();
        valid_requested.iter().filter(|s| seen.insert(*s)).collect()
    };

    selected
        .iter()
        .filter_map(|k| defs_by_key.get(*k).copied())
        .collect()
}

fn get_field_value(order: &Order, key: &str) -> String {
    match key {
        "orderNo" => order.order_no.clone(),
        "startDate" => order.start_date.clone(),
        "endDate" => order.end_date.clone(),
        "deliveryDate" => order.delivery_date.clone(),
        "pickupMethods" => pickup_methods_to_string(&order.pickup_methods),
        "address" => order.address.clone(),
        "notes" => order.notes.clone(),
        "devices" => order.devices.join("\u{3001}"),
        "createdAt" => order.created_at.clone(),
        "updatedAt" => String::new(), // Not stored in DB
        _ => String::new(),
    }
}

pub fn format_export_file_name() -> String {
    let ts = time::format_export_timestamp();
    format!("orders-{}.xlsx", ts)
}

pub fn build_export_workbook(orders: &[Order], field_keys: &[String]) -> Result<Vec<u8>, AppError> {
    let defs = resolve_export_field_definitions(field_keys);

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    // Set worksheet name
    worksheet
        .set_name("orders")
        .map_err(|e| AppError::Internal(format!("Failed to set sheet name: {}", e)))?;

    // Write header
    let header_format = Format::new().set_bold();
    for (col, def) in defs.iter().enumerate() {
        worksheet
            .write_string_with_format(0, col as u16, def.label, &header_format)
            .map_err(|e| AppError::Internal(format!("Failed to write header: {}", e)))?;
    }

    // Write data rows
    for (row_idx, order) in orders.iter().enumerate() {
        let excel_row = (row_idx + 1) as u32;
        for (col, def) in defs.iter().enumerate() {
            let value = get_field_value(order, def.key);
            worksheet
                .write_string(excel_row, col as u16, &value)
                .map_err(|e| AppError::Internal(format!("Failed to write cell: {}", e)))?;
        }
    }

    // Write to buffer
    let buffer = workbook
        .save_to_buffer()
        .map_err(|e| AppError::Internal(format!("Failed to save workbook: {}", e)))?;

    Ok(buffer)
}

// ── Dashboard stats ─────────────────────────────────────────────────

pub fn get_dashboard_stats(
    pool: &Pool<SqliteConnectionManager>,
) -> Result<serde_json::Value, AppError> {
    const OVERDUE_GRACE_DAYS: i64 = 4;
    let conn = pool.get()?;
    let today = time::shanghai_now_date_key();

    let active_orders: i64 = conn.query_row(
        "SELECT COUNT(*) FROM orders WHERE startDate <= ?1 AND endDate >= ?1 AND status != 'completed'",
        params![today],
        |row| row.get(0),
    )?;

    let devices_out: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT od.serialNo) FROM order_devices od JOIN orders o ON o.id = od.orderId WHERE o.startDate <= ?1 AND o.endDate >= ?1 AND o.status != 'completed'",
        params![today],
        |row| row.get(0),
    )?;

    let total_devices: i64 =
        conn.query_row("SELECT COUNT(*) FROM devices", [], |row| row.get(0))?;

    let returns_due_today: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT od.serialNo) FROM order_devices od JOIN orders o ON o.id = od.orderId WHERE o.endDate = ?1 AND o.status != 'completed'",
        params![today],
        |row| row.get(0),
    )?;

    let overdue_returns: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT od.serialNo) FROM order_devices od JOIN orders o ON o.id = od.orderId WHERE o.endDate < date(?1, '-' || ?2 || ' days') AND o.status != 'completed'",
        params![today, OVERDUE_GRACE_DAYS],
        |row| row.get(0),
    )?;

    let today_new_orders: i64 = conn.query_row(
        "SELECT COUNT(*) FROM orders WHERE date(createdAt) = ?1",
        params![today],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT id, orderNo, startDate, endDate, province, totalPrice, createdAt FROM orders ORDER BY createdAt DESC LIMIT 5",
    )?;
    let recent_orders: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "orderNo": row.get::<_, String>(1)?,
                "startDate": row.get::<_, String>(2)?,
                "endDate": row.get::<_, String>(3)?,
                "province": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                "totalPrice": row.get::<_, f64>(5)?,
                "createdAt": row.get::<_, String>(6)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt2 = conn.prepare(
        "SELECT od.serialNo, o.orderNo, o.endDate, o.province,
                CAST(julianday(?1) - julianday(o.endDate) AS INTEGER) as daysOverdue
         FROM order_devices od
         JOIN orders o ON o.id = od.orderId
         WHERE o.endDate < date(?2, '-' || ?3 || ' days') AND o.status != 'completed'
         ORDER BY o.endDate ASC",
    )?;
    let overdue_raw: Vec<serde_json::Value> = stmt2
        .query_map(params![today, today, OVERDUE_GRACE_DAYS], |row| {
            Ok(serde_json::json!({
                "serialNo": row.get::<_, String>(0)?,
                "orderNo": row.get::<_, String>(1)?,
                "endDate": row.get::<_, String>(2)?,
                "province": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                "daysOverdue": row.get::<_, i64>(4)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Merge per serialNo — same device may appear in multiple active orders
    use std::collections::HashMap;
    let mut overdue_map: HashMap<String, serde_json::Value> = HashMap::new();
    for row in overdue_raw {
        let sn = row["serialNo"].as_str().unwrap_or("").to_string();
        let on = row["orderNo"].as_str().unwrap_or("").to_string();
        let ed = row["endDate"].as_str().unwrap_or("").to_string();
        let prov = row["province"].as_str().unwrap_or("").to_string();
        let days: i64 = row["daysOverdue"].as_i64().unwrap_or(0);
        if let Some(existing) = overdue_map.get_mut(&sn) {
            let ex_days = existing["daysOverdue"].as_i64().unwrap_or(0);
            if days > ex_days {
                existing["daysOverdue"] = serde_json::json!(days);
            }
            if ed.as_str() < existing["endDate"].as_str().unwrap_or("") {
                existing["endDate"] = serde_json::json!(ed);
            }
            if let Some(arr) = existing["orderNos"].as_array_mut() {
                arr.push(serde_json::json!(on));
            }
        } else {
            overdue_map.insert(
                sn,
                serde_json::json!({
                    "serialNo": row["serialNo"],
                    "orderNos": [on],
                    "endDate": ed,
                    "province": prov,
                    "daysOverdue": days,
                }),
            );
        }
    }
    let overdue_details: Vec<serde_json::Value> = overdue_map.into_values().collect();

    // ── Order buckets by status (for dashboard tabs / HUD scene) ──
    let mut stmt_buckets =
        conn.prepare("SELECT status, COUNT(*) as cnt FROM orders GROUP BY status")?;
    let order_buckets: Vec<serde_json::Value> = stmt_buckets
        .query_map([], |row| {
            Ok(serde_json::json!({
                "status": row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                "count": row.get::<_, i64>(1)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // ── Scene nodes (for HUD isometric SVG) ──
    let scene_nodes: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "id": "node-active",
            "kind": "device-cluster",
            "label": "在租设备",
            "status": "normal",
            "count": devices_out,
            "route": {"name": "devices"}
        }),
        serde_json::json!({
            "id": "node-returns",
            "kind": "return-risk",
            "label": "今日待还",
            "status": if returns_due_today > 8 { "warning" } else { "normal" },
            "count": returns_due_today,
            "route": {"name": "customers", "query": {"endDate": today.to_string()}}
        }),
        serde_json::json!({
            "id": "node-overdue",
            "kind": "return-risk",
            "label": "逾期设备",
            "status": if overdue_returns > 0 { "critical" } else { "normal" },
            "count": overdue_returns,
            "route": {"name": "customers"}
        }),
    ];

    Ok(serde_json::json!({
        "version": 2,
        "asOf": today,
        "activeOrders": active_orders,
        "devicesOut": devices_out,
        "availableDevices": total_devices - devices_out,
        "totalDevices": total_devices,
        "returnsDueToday": returns_due_today,
        "overdueReturns": overdue_returns,
        "todayNewOrders": today_new_orders,
        "recentOrders": recent_orders,
        "overdueDetails": overdue_details,
        "orderBuckets": order_buckets,
        "sceneNodes": scene_nodes,
    }))
}

/// Return order occupancy counts for the next `days` days (default 15).
/// Each entry: { dateKey: "YYYY-MM-DD", count: i64 }
pub fn get_daily_order_counts(
    pool: &Pool<SqliteConnectionManager>,
    days: i64,
) -> Result<Vec<serde_json::Value>, AppError> {
    let days = days.clamp(1, 60);
    let conn = pool.get()?;
    let today = time::shanghai_now_date_key();
    let last_date = time::add_days_to_date_key(&today, days - 1);

    // Fetch all orders active in the window
    let mut stmt = conn.prepare(
        "SELECT startDate, endDate FROM orders WHERE startDate <= ?1 AND endDate >= ?2 AND status != 'completed'",
    )?;
    let active_orders: Vec<(String, String)> = stmt
        .query_map(params![last_date, today], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut rows = Vec::with_capacity(days as usize);
    for i in 0..days {
        let date_key = time::add_days_to_date_key(&today, i);
        let count = active_orders
            .iter()
            .filter(|(s, e)| s.as_str() <= date_key.as_str() && e.as_str() >= date_key.as_str())
            .count() as i64;
        rows.push(serde_json::json!({ "dateKey": date_key, "count": count }));
    }

    Ok(rows)
}

/// idempotent overdue task seeder
pub fn seed_overdue_tasks(pool: &Pool<SqliteConnectionManager>) -> Result<usize, AppError> {
    let today = time::shanghai_now_date_key();
    let now = time::shanghai_now_iso();
    crate::repositories::MaintenanceCompatibilityRepository::new(pool.clone())
        .seed_overdue_tasks(&today, &now)
}
