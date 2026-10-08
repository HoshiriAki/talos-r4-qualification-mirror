use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use system_core::ExecutionContext;
use uuid::Uuid;

use crate::repositories::{
    RepositoryError, RepositoryProvider, UpsertWarehouseRegionRule,
    WarehouseDeviceDetailProjection, WarehouseDevicePage, WarehouseMutationError,
    WarehouseRegionRuleProjection, WarehouseRoutingRuleProjection,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Debug)]
pub enum WarehouseAuthorityError {
    InvalidInput(String),
    NotFound,
    DuplicateName,
    Referenced(u64),
    RegionRuleNotFound,
    Persistence { code: &'static str },
}

impl From<RepositoryError> for WarehouseAuthorityError {
    fn from(value: RepositoryError) -> Self {
        Self::Persistence { code: value.code() }
    }
}

impl From<WarehouseMutationError> for WarehouseAuthorityError {
    fn from(value: WarehouseMutationError) -> Self {
        match value {
            WarehouseMutationError::NotFound => Self::NotFound,
            WarehouseMutationError::DuplicateName => Self::DuplicateName,
            WarehouseMutationError::Referenced(count) => Self::Referenced(count),
            WarehouseMutationError::RegionRuleNotFound => Self::RegionRuleNotFound,
            WarehouseMutationError::Storage(error) => error.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRegionRuleView {
    pub id: String,
    pub warehouse_id: String,
    pub province: String,
    pub shipping_days: i32,
    pub return_days: i32,
    pub is_primary: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<WarehouseRegionRuleProjection> for WarehouseRegionRuleView {
    fn from(value: WarehouseRegionRuleProjection) -> Self {
        Self {
            id: value.id,
            warehouse_id: value.warehouse_id,
            province: value.province,
            shipping_days: value.shipping_days,
            return_days: value.return_days,
            is_primary: value.is_primary,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRouteView {
    pub send_warehouse_id: String,
    pub send_warehouse_name: String,
    pub return_warehouse_id: String,
    pub return_warehouse_name: String,
    pub shipping_days: i64,
    pub return_days: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseDeviceDetailView {
    pub id: String,
    pub serial_no: String,
    pub rental_status: String,
    pub model_id: String,
    pub notes: String,
    pub current_warehouse_id: String,
    pub expected_warehouse_id: String,
    pub expected_available_date: String,
    pub created_at: String,
}

impl From<WarehouseDeviceDetailProjection> for WarehouseDeviceDetailView {
    fn from(value: WarehouseDeviceDetailProjection) -> Self {
        Self {
            id: value.id,
            serial_no: value.serial_no,
            rental_status: value.rental_status,
            model_id: value.model_id,
            notes: value.notes,
            current_warehouse_id: value.current_warehouse_id,
            expected_warehouse_id: value.expected_warehouse_id,
            expected_available_date: value.expected_available_date,
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseDevicePageView {
    pub data: Vec<WarehouseDeviceDetailView>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

impl From<WarehouseDevicePage> for WarehouseDevicePageView {
    fn from(value: WarehouseDevicePage) -> Self {
        Self {
            data: value
                .data
                .into_iter()
                .map(WarehouseDeviceDetailView::from)
                .collect(),
            total: value.total,
            page: value.page,
            page_size: value.page_size,
        }
    }
}

#[derive(Clone)]
pub struct WarehouseAuthorityService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl WarehouseAuthorityService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn resolve_route(
        &self,
        ctx: &ExecutionContext,
        province: &str,
    ) -> Result<Option<WarehouseRouteView>, WarehouseAuthorityError> {
        let province = province.trim();
        if province.is_empty() {
            return Err(WarehouseAuthorityError::InvalidInput("省份不能为空".into()));
        }
        let scoped = self.repository_provider.bind(ctx)?;
        let rules = scoped.warehouses().routing_rules_for_province(province)?;
        Ok(select_route(&rules))
    }

    pub fn region_rules(
        &self,
        ctx: &ExecutionContext,
        warehouse_id: &str,
    ) -> Result<Vec<WarehouseRegionRuleView>, WarehouseAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        scoped
            .warehouses()
            .region_rules(warehouse_id)
            .map(|rules| {
                rules
                    .into_iter()
                    .map(WarehouseRegionRuleView::from)
                    .collect()
            })
            .map_err(Into::into)
    }

    pub fn upsert_region_rule(
        &self,
        ctx: &ExecutionContext,
        warehouse_id: &str,
        body: &Value,
    ) -> Result<Vec<WarehouseRegionRuleView>, WarehouseAuthorityError> {
        let province = body
            .get("province")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| WarehouseAuthorityError::InvalidInput("省份不能为空".into()))?
            .to_owned();
        let shipping_days = non_negative_i32(body, "shippingDays")?;
        let return_days = non_negative_i32(body, "returnDays")?;
        let is_primary = match body.get("isPrimary") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                WarehouseAuthorityError::InvalidInput("isPrimary 必须是布尔值".into())
            })?,
        };

        let scoped = self.repository_provider.bind(ctx)?;
        scoped
            .warehouses()
            .upsert_region_rule(
                warehouse_id,
                &UpsertWarehouseRegionRule {
                    id: Uuid::new_v4().to_string(),
                    province,
                    shipping_days,
                    return_days,
                    is_primary,
                    now: shanghai_now_iso(),
                },
            )
            .map(|rules| {
                rules
                    .into_iter()
                    .map(WarehouseRegionRuleView::from)
                    .collect()
            })
            .map_err(Into::into)
    }

    pub fn delete_region_rule(
        &self,
        ctx: &ExecutionContext,
        warehouse_id: &str,
        province: &str,
    ) -> Result<(), WarehouseAuthorityError> {
        let province = province.trim();
        if province.is_empty() {
            return Err(WarehouseAuthorityError::InvalidInput("省份不能为空".into()));
        }
        let scoped = self.repository_provider.bind(ctx)?;
        scoped
            .warehouses()
            .delete_region_rule(warehouse_id, province)
            .map_err(Into::into)
    }

    pub fn devices_paged(
        &self,
        ctx: &ExecutionContext,
        warehouse_id: &str,
        page: i64,
        page_size: i64,
        keyword: Option<&str>,
    ) -> Result<WarehouseDevicePageView, WarehouseAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        scoped
            .warehouses()
            .devices_paged(warehouse_id, page, page_size, keyword)
            .map(WarehouseDevicePageView::from)
            .map_err(Into::into)
    }
}

fn select_route(rules: &[WarehouseRoutingRuleProjection]) -> Option<WarehouseRouteView> {
    let best_send = rules.first()?;
    let owned_rules = rules
        .iter()
        .filter(|rule| rule.warehouse_type == "owned")
        .collect::<Vec<_>>();

    let mut best_return = None;
    let mut return_days = i32::MAX;
    for rule in rules {
        let is_owned = rule.warehouse_type == "owned";
        if !is_owned && rule.warehouse_id != best_send.warehouse_id {
            continue;
        }
        if rule.return_days < return_days {
            return_days = rule.return_days;
            best_return = Some(rule);
        }
    }

    if best_return.is_none() && best_send.warehouse_type == "owned" {
        if let Some(other) = owned_rules
            .iter()
            .find(|rule| rule.warehouse_id != best_send.warehouse_id)
        {
            best_return = Some(*other);
            return_days = other.return_days;
        }
    }

    let best_return = best_return.unwrap_or(best_send);
    if return_days == i32::MAX {
        return_days = best_send.return_days;
    }

    Some(WarehouseRouteView {
        send_warehouse_id: best_send.warehouse_id.clone(),
        send_warehouse_name: best_send.warehouse_name.clone(),
        return_warehouse_id: best_return.warehouse_id.clone(),
        return_warehouse_name: best_return.warehouse_name.clone(),
        shipping_days: i64::from(best_send.shipping_days),
        return_days: i64::from(return_days),
    })
}

fn non_negative_i32(body: &Value, field: &str) -> Result<i32, WarehouseAuthorityError> {
    let value = body
        .get(field)
        .and_then(Value::as_i64)
        .filter(|value| *value >= 0 && *value <= i32::MAX as i64)
        .ok_or_else(|| WarehouseAuthorityError::InvalidInput(format!("{field} 必须为非负整数")))?;
    Ok(value as i32)
}

#[cfg(test)]
mod tests {
    use super::{WarehouseAuthorityError, non_negative_i32, select_route};
    use crate::repositories::WarehouseRoutingRuleProjection;

    #[test]
    fn route_selection_preserves_owned_cross_routing_and_partner_affinity() {
        let rules = vec![
            WarehouseRoutingRuleProjection {
                warehouse_id: "partner".into(),
                warehouse_name: "Partner".into(),
                warehouse_type: "partner".into(),
                shipping_days: 1,
                return_days: 9,
            },
            WarehouseRoutingRuleProjection {
                warehouse_id: "owned-a".into(),
                warehouse_name: "Owned A".into(),
                warehouse_type: "owned".into(),
                shipping_days: 2,
                return_days: 4,
            },
            WarehouseRoutingRuleProjection {
                warehouse_id: "owned-b".into(),
                warehouse_name: "Owned B".into(),
                warehouse_type: "owned".into(),
                shipping_days: 3,
                return_days: 2,
            },
        ];
        let route = select_route(&rules).expect("routing rules resolve");
        assert_eq!(route.send_warehouse_id, "partner");
        assert_eq!(route.return_warehouse_id, "owned-b");
        assert_eq!(route.shipping_days, 1);
        assert_eq!(route.return_days, 2);
    }

    #[test]
    fn region_rule_days_are_bounded_non_negative_integers() {
        assert_eq!(
            non_negative_i32(&serde_json::json!({"days": 3}), "days").unwrap(),
            3
        );
        assert!(matches!(
            non_negative_i32(&serde_json::json!({"days": -1}), "days"),
            Err(WarehouseAuthorityError::InvalidInput(_))
        ));
    }
}
