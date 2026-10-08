// Multi-tenant query helpers
//
// This module provides tenant-aware versions of core service functions.
// These functions ensure data isolation by adding tenant_id filtering to all queries.
//
// Usage in routes (after tenant middleware is integrated):
// ```rust
// use crate::middleware::tenant_extractors::TenantUser;
//
// async fn list_orders(TenantUser(tenant, _user): TenantUser) -> Result<Json<Response>> {
//     let orders = tenant_helpers::query_orders_for_tenant(&pool, &tenant.id, &filters)?;
//     Ok(Json(orders))
// }
// ```

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde_json::Value;
use system_core::DataScope;

use crate::error::AppError;

/// Filter values for SQL WHERE clause construction.
pub struct FilterValues {
    pub where_sql: String,
    pub values: Vec<String>,
}

/// Build filter SQL with tenant_id as the first condition.
///
/// This is a tenant-aware version of build_user_filter_sql that ensures
/// tenant_id is always included as the primary filter.
pub fn build_tenant_filter_sql(scope: &DataScope, filter: &Value) -> FilterValues {
    let mut where_clauses: Vec<String> = vec!["tenant_id = ?".to_string()];
    let mut values: Vec<String> = vec![scope.tenant_id().as_str().to_string()];

    // Keyword search
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

    // orderNo filter
    if let Some(on) = filter["orderNo"].as_str() {
        let trimmed = on.trim();
        if !trimmed.is_empty() {
            where_clauses.push("orderNo LIKE ?".to_string());
            values.push(format!("%{}%", trimmed));
        }
    }

    // address filter
    if let Some(addr) = filter["address"].as_str() {
        let trimmed = addr.trim();
        if !trimmed.is_empty() {
            where_clauses.push("address LIKE ?".to_string());
            values.push(format!("%{}%", trimmed));
        }
    }

    // startDate range filters
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

    // status filter
    if let Some(status) = filter["status"].as_str() {
        let trimmed = status.trim();
        if !trimmed.is_empty() {
            where_clauses.push("status = ?".to_string());
            values.push(trimmed.to_string());
        }
    }

    // province filter
    if let Some(province) = filter["province"].as_str() {
        let trimmed = province.trim();
        if !trimmed.is_empty() {
            where_clauses.push("province = ?".to_string());
            values.push(trimmed.to_string());
        }
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    FilterValues { where_sql, values }
}

/// Find a single order by ID for a specific tenant.
///
/// Returns None if the order doesn't exist or belongs to a different tenant.
pub fn find_order_by_id_for_tenant(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    order_id: &str,
) -> Result<Option<Value>, AppError> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(1) FROM orders WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), order_id],
        |row| row.get(0),
    )?;

    if count == 0 {
        return Ok(None);
    }

    // Use existing service function to get full order details
    crate::services::order_compatibility_support::find_order_by_id(pool, order_id)
}

/// Find a single order by order number for a specific tenant.
pub fn find_order_by_order_no_for_tenant(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    order_no: &str,
) -> Result<Option<Value>, AppError> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(1) FROM orders WHERE tenant_id = ?1 AND orderNo = ?2",
        params![scope.tenant_id().as_str(), order_no],
        |row| row.get(0),
    )?;

    if count == 0 {
        return Ok(None);
    }

    // Use existing service function to get full order details
    crate::services::order_compatibility_support::find_order_by_order_no(pool, order_no)
}

/// Verify that an order belongs to the specified tenant.
///
/// This is used for cross-tenant access prevention before update/delete operations.
pub fn verify_order_tenant(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    order_id: &str,
) -> Result<bool, AppError> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(1) FROM orders WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), order_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Verify that a device belongs to the specified tenant.
pub fn verify_device_tenant(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    serial_no: &str,
) -> Result<bool, AppError> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(1) FROM devices WHERE tenant_id = ?1 AND serial_no = ?2",
        params![scope.tenant_id().as_str(), serial_no],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Verify that a warehouse belongs to the specified tenant.
pub fn verify_warehouse_tenant(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    warehouse_id: &str,
) -> Result<bool, AppError> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(1) FROM warehouses WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), warehouse_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{Namespace, Revision, TenantId};

    fn scope() -> DataScope {
        DataScope::new(
            TenantId::new("tenant_123").unwrap(),
            Namespace::production(),
            Revision::new("test-revision").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn test_build_tenant_filter_sql_basic() {
        let filter = serde_json::json!({});
        let result = build_tenant_filter_sql(&scope(), &filter);

        assert_eq!(result.where_sql, "WHERE tenant_id = ?");
        assert_eq!(result.values.len(), 1);
        assert_eq!(result.values[0], "tenant_123");
    }

    #[test]
    fn test_build_tenant_filter_sql_with_status() {
        let filter = serde_json::json!({
            "status": "active"
        });
        let result = build_tenant_filter_sql(&scope(), &filter);

        assert!(result.where_sql.contains("tenant_id = ?"));
        assert!(result.where_sql.contains("status = ?"));
        assert_eq!(result.values.len(), 2);
        assert_eq!(result.values[0], "tenant_123");
        assert_eq!(result.values[1], "active");
    }
}
