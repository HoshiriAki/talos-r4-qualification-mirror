use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::Serialize;

use crate::error::AppError;
use crate::utils::time;

fn ensure_warehouse_in_tenant(
    conn: &rusqlite::Connection,
    tenant_id: &str,
    warehouse_id: &str,
) -> Result<(), AppError> {
    let found = conn
        .query_row(
            "SELECT 1 FROM warehouses WHERE id = ?1 AND tenant_id = ?2 LIMIT 1",
            params![warehouse_id, tenant_id],
            |_| Ok(()),
        )
        .is_ok();
    if found {
        Ok(())
    } else {
        Err(AppError::NotFound("仓库不存在".to_string()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Warehouse {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    pub enabled: bool,
    pub address: String,
    #[serde(rename = "contactName")]
    pub contact_name: String,
    #[serde(rename = "contactPhone")]
    pub contact_phone: String,
    pub notes: String,
    pub capacity: i32,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseStats {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    pub enabled: bool,
    pub address: String,
    #[serde(rename = "contactName")]
    pub contact_name: String,
    #[serde(rename = "contactPhone")]
    pub contact_phone: String,
    pub notes: String,
    pub capacity: i32,
    #[serde(rename = "totalDevices")]
    pub total_devices: i64,
    #[serde(rename = "availableDevices")]
    pub available_devices: i64,
    #[serde(rename = "rentedDevices")]
    pub rented_devices: i64,
    #[serde(rename = "repairingDevices")]
    pub repairing_devices: i64,
    #[serde(rename = "utilizationPercent")]
    pub utilization_percent: i64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseDevice {
    pub id: String,
    #[serde(rename = "serialNo")]
    pub serial_no: String,
    #[serde(rename = "rentalStatus")]
    pub rental_status: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub notes: String,
    #[serde(rename = "currentWarehouseId")]
    pub current_warehouse_id: String,
    #[serde(rename = "expectedWarehouseId")]
    pub expected_warehouse_id: String,
    #[serde(rename = "expectedAvailableDate")]
    pub expected_available_date: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseDevicePage {
    pub data: Vec<WarehouseDevice>,
    pub total: i64,
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseRegionRule {
    pub id: String,
    #[serde(rename = "warehouseId")]
    pub warehouse_id: String,
    pub province: String,
    #[serde(rename = "shippingDays")]
    pub shipping_days: i32,
    #[serde(rename = "returnDays")]
    pub return_days: i32,
    #[serde(rename = "isPrimary")]
    pub is_primary: bool,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

// ── Row mapping ─────────────────────────────────────────────────

fn row_to_warehouse(row: &rusqlite::Row) -> rusqlite::Result<Warehouse> {
    Ok(Warehouse {
        id: row.get(0)?,
        name: row.get(1)?,
        wh_type: row.get(2)?,
        enabled: row.get::<_, i32>(3)? != 0,
        address: row.get::<_, String>(4).unwrap_or_default(),
        contact_name: row.get::<_, String>(5).unwrap_or_default(),
        contact_phone: row.get::<_, String>(6).unwrap_or_default(),
        notes: row.get::<_, String>(7).unwrap_or_default(),
        capacity: row.get::<_, i32>(8).unwrap_or(0),
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn row_to_region_rule(row: &rusqlite::Row) -> rusqlite::Result<WarehouseRegionRule> {
    Ok(WarehouseRegionRule {
        id: row.get(0)?,
        warehouse_id: row.get(1)?,
        province: row.get(2)?,
        shipping_days: row.get(3)?,
        return_days: row.get(4)?,
        is_primary: row.get::<_, i32>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

// ── Warehouse CRUD ──────────────────────────────────────────────

pub fn list_warehouses(pool: &Pool<SqliteConnectionManager>) -> Result<Vec<Warehouse>, AppError> {
    let conn = pool.get()?;
    let mut stmt =
        conn.prepare("SELECT id, name, type, enabled, address, contactName, contactPhone, notes, capacity, createdAt, updatedAt FROM warehouses ORDER BY createdAt ASC")?;
    let rows = stmt
        .query_map([], row_to_warehouse)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_warehouse_by_id(
    pool: &Pool<SqliteConnectionManager>,
    id: &str,
) -> Result<Option<Warehouse>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, type, enabled, createdAt, updatedAt FROM warehouses WHERE id = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![id], row_to_warehouse)?;
    Ok(rows.next().transpose()?)
}

pub fn get_warehouse_region_rules(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    warehouse_id: &str,
) -> Result<Vec<WarehouseRegionRule>, AppError> {
    let conn = pool.get()?;
    ensure_warehouse_in_tenant(&conn, tenant_id, warehouse_id)?;
    let mut stmt = conn.prepare(
        "SELECT r.id, r.warehouseId, r.province, r.shippingDays, r.returnDays, r.isPrimary, r.createdAt, r.updatedAt FROM warehouse_region_rules r JOIN warehouses w ON w.id = r.warehouseId WHERE r.warehouseId = ?1 AND w.tenant_id = ?2 ORDER BY r.province ASC",
    )?;
    let rows = stmt
        .query_map(params![warehouse_id, tenant_id], row_to_region_rule)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn create_warehouse(
    pool: &Pool<SqliteConnectionManager>,
    payload: &serde_json::Value,
) -> Result<Warehouse, AppError> {
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("仓库名称不能为空".to_string()));
    }

    let wh_type = payload
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if wh_type != "owned" && wh_type != "partner" {
        return Err(AppError::BadRequest(
            "仓库类型无效，必须是 owned 或 partner".to_string(),
        ));
    }

    let conn = pool.get()?;
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM warehouses WHERE name = ?1 LIMIT 1",
            params![name],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        return Err(AppError::Conflict("仓库名称已存在".to_string()));
    }

    let now = time::shanghai_now_iso();
    let id = uuid::Uuid::new_v4().to_string();
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let address = payload
        .get("address")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let contact_name = payload
        .get("contactName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let contact_phone = payload
        .get("contactPhone")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let notes = payload
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let capacity = payload
        .get("capacity")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .unwrap_or(0) as i32;

    conn.execute(
        "INSERT INTO warehouses (id, name, type, enabled, address, contactName, contactPhone, notes, capacity, createdAt, updatedAt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![id, name, wh_type, enabled as i32, address, contact_name, contact_phone, notes, capacity, now, now],
    )?;

    drop(conn);
    get_warehouse_by_id(pool, &id)?
        .ok_or_else(|| AppError::Internal("创建后查找仓库失败".to_string()))
}

pub fn update_warehouse(
    pool: &Pool<SqliteConnectionManager>,
    id: &str,
    patch: &serde_json::Value,
) -> Result<Warehouse, AppError> {
    let conn = pool.get()?;
    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM warehouses WHERE id = ?1 LIMIT 1",
            params![id],
            |_| Ok(()),
        )
        .is_ok();
    if !existing {
        return Err(AppError::NotFound("仓库不存在".to_string()));
    }

    let now = time::shanghai_now_iso();
    let mut fields: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(name_val) = patch.get("name").and_then(|v| v.as_str()) {
        let name = name_val.trim().to_string();
        if name.is_empty() {
            return Err(AppError::BadRequest("仓库名称不能为空".to_string()));
        }
        let dup: bool = conn
            .query_row(
                "SELECT 1 FROM warehouses WHERE name = ?1 AND id <> ?2 LIMIT 1",
                params![name, id],
                |_| Ok(()),
            )
            .is_ok();
        if dup {
            return Err(AppError::Conflict("仓库名称已存在".to_string()));
        }
        fields.push("name = ?".to_string());
        values.push(Box::new(name));
    }
    if let Some(wh_type) = patch.get("type").and_then(|v| v.as_str()) {
        let t = wh_type.trim().to_string();
        if t != "owned" && t != "partner" {
            return Err(AppError::BadRequest("仓库类型无效".to_string()));
        }
        fields.push("type = ?".to_string());
        values.push(Box::new(t));
    }
    if let Some(enabled) = patch.get("enabled").and_then(|v| v.as_bool()) {
        fields.push("enabled = ?".to_string());
        values.push(Box::new(enabled as i32));
    }
    if let Some(v) = patch.get("address").and_then(|v| v.as_str()) {
        fields.push("address = ?".to_string());
        values.push(Box::new(v.trim().to_string()));
    }
    if let Some(v) = patch.get("contactName").and_then(|v| v.as_str()) {
        fields.push("contactName = ?".to_string());
        values.push(Box::new(v.trim().to_string()));
    }
    if let Some(v) = patch.get("contactPhone").and_then(|v| v.as_str()) {
        fields.push("contactPhone = ?".to_string());
        values.push(Box::new(v.trim().to_string()));
    }
    if let Some(v) = patch.get("notes").and_then(|v| v.as_str()) {
        fields.push("notes = ?".to_string());
        values.push(Box::new(v.trim().to_string()));
    }
    if let Some(n) = patch.get("capacity").and_then(|v| v.as_i64()) {
        let cap = if n >= 0 { n as i32 } else { 0 };
        fields.push("capacity = ?".to_string());
        values.push(Box::new(cap));
    }

    if !fields.is_empty() {
        fields.push("updatedAt = ?".to_string());
        values.push(Box::new(now.clone()));
        values.push(Box::new(id.to_string()));
        let sql = format!("UPDATE warehouses SET {} WHERE id = ?", fields.join(", "));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        conn.execute(&sql, rusqlite::params_from_iter(param_refs))?;
    }

    drop(conn);
    get_warehouse_by_id(pool, id)?
        .ok_or_else(|| AppError::Internal("更新后查找仓库失败".to_string()))
}

pub fn delete_warehouse(pool: &Pool<SqliteConnectionManager>, id: &str) -> Result<(), AppError> {
    let conn = pool.get()?;
    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM warehouses WHERE id = ?1 LIMIT 1",
            params![id],
            |_| Ok(()),
        )
        .is_ok();
    if !existing {
        return Err(AppError::NotFound("仓库不存在".to_string()));
    }

    let device_count: i64 = conn.query_row(
        "SELECT COUNT(1) AS cnt FROM devices WHERE currentWarehouseId = ?1 OR expectedWarehouseId = ?2",
        params![id, id],
        |row| row.get(0),
    )?;
    if device_count > 0 {
        return Err(AppError::Conflict(format!(
            "该仓库下有 {} 台设备，无法删除",
            device_count
        )));
    }

    conn.execute("DELETE FROM warehouses WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Warehouse stats ────────────────────────────────────────────

pub fn get_all_warehouse_stats(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
) -> Result<Vec<WarehouseStats>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT
            w.id, w.name, w.type, w.enabled,
            w.address, w.contactName, w.contactPhone, w.notes, w.capacity,
            w.createdAt, w.updatedAt,
            COUNT(d.id) AS totalDevices,
            SUM(CASE WHEN d.rentalStatus = '已入库' THEN 1 ELSE 0 END) AS availableDevices,
            SUM(CASE WHEN d.rentalStatus = '租赁中' THEN 1 ELSE 0 END) AS rentedDevices,
            SUM(CASE WHEN d.rentalStatus = '返厂维修' THEN 1 ELSE 0 END) AS repairingDevices
        FROM warehouses w
        LEFT JOIN devices d ON d.currentWarehouseId = w.id AND d.tenant_id = w.tenant_id
        WHERE w.enabled = 1 AND w.tenant_id = ?1
        GROUP BY w.id
        ORDER BY w.createdAt ASC",
    )?;

    let rows = stmt.query_map(params![tenant_id], |row| {
        let total: i64 = row.get(11)?;
        let capacity: i32 = row.get(8)?;
        let util = if capacity > 0 {
            ((total as f64 / capacity as f64) * 100.0).round() as i64
        } else {
            0
        };
        Ok(WarehouseStats {
            id: row.get(0)?,
            name: row.get(1)?,
            wh_type: row.get(2)?,
            enabled: row.get::<_, i32>(3)? != 0,
            address: row.get::<_, String>(4).unwrap_or_default(),
            contact_name: row.get::<_, String>(5).unwrap_or_default(),
            contact_phone: row.get::<_, String>(6).unwrap_or_default(),
            notes: row.get::<_, String>(7).unwrap_or_default(),
            capacity,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            total_devices: total,
            available_devices: row.get(12)?,
            rented_devices: row.get(13)?,
            repairing_devices: row.get(14)?,
            utilization_percent: util,
        })
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_warehouse_devices(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    warehouse_id: &str,
    page: i64,
    page_size: i64,
    keyword: Option<&str>,
) -> Result<WarehouseDevicePage, AppError> {
    let conn = pool.get()?;
    ensure_warehouse_in_tenant(&conn, tenant_id, warehouse_id)?;
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    let (where_clause, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(
        kw,
    ) =
        keyword.filter(|s| !s.trim().is_empty())
    {
        let pattern = format!("%{}%", kw.trim());
        (
            "WHERE d.currentWarehouseId = ?1 AND d.tenant_id = ?2 AND (d.serialNo LIKE ?3 OR d.notes LIKE ?4)".to_string(),
            vec![
                Box::new(warehouse_id.to_string()),
                Box::new(tenant_id.to_string()),
                Box::new(pattern.clone()),
                Box::new(pattern),
            ],
        )
    } else {
        (
            "WHERE d.currentWarehouseId = ?1 AND d.tenant_id = ?2".to_string(),
            vec![
                Box::new(warehouse_id.to_string()),
                Box::new(tenant_id.to_string()),
            ],
        )
    };

    let count_sql = format!("SELECT COUNT(*) AS cnt FROM devices d {}", where_clause);
    let total: i64 = {
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|v| v.as_ref()).collect();
        conn.query_row(&count_sql, rusqlite::params_from_iter(param_refs), |r| {
            r.get(0)
        })?
    };

    let offset = (page - 1) * page_size;
    let data_sql = format!(
        "SELECT d.id, d.serialNo, d.rentalStatus, d.modelId, d.notes, d.currentWarehouseId, d.expectedWarehouseId, d.expectedAvailableDate, d.createdAt FROM devices d {} ORDER BY d.createdAt DESC LIMIT ? OFFSET ?",
        where_clause
    );

    let mut data_params: Vec<Box<dyn rusqlite::types::ToSql>> = params_vec;
    data_params.push(Box::new(page_size));
    data_params.push(Box::new(offset));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        data_params.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&data_sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
        Ok(WarehouseDevice {
            id: row.get(0)?,
            serial_no: row.get(1)?,
            rental_status: row.get(2)?,
            model_id: row.get::<_, String>(3).unwrap_or_default(),
            notes: row.get::<_, String>(4).unwrap_or_default(),
            current_warehouse_id: row.get::<_, String>(5).unwrap_or_default(),
            expected_warehouse_id: row.get::<_, String>(6).unwrap_or_default(),
            expected_available_date: row.get::<_, String>(7).unwrap_or_default(),
            created_at: row.get(8)?,
        })
    })?;

    let data: Vec<WarehouseDevice> = rows.filter_map(|r| r.ok()).collect();
    Ok(WarehouseDevicePage {
        data,
        total,
        page,
        page_size,
    })
}

// ── Region rules ────────────────────────────────────────────────

pub fn upsert_region_rule(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    warehouse_id: &str,
    payload: &serde_json::Value,
) -> Result<Vec<WarehouseRegionRule>, AppError> {
    let conn = pool.get()?;
    let wh_exists: bool = conn
        .query_row(
            "SELECT 1 FROM warehouses WHERE id = ?1 AND tenant_id = ?2 LIMIT 1",
            params![warehouse_id, tenant_id],
            |_| Ok(()),
        )
        .is_ok();
    if !wh_exists {
        return Err(AppError::NotFound("仓库不存在".to_string()));
    }

    let province = payload
        .get("province")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if province.is_empty() {
        return Err(AppError::BadRequest("省份不能为空".to_string()));
    }

    let shipping_days = payload
        .get("shippingDays")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .ok_or_else(|| AppError::BadRequest("shippingDays 必须为非负整数".to_string()))?
        as i32;
    let return_days = payload
        .get("returnDays")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .ok_or_else(|| AppError::BadRequest("returnDays 必须为非负整数".to_string()))?
        as i32;

    let is_primary = payload
        .get("isPrimary")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let now = time::shanghai_now_iso();

    let existing_rule: Option<String> = conn
        .query_row(
            "SELECT id FROM warehouse_region_rules WHERE warehouseId = ?1 AND province = ?2 LIMIT 1",
            params![warehouse_id, province],
            |row| row.get(0),
        )
        .ok();

    if let Some(_rule_id) = existing_rule {
        conn.execute(
            "UPDATE warehouse_region_rules SET shippingDays = ?1, returnDays = ?2, isPrimary = ?3, updatedAt = ?4 WHERE warehouseId = ?5 AND province = ?6",
            params![shipping_days, return_days, is_primary as i32, now, warehouse_id, province],
        )?;
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO warehouse_region_rules (id, warehouseId, province, shippingDays, returnDays, isPrimary, createdAt, updatedAt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, warehouse_id, province, shipping_days, return_days, is_primary as i32, now, now],
        )?;
    }

    drop(conn);
    get_warehouse_region_rules(pool, tenant_id, warehouse_id)
}

pub fn delete_region_rule(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &str,
    warehouse_id: &str,
    province: &str,
) -> Result<(), AppError> {
    let conn = pool.get()?;
    let changes = conn.execute(
        "DELETE FROM warehouse_region_rules WHERE warehouseId = ?1 AND province = ?2 AND EXISTS (SELECT 1 FROM warehouses w WHERE w.id = warehouse_region_rules.warehouseId AND w.tenant_id = ?3)",
        params![warehouse_id, province, tenant_id],
    )?;
    if changes == 0 {
        return Err(AppError::NotFound("区域规则不存在".to_string()));
    }
    Ok(())
}
