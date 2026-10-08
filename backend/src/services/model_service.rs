use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::Serialize;
use system_core::DataScope;

use crate::error::AppError;
use crate::utils::time;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceModel {
    pub id: String,
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub enabled: bool,
    #[serde(rename = "weekdayPrice")]
    pub weekday_price: Option<f64>,
    #[serde(rename = "weekendPrice")]
    pub weekend_price: Option<f64>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

fn row_to_model(row: &rusqlite::Row) -> rusqlite::Result<DeviceModel> {
    Ok(DeviceModel {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        prefix: row.get(3)?,
        enabled: row.get::<_, i32>(4)? != 0,
        weekday_price: row.get::<_, Option<f64>>(5)?,
        weekend_price: row.get::<_, Option<f64>>(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const MODEL_COLUMNS: &str = "dm.id, dm.name, dm.category, dm.prefix, dm.enabled, mbp.weekdayPrice, mbp.weekendPrice, dm.createdAt, dm.updatedAt";

const MODEL_FROM_JOIN: &str = "FROM device_models dm LEFT JOIN model_base_prices mbp ON mbp.modelId = dm.id AND mbp.tenant_id = dm.tenant_id";

// ── Queries ─────────────────────────────────────────────────────

pub fn list_models(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
) -> Result<Vec<DeviceModel>, AppError> {
    let conn = pool.get()?;
    let sql = format!(
        "SELECT {} {} WHERE dm.tenant_id = ?1 ORDER BY dm.createdAt ASC",
        MODEL_COLUMNS, MODEL_FROM_JOIN
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![scope.tenant_id().as_str()], row_to_model)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_model_by_id(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    id: &str,
) -> Result<Option<DeviceModel>, AppError> {
    let conn = pool.get()?;
    let sql = format!(
        "SELECT {} {} WHERE dm.tenant_id = ?1 AND dm.id = ?2 LIMIT 1",
        MODEL_COLUMNS, MODEL_FROM_JOIN
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![scope.tenant_id().as_str(), id], row_to_model)?;
    Ok(rows.next().transpose()?)
}

pub fn get_model_by_prefix(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    prefix: &str,
) -> Result<Option<DeviceModel>, AppError> {
    let conn = pool.get()?;
    let sql = format!(
        "SELECT {} {} WHERE dm.tenant_id = ?1 AND dm.prefix = ?2 AND dm.enabled = 1 LIMIT 1",
        MODEL_COLUMNS, MODEL_FROM_JOIN
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![scope.tenant_id().as_str(), prefix], row_to_model)?;
    Ok(rows.next().transpose()?)
}

pub fn detect_model_from_serial(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    serial_no: &str,
) -> Result<Option<DeviceModel>, AppError> {
    let normalized = serial_no.trim().to_uppercase();
    if normalized.is_empty() {
        return Ok(None);
    }

    let conn = pool.get()?;
    let sql = format!(
        "SELECT {} {} WHERE dm.tenant_id = ?1 AND dm.enabled = 1 ORDER BY LENGTH(dm.prefix) DESC",
        MODEL_COLUMNS, MODEL_FROM_JOIN
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<DeviceModel> = stmt
        .query_map(params![scope.tenant_id().as_str()], row_to_model)?
        .filter_map(|r| r.ok())
        .collect();

    for model in rows {
        if normalized.starts_with(&model.prefix.to_uppercase()) {
            return Ok(Some(model));
        }
    }

    Ok(None)
}

// ── CRUD ────────────────────────────────────────────────────────

pub fn create_model(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    payload: &serde_json::Value,
    updated_by: &str,
) -> Result<DeviceModel, AppError> {
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("型号名称不能为空".to_string()));
    }

    let conn = pool.get()?;
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND name = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), name],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        return Err(AppError::Conflict("型号名称已存在".to_string()));
    }

    let now = time::shanghai_now_iso();
    let id = uuid::Uuid::new_v4().to_string();
    let category = payload
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let prefix = payload.get("prefix").and_then(|v| v.as_str()).unwrap_or("");
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Use transaction
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO device_models (id, name, category, prefix, enabled, createdAt, updatedAt, tenant_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, name, category, prefix, enabled as i32, now, now, scope.tenant_id().as_str()],
    )?;

    let weekday_price = payload.get("weekdayPrice").and_then(|v| v.as_f64());
    let weekend_price = payload.get("weekendPrice").and_then(|v| v.as_f64());
    if let (Some(wd), Some(we)) = (weekday_price, weekend_price) {
        tx.execute(
            "INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, tenant_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, wd, we, updated_by, now, now, scope.tenant_id().as_str()],
        )?;
    }

    tx.commit()?;
    drop(conn);

    get_model_by_id(pool, scope, &id)?
        .ok_or_else(|| AppError::Internal("创建后查找型号失败".to_string()))
}

pub fn update_model(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    id: &str,
    patch: &serde_json::Value,
) -> Result<DeviceModel, AppError> {
    let conn = pool.get()?;
    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND id = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), id],
            |_| Ok(()),
        )
        .is_ok();
    if !existing {
        return Err(AppError::NotFound("型号不存在".to_string()));
    }

    let now = time::shanghai_now_iso();
    let mut fields: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(name_val) = patch.get("name").and_then(|v| v.as_str()) {
        let name = name_val.trim().to_string();
        if name.is_empty() {
            return Err(AppError::BadRequest("型号名称不能为空".to_string()));
        }
        let dup: bool = conn
            .query_row(
                "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND name = ?2 AND id <> ?3 LIMIT 1",
                params![scope.tenant_id().as_str(), name, id],
                |_| Ok(()),
            )
            .is_ok();
        if dup {
            return Err(AppError::Conflict("型号名称已存在".to_string()));
        }
        fields.push("name = ?".to_string());
        values.push(Box::new(name));
    }
    if let Some(category) = patch.get("category").and_then(|v| v.as_str()) {
        fields.push("category = ?".to_string());
        values.push(Box::new(category.to_string()));
    }
    if let Some(prefix) = patch.get("prefix").and_then(|v| v.as_str()) {
        fields.push("prefix = ?".to_string());
        values.push(Box::new(prefix.to_string()));
    }
    if let Some(enabled) = patch.get("enabled").and_then(|v| v.as_bool()) {
        fields.push("enabled = ?".to_string());
        values.push(Box::new(enabled as i32));
    }

    if !fields.is_empty() {
        fields.push("updatedAt = ?".to_string());
        values.push(Box::new(now.clone()));
        values.push(Box::new(scope.tenant_id().as_str().to_string()));
        values.push(Box::new(id.to_string()));
        let sql = format!(
            "UPDATE device_models SET {} WHERE tenant_id = ? AND id = ?",
            fields.join(", ")
        );
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        conn.execute(&sql, rusqlite::params_from_iter(param_refs))?;
    }

    drop(conn);
    get_model_by_id(pool, scope, id)?
        .ok_or_else(|| AppError::Internal("更新后查找型号失败".to_string()))
}

