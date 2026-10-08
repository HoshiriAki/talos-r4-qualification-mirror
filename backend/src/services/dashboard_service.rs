use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use crate::error::AppError;

fn build_granularity_clauses(granularity: &str) -> (&'static str, &'static str) {
    match granularity {
        "week" => (
            "strftime('%Y-%W', createdAt)",
            "strftime('%Y-%m-%d', createdAt, 'weekday 0') as date",
        ),
        "month" => (
            "strftime('%Y-%m', createdAt)",
            "strftime('%Y-%m-01', createdAt) as date",
        ),
        _ => ("date(createdAt)", "date(createdAt) as date"),
    }
}

// ── 1. Order Trend ──────────────────────────────────────────────────

pub fn get_order_trend(
    pool: &Pool<SqliteConnectionManager>,
    granularity: &str,
    days: i64,
) -> Result<serde_json::Value, AppError> {
    let days = days.clamp(1, 365);
    let (group_expr, date_select) = build_granularity_clauses(granularity);

    let sql = format!(
        "SELECT {}, COUNT(*) as count FROM orders WHERE createdAt >= date('now', '-' || ?1 || ' days') GROUP BY {} ORDER BY date",
        date_select, group_expr
    );

    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map(params![days], |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "count": row.get::<_, i64>(1)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({
        "data": rows,
        "granularity": granularity,
        "days": days,
    }))
}

// ── 2. Revenue Trend ─────────────────────────────────────────────────

pub fn get_revenue_trend(
    pool: &Pool<SqliteConnectionManager>,
    granularity: &str,
    days: i64,
) -> Result<serde_json::Value, AppError> {
    let days = days.clamp(1, 365);
    let (group_expr, date_select) = build_granularity_clauses(granularity);

    let sql = format!(
        "SELECT {}, COUNT(*) as orderCount, COALESCE(SUM(totalPrice), 0) as totalRevenue FROM orders WHERE createdAt >= date('now', '-' || ?1 || ' days') GROUP BY {} ORDER BY date",
        date_select, group_expr
    );

    let conn = pool.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map(params![days], |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "orderCount": row.get::<_, i64>(1)?,
                "totalRevenue": row.get::<_, f64>(2)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({
        "data": rows,
        "granularity": granularity,
        "days": days,
    }))
}

// ── 3. Cancel Trend ──────────────────────────────────────────────────

pub fn get_cancel_trend(
    pool: &Pool<SqliteConnectionManager>,
    days: i64,
) -> Result<serde_json::Value, AppError> {
    let days = days.clamp(1, 365);

    let sql = "SELECT date(createdAt) as date, COUNT(*) as count FROM audit_logs WHERE actionType = 'order_delete' AND createdAt >= date('now', '-' || ?1 || ' days') GROUP BY date(createdAt) ORDER BY date";

    let conn = pool.get()?;
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map(params![days], |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "count": row.get::<_, i64>(1)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({
        "data": rows,
        "days": days,
    }))
}

// ── 4. Device Status Distribution ────────────────────────────────────

pub fn get_device_status_distribution(
    pool: &Pool<SqliteConnectionManager>,
) -> Result<serde_json::Value, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT rentalStatus as status, COUNT(*) as count FROM devices GROUP BY rentalStatus",
    )?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "status": row.get::<_, String>(0)?,
                "count": row.get::<_, i64>(1)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({
        "data": rows,
    }))
}

// ── 5. Province Stats ────────────────────────────────────────────────

pub fn get_province_stats(
    pool: &Pool<SqliteConnectionManager>,
    stat_type: &str,
    days: i64,
) -> Result<serde_json::Value, AppError> {
    let days = days.clamp(1, 365);
    let conn = pool.get()?;

    let pie_sql = "SELECT province as name, COUNT(*) as value FROM orders WHERE province IS NOT NULL AND province != '' AND createdAt >= date('now', '-' || ?1 || ' days') GROUP BY province ORDER BY value DESC";
    let mut pie_stmt = conn.prepare(pie_sql)?;
    let pie_rows: Vec<serde_json::Value> = pie_stmt
        .query_map(params![days], |row| {
            Ok(serde_json::json!({
                "name": row.get::<_, String>(0)?,
                "value": row.get::<_, i64>(1)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut trend_rows = Vec::new();
    if stat_type == "trend" {
        let trend_sql = "SELECT date(createdAt) as date, province, COUNT(*) as count FROM orders WHERE province IS NOT NULL AND province != '' AND createdAt >= date('now', '-' || ?1 || ' days') GROUP BY date(createdAt), province ORDER BY date, province";
        let mut trend_stmt = conn.prepare(trend_sql)?;
        trend_rows = trend_stmt
            .query_map(params![days], |row| {
                Ok(serde_json::json!({
                    "date": row.get::<_, String>(0)?,
                    "province": row.get::<_, String>(1)?,
                    "count": row.get::<_, i64>(2)?,
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();
    }

    Ok(serde_json::json!({
        "pie": pie_rows,
        "trend": trend_rows,
        "days": days,
    }))
}

// ── 6. Model Ranking ─────────────────────────────────────────────────

pub fn get_model_ranking(
    pool: &Pool<SqliteConnectionManager>,
    days: i64,
    limit: i64,
) -> Result<serde_json::Value, AppError> {
    let days = days.clamp(1, 365);
    let limit = limit.clamp(1, 100);

    let sql = "SELECT dm.name as modelName, COUNT(od.orderId) as orderCount, COALESCE(SUM(o.totalPrice), 0) as revenue FROM order_devices od JOIN devices d ON d.serialNo = od.serialNo JOIN device_models dm ON dm.id = d.modelId JOIN orders o ON o.id = od.orderId WHERE o.createdAt >= date('now', '-' || ?1 || ' days') GROUP BY dm.name ORDER BY revenue DESC LIMIT ?2";

    let conn = pool.get()?;
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map(params![days, limit], |row| {
            Ok(serde_json::json!({
                "modelName": row.get::<_, String>(0)?,
                "orderCount": row.get::<_, i64>(1)?,
                "revenue": row.get::<_, f64>(2)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({
        "data": rows,
        "days": days,
    }))
}

// ── 7. Warehouse Stats ───────────────────────────────────────────────

pub fn get_warehouse_stats(
    pool: &Pool<SqliteConnectionManager>,
) -> Result<serde_json::Value, AppError> {
    let conn = pool.get()?;

    let mut wh_stmt = conn.prepare("SELECT id, name FROM warehouses")?;
    let warehouses: Vec<(String, String)> = wh_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let in_stock: i64 = conn
        .query_row(
            "SELECT COUNT(*) as cnt FROM devices WHERE rentalStatus = '已入库'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut result = Vec::new();
    for (wh_id, wh_name) in &warehouses {
        let outgoing: i64 = conn
            .query_row(
                "SELECT COUNT(*) as cnt FROM orders WHERE sendWarehouseId = ?1 AND status != 'completed'",
                params![wh_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        result.push(serde_json::json!({
            "warehouseId": wh_id,
            "name": wh_name,
            "outgoing": outgoing,
            "currentStock": in_stock,
        }));
    }

    Ok(serde_json::json!({
        "data": result,
    }))
}
