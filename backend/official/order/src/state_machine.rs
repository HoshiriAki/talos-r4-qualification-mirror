//! Legacy Order status compatibility vocabulary.
//!
//! R1-P5 moved lifecycle write authority to `order_lifecycle_v2`, where Commercial,
//! Contract, Financial, Fulfilment and Risk are independent dimensions. The values and
//! labels in this module remain for the bounded `orders.status` compatibility projection.
//! The old generic transition guard is deliberately fail-closed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod status {
    pub const DRAFT: &str = "draft";
    pub const CONFIRMED: &str = "confirmed";
    pub const PAID: &str = "paid";
    pub const SHIPPED: &str = "shipped";
    pub const IN_USE: &str = "in_use";
    pub const RETURNED: &str = "returned";
    pub const INSPECTED: &str = "inspected";
    pub const COMPLETED: &str = "completed";
    pub const CLOSED: &str = "closed";
    pub const CANCELLED: &str = "cancelled";
    pub const REPAIRING: &str = "repairing";
}

/// Historical transition graph retained only for compatibility inspection/tests. It is no
/// longer a write-policy authority; callers cannot execute it through `guard_transition`.
pub const TRANSITIONS: &[(&str, &[&str])] = &[
    (status::DRAFT, &[status::CONFIRMED, status::CANCELLED]),
    (status::CONFIRMED, &[status::PAID, status::CANCELLED]),
    (status::PAID, &[status::SHIPPED, status::CANCELLED]),
    (status::SHIPPED, &[status::IN_USE, status::CANCELLED]),
    (status::IN_USE, &[status::RETURNED]),
    (status::RETURNED, &[status::INSPECTED]),
    (status::INSPECTED, &[status::COMPLETED, status::REPAIRING]),
    (status::REPAIRING, &[status::COMPLETED]),
    (status::COMPLETED, &[status::CLOSED]),
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransitionInput {
    pub order_id: String,
    pub to_status: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusHistoryEntry {
    pub id: String,
    pub order_id: String,
    pub from_status: String,
    pub to_status: String,
    pub operator: String,
    pub reason: String,
    pub created_at: String,
}

pub fn is_valid_transition(from: &str, to: &str) -> bool {
    for &(src, dsts) in TRANSITIONS {
        if src == from && dsts.contains(&to) {
            return true;
        }
    }
    false
}

pub fn allowed_next(from: &str) -> Vec<&'static str> {
    for &(src, dsts) in TRANSITIONS {
        if src == from {
            return dsts.to_vec();
        }
    }
    vec![]
}

/// Generic single-axis lifecycle mutation is retired. Named R1-P5 guards own all new writes.
pub fn guard_transition(_from: &str, _to: &str, _extra_checks: &[bool]) -> Result<(), String> {
    Err("BIZ_LEGACY_ORDER_TRANSITION_RETIRED: use order_lifecycle_v2 named actions".into())
}

impl system_core::Sanitize for TransitionInput {
    fn sanitize(&mut self) {
        self.order_id = self.order_id.trim().to_string();
        self.to_status = self.to_status.trim().to_lowercase();
        self.reason = self.reason.trim().to_string();
    }
}

impl system_core::Validate for TransitionInput {
    fn validate(&self) -> system_core::ValidationResult {
        let mut errors = Vec::new();
        if self.order_id.is_empty() {
            errors.push(system_core::FieldError {
                field: "orderId".into(),
                code: "VAL_REQUIRED".into(),
                message: "订单 ID 不能为空".into(),
            });
        }
        if self.to_status.is_empty() {
            errors.push(system_core::FieldError {
                field: "toStatus".into(),
                code: "VAL_REQUIRED".into(),
                message: "目标状态不能为空".into(),
            });
        }
        system_core::ValidationResult { errors }
    }
}

pub fn display_label(db_value: &str) -> &str {
    match db_value {
        status::DRAFT => "草稿",
        status::CONFIRMED => "已确认",
        status::PAID => "已付款",
        status::SHIPPED => "已发货",
        status::IN_USE => "使用中",
        status::RETURNED => "已归还",
        status::INSPECTED => "检查中",
        status::REPAIRING => "维修中",
        status::COMPLETED => "已完成",
        status::CLOSED => "已关闭",
        status::CANCELLED => "已取消",
        _ => db_value,
    }
}

#[cfg(test)]
mod tests {
    use super::{allowed_next, guard_transition, is_valid_transition, status};

    #[test]
    fn historical_graph_remains_available_as_compatibility_vocabulary() {
        assert!(is_valid_transition(status::DRAFT, status::CONFIRMED));
        assert!(is_valid_transition(status::INSPECTED, status::REPAIRING));
        assert!(!is_valid_transition(status::CLOSED, status::DRAFT));
        assert!(allowed_next(status::CANCELLED).is_empty());
    }

    #[test]
    fn generic_transition_write_guard_is_retired_fail_closed() {
        let error = guard_transition(status::DRAFT, status::CONFIRMED, &[true]).unwrap_err();
        assert!(error.contains("BIZ_LEGACY_ORDER_TRANSITION_RETIRED"));
    }
}