pub fn update_model_full(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    id: &str,
    payload: &serde_json::Value,
    updated_by: &str,
) -> Result<DeviceModel, AppError> {
    let conn = pool.get()?;
    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND id = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), id],
            |_| Ok(()),
        )
        .is_ok();
    if !existing {
        return Err(AppError::NotFound("型号不存在".to_string()));
    }

    let now = time::shanghai_now_iso();

    let tx = conn.unchecked_transaction()?;

    // Update model fields
    let mut fields: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(name_val) = payload.get("name").and_then(|v| v.as_str()) {
        let name = name_val.trim().to_string();
        if name.is_empty() {
            return Err(AppError::BadRequest("型号名称不能为空".to_string()));
        }
        let dup: bool = tx
            .query_row(
                "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND name = ?2 AND id <> ?3 LIMIT 1",
                params![scope.tenant_id().as_str(), name, id],
                |_| Ok(()),
            )
            .is_ok();
        if dup {
            return Err(AppError::Conflict("型号名称已存在".to_string()));
        }
        fields.push("name = ?".to_string());
        values.push(Box::new(name));
    }
    if let Some(category) = payload.get("category").and_then(|v| v.as_str()) {
        fields.push("category = ?".to_string());
        values.push(Box::new(category.to_string()));
    }
    if let Some(prefix) = payload.get("prefix").and_then(|v| v.as_str()) {
        fields.push("prefix = ?".to_string());
        values.push(Box::new(prefix.to_string()));
    }
    if let Some(enabled) = payload.get("enabled").and_then(|v| v.as_bool()) {
        fields.push("enabled = ?".to_string());
        values.push(Box::new(enabled as i32));
    }

    if !fields.is_empty() {
        fields.push("updatedAt = ?".to_string());
        values.push(Box::new(now.clone()));
        values.push(Box::new(scope.tenant_id().as_str().to_string()));
        values.push(Box::new(id.to_string()));
        let sql = format!(
            "UPDATE device_models SET {} WHERE tenant_id = ? AND id = ?",
            fields.join(", ")
        );
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        tx.execute(&sql, rusqlite::params_from_iter(param_refs))?;
    }

    // Update pricing if provided
    if payload.get("useGlobalPrice").and_then(|v| v.as_bool()) == Some(true) {
        // Remove custom pricing — fall back to global defaults
        tx.execute(
            "DELETE FROM model_base_prices WHERE tenant_id = ?1 AND modelId = ?2",
            params![scope.tenant_id().as_str(), id],
        )?;
    } else if payload.get("weekdayPrice").is_some() || payload.get("weekendPrice").is_some() {
        let weekday_price = payload
            .get("weekdayPrice")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| AppError::BadRequest("weekdayPrice 必须大于 0".to_string()))?;
        let weekend_price = payload
            .get("weekendPrice")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| AppError::BadRequest("weekendPrice 必须大于 0".to_string()))?;

        let existing_price: bool = tx
            .query_row(
                "SELECT 1 FROM model_base_prices WHERE tenant_id = ?1 AND modelId = ?2 LIMIT 1",
                params![scope.tenant_id().as_str(), id],
                |_| Ok(()),
            )
            .is_ok();

        if existing_price {
            tx.execute(
                "UPDATE model_base_prices SET weekdayPrice = ?1, weekendPrice = ?2, updatedBy = ?3, updatedAt = ?4 WHERE tenant_id = ?5 AND modelId = ?6",
                params![weekday_price, weekend_price, updated_by, now, scope.tenant_id().as_str(), id],
            )?;
        } else {
            tx.execute(
                "INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, tenant_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, weekday_price, weekend_price, updated_by, now, now, scope.tenant_id().as_str()],
            )?;
        }
    }

    tx.commit()?;
    drop(conn);

    get_model_by_id(pool, scope, id)?
        .ok_or_else(|| AppError::Internal("更新后查找型号失败".to_string()))
}

