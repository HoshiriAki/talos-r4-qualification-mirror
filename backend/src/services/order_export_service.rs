use std::collections::{HashMap, HashSet};

use serde_json::Value;
use system_core::ExecutionContext;

use crate::error::AppError;
use crate::repositories::OrderReadProjection;
use crate::services::order_compatibility_support::Order;
use crate::state::AppState;

const EXPORT_PAGE_SIZE: usize = 200;

fn parse_json_vec(raw: &str, field: &str) -> Result<Vec<String>, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(raw)
        .map_err(|error| AppError::Internal(format!("订单导出投影字段 {field} JSON 无效: {error}")))
}

fn parse_json_map(raw: &str, field: &str) -> Result<HashMap<String, i64>, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_str(raw)
        .map_err(|error| AppError::Internal(format!("订单导出投影字段 {field} JSON 无效: {error}")))
}

fn projection_to_export_order(projection: OrderReadProjection) -> Result<Order, AppError> {
    Ok(Order {
        id: projection.id,
        order_no: projection.order_no,
        start_date: projection.start_date,
        end_date: projection.end_date,
        delivery_date: projection.delivery_date,
        pickup_methods: projection.pickup_methods,
        address: projection.address,
        notes: projection.notes,
        device_serial_no: projection.device_serial_no,
        tracking_no: projection.tracking_no,
        province: projection.province,
        total_price: projection.total_price,
        send_warehouse_id: projection.send_warehouse_id,
        return_warehouse_id: projection.return_warehouse_id,
        model_id: String::new(),
        devices: projection.devices,
        accessories: parse_json_vec(&projection.accessories, "accessories")?,
        device_models: parse_json_map(&projection.device_models, "deviceModels")?,
        status: projection.status,
        created_at: projection.created_at,
    })
}

fn projection_from_value(value: Value) -> Result<OrderReadProjection, AppError> {
    serde_json::from_value(value)
        .map_err(|error| AppError::Internal(format!("订单导出投影反序列化失败: {error}")))
}

fn page_orders(value: Value) -> Result<Vec<OrderReadProjection>, AppError> {
    let users = value
        .get("users")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Internal("订单导出分页响应缺少 users".to_string()))?;
    users.iter().cloned().map(projection_from_value).collect()
}

pub fn load_filtered_orders(
    state: &AppState,
    ctx: &ExecutionContext,
    filter: Value,
    max_rows: usize,
) -> Result<Vec<Order>, AppError> {
    if max_rows == 0 {
        return Err(AppError::BadRequest("订单导出行预算必须大于 0".to_string()));
    }

    let first = state
        .registry
        .execute(
            "order_read_compatibility",
            "list_orders",
            serde_json::json!({
                "page": 1,
                "pageSize": EXPORT_PAGE_SIZE,
                "filter": filter,
            }),
            ctx,
        )
        .map_err(AppError::from_error_payload)?;

    let pagination = first
        .get("pagination")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Internal("订单导出分页响应缺少 pagination".to_string()))?;
    let total = pagination
        .get("total")
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::Internal("订单导出分页响应缺少 total".to_string()))?
        as usize;
    if total > max_rows {
        return Err(AppError::BadRequest(format!(
            "订单导出结果 {total} 行，超过上限 {max_rows}；请缩小筛选范围"
        )));
    }
    let total_pages = pagination
        .get("totalPages")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;

    let mut projections = page_orders(first)?;
    if projections.len() > max_rows {
        return Err(AppError::BadRequest(format!(
            "订单导出结果超过上限 {max_rows}"
        )));
    }

    for page in 2..=total_pages {
        let value = state
            .registry
            .execute(
                "order_read_compatibility",
                "list_orders",
                serde_json::json!({
                    "page": page,
                    "pageSize": EXPORT_PAGE_SIZE,
                    "filter": filter,
                }),
                ctx,
            )
            .map_err(AppError::from_error_payload)?;
        let next = page_orders(value)?;
        let next_total = projections
            .len()
            .checked_add(next.len())
            .ok_or_else(|| AppError::BadRequest("订单导出行数溢出".to_string()))?;
        if next_total > max_rows {
            return Err(AppError::BadRequest(format!(
                "订单导出结果超过上限 {max_rows}"
            )));
        }
        projections.extend(next);
    }

    projections
        .into_iter()
        .map(projection_to_export_order)
        .collect()
}

pub fn load_orders_by_ids(
    state: &AppState,
    ctx: &ExecutionContext,
    ids: &[String],
    max_ids: usize,
) -> Result<Vec<Order>, AppError> {
    if ids.len() > max_ids {
        return Err(AppError::BadRequest(format!("ids 数量超过上限 {max_ids}")));
    }

    let mut seen = HashSet::new();
    let normalized: Vec<String> = ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect();

    let mut orders = Vec::with_capacity(normalized.len());
    for id in normalized {
        let value = state
            .registry
            .execute(
                "order_read_compatibility",
                "get_order",
                serde_json::json!({"id": id}),
                ctx,
            )
            .map_err(AppError::from_error_payload)?;
        orders.push(projection_to_export_order(projection_from_value(value)?)?);
    }
    Ok(orders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_page_size_stays_within_registry_read_cap() {
        assert!((1..=200).contains(&EXPORT_PAGE_SIZE));
    }

    #[test]
    fn malformed_projection_json_fails_closed() {
        assert!(parse_json_vec("not-json", "accessories").is_err());
        assert!(parse_json_map("not-json", "deviceModels").is_err());
    }
}
