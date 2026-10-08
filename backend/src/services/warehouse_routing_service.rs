use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::Serialize;

use crate::error::AppError;

const OCCUPANCY_COEFFICIENT_SHIPPING: f64 = 0.2;
const OCCUPANCY_COEFFICIENT_NORMAL: f64 = 1.0;
const OCCUPANCY_COEFFICIENT_RETURN: f64 = 0.2;

// ── Warehouse routing structures ─────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRoute {
    pub send_warehouse_id: String,
    pub send_warehouse_name: String,
    pub return_warehouse_id: String,
    pub return_warehouse_name: String,
    pub shipping_days: i64,
    pub return_days: i64,
}

#[derive(Debug, Clone)]
struct RegionRule {
    warehouse_id: String,
    warehouse_name: String,
    warehouse_type: String,
    enabled: bool,
    province: String,
    shipping_days: i64,
    return_days: i64,
}

// ── Occupancy coefficients ───────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct OccupancyCoefficients {
    pub shipping: f64,
    pub normal: f64,
    #[serde(rename = "return")]
    pub r#return: f64,
}

pub fn get_occupancy_coefficients() -> OccupancyCoefficients {
    OccupancyCoefficients {
        shipping: OCCUPANCY_COEFFICIENT_SHIPPING,
        normal: OCCUPANCY_COEFFICIENT_NORMAL,
        r#return: OCCUPANCY_COEFFICIENT_RETURN,
    }
}

// ── Warehouse resolution ─────────────────────────────────────────

/// Resolve the optimal send and return warehouses for a given province.
///
/// Routing rules:
/// - 南昌友仓 (partner): only serves 湖南, 湖北, 江西; no cross-routing
/// - 上海仓 + 珠海仓 (owned): serve remaining provinces; cross-routing allowed
/// - Send warehouse: min shippingDays among eligible warehouses
/// - Return warehouse: min returnDays among eligible warehouses
///   If send warehouse is owned and no return route exists for province,
///   the other owned warehouse can handle the return.
pub fn resolve_warehouse_route(
    pool: &Pool<SqliteConnectionManager>,
    province: &str,
) -> Result<Option<WarehouseRoute>, AppError> {
    let normalized = province.trim();
    if normalized.is_empty() {
        return Ok(None);
    }

    let conn = pool.get()?;

    // Get all region rules for this province, joined with warehouse info
    let mut stmt = conn.prepare(
        "SELECT wrr.warehouseId, w.name, w.type, w.enabled,
                wrr.province, wrr.shippingDays, wrr.returnDays
         FROM warehouse_region_rules wrr
         JOIN warehouses w ON w.id = wrr.warehouseId
         WHERE wrr.province = ?1 AND w.enabled = 1
         ORDER BY wrr.shippingDays ASC",
    )?;

    let rules: Vec<RegionRule> = stmt
        .query_map(params![normalized], |row| {
            Ok(RegionRule {
                warehouse_id: row.get(0)?,
                warehouse_name: row.get(1)?,
                warehouse_type: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0,
                province: row.get(4)?,
                shipping_days: row.get(5)?,
                return_days: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rules.is_empty() {
        return Ok(None);
    }

    let owned_rules: Vec<&RegionRule> = rules
        .iter()
        .filter(|r| r.warehouse_type == "owned")
        .collect();

    // Best send warehouse: minimum shippingDays
    // (rules are already sorted by shippingDays ASC)
    let best_send = &rules[0];
    let shipping_days = best_send.shipping_days;

    // Best return warehouse: minimum returnDays
    // For owned warehouses, allow cross-routing
    let mut best_return: Option<&RegionRule> = None;
    let mut return_days = i64::MAX;

    for rule in &rules {
        let is_this_owned = rule.warehouse_type == "owned";

        // Partner warehouses only accept returns if they were the send warehouse
        if !is_this_owned && rule.warehouse_id != best_send.warehouse_id {
            continue;
        }

        // Owned warehouses can cross-route
        if rule.return_days < return_days {
            return_days = rule.return_days;
            best_return = Some(rule);
        }
    }

    // If no direct return rule found for owned send warehouse, check other owned warehouses
    if best_return.is_none() && best_send.warehouse_type == "owned" {
        let other_owned = owned_rules
            .iter()
            .find(|r| r.warehouse_id != best_send.warehouse_id);
        if let Some(other) = other_owned {
            best_return = Some(other);
            return_days = other.return_days;
        }
    }

    // Fallback: use send warehouse for return
    let best_return = best_return.unwrap_or(best_send);
    if return_days == i64::MAX {
        return_days = best_send.return_days;
    }

    Ok(Some(WarehouseRoute {
        send_warehouse_id: best_send.warehouse_id.clone(),
        send_warehouse_name: best_send.warehouse_name.clone(),
        return_warehouse_id: best_return.warehouse_id.clone(),
        return_warehouse_name: best_return.warehouse_name.clone(),
        shipping_days,
        return_days,
    }))
}