pub fn update_model_pricing(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    model_id: &str,
    payload: &serde_json::Value,
    updated_by: &str,
) -> Result<DeviceModel, AppError> {
    let conn = pool.get()?;
    let model_exists: bool = conn
        .query_row(
            "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND id = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), model_id],
            |_| Ok(()),
        )
        .is_ok();
    if !model_exists {
        return Err(AppError::NotFound("型号不存在".to_string()));
    }

    let weekday_price = payload
        .get("weekdayPrice")
        .and_then(|v| v.as_f64())
        .filter(|n| n.is_finite() && *n > 0.0)
        .ok_or_else(|| AppError::BadRequest("weekdayPrice 必须大于 0".to_string()))?;
    let weekend_price = payload
        .get("weekendPrice")
        .and_then(|v| v.as_f64())
        .filter(|n| n.is_finite() && *n > 0.0)
        .ok_or_else(|| AppError::BadRequest("weekendPrice 必须大于 0".to_string()))?;

    let now = time::shanghai_now_iso();
    let existing_price: bool = conn
        .query_row(
            "SELECT 1 FROM model_base_prices WHERE tenant_id = ?1 AND modelId = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), model_id],
            |_| Ok(()),
        )
        .is_ok();

    if existing_price {
        conn.execute(
            "UPDATE model_base_prices SET weekdayPrice = ?1, weekendPrice = ?2, updatedBy = ?3, updatedAt = ?4 WHERE tenant_id = ?5 AND modelId = ?6",
            params![weekday_price, weekend_price, updated_by, now, scope.tenant_id().as_str(), model_id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, tenant_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![model_id, weekday_price, weekend_price, updated_by, now, now, scope.tenant_id().as_str()],
        )?;
    }

    drop(conn);
    get_model_by_id(pool, scope, model_id)?
        .ok_or_else(|| AppError::Internal("更新后查找型号失败".to_string()))
}

pub fn delete_model(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    id: &str,
) -> Result<(), AppError> {
    let conn = pool.get()?;
    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM device_models WHERE tenant_id = ?1 AND id = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), id],
            |_| Ok(()),
        )
        .is_ok();
    if !existing {
        return Err(AppError::NotFound("型号不存在".to_string()));
    }

    let device_count: i64 = conn.query_row(
        "SELECT COUNT(1) AS cnt FROM devices WHERE tenant_id = ?1 AND modelId = ?2",
        params![scope.tenant_id().as_str(), id],
        |row| row.get(0),
    )?;

    if device_count > 0 {
        return Err(AppError::Conflict(format!(
            "该型号已被 {} 台设备使用，无法删除",
            device_count
        )));
    }

    conn.execute(
        "DELETE FROM device_models WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tenant_isolation_tests {
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{DataScope, Revision, TenantId};

    use super::{get_model_by_id, update_model_pricing};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).unwrap(),
            Revision::new("model-test-revision").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn model_reads_and_mutations_cannot_cross_tenants() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE device_models (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT NOT NULL,
                prefix TEXT NOT NULL, enabled INTEGER NOT NULL, createdAt TEXT NOT NULL,
                updatedAt TEXT NOT NULL, tenant_id TEXT NOT NULL
             );
             CREATE TABLE model_base_prices (
                modelId TEXT PRIMARY KEY, weekdayPrice REAL NOT NULL, weekendPrice REAL NOT NULL,
                updatedBy TEXT DEFAULT '', createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL,
                tenant_id TEXT NOT NULL
             );
             INSERT INTO device_models VALUES
                ('model-a', 'A', 'camera', 'A', 1, 'now', 'now', 'tenant-a'),
                ('model-b', 'B', 'camera', 'B', 1, 'now', 'now', 'tenant-b');
             INSERT INTO model_base_prices VALUES
                ('model-a', 10, 20, '', 'now', 'now', 'tenant-a'),
                ('model-b', 30, 40, '', 'now', 'now', 'tenant-b');",
        )
        .unwrap();
        drop(conn);

        let tenant_a = scope("tenant-a");
        let tenant_b = scope("tenant-b");
        assert!(
            get_model_by_id(&pool, &tenant_a, "model-b")
                .unwrap()
                .is_none()
        );
        assert!(
            update_model_pricing(
                &pool,
                &tenant_a,
                "model-b",
                &serde_json::json!({"weekdayPrice": 99.0, "weekendPrice": 100.0}),
                "admin-a",
            )
            .is_err()
        );
        let untouched = get_model_by_id(&pool, &tenant_b, "model-b")
            .unwrap()
            .unwrap();
        assert_eq!(untouched.weekday_price, Some(30.0));
    }
}
