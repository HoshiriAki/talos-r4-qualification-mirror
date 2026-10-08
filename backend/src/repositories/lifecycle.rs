use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::workflow::append_outbox_tx;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderLifecycleProjection {
    pub order_id: String,
    pub commercial_status: String,
    pub contract_status: String,
    pub financial_status: String,
    pub fulfilment_status: String,
    pub risk_status: String,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleAllowedAction {
    pub action: String,
    pub dimension: String,
    pub target_status: String,
    pub label: String,
    pub expected_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleOperationalView {
    pub lifecycle: OrderLifecycleProjection,
    pub allowed_actions: Vec<LifecycleAllowedAction>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleHistoryProjection {
    pub id: String,
    pub order_id: String,
    pub dimension: String,
    pub from_status: String,
    pub to_status: String,
    pub guard_name: String,
    pub actor_user_id: String,
    pub reason: String,
    pub expected_version: i64,
    pub resulting_version: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleMigrationExceptionProjection {
    pub id: String,
    pub order_id: String,
    pub legacy_status: String,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

pub struct ScopedOrderLifecycleRepository<'a> {
    session: &'a SqliteRepositorySession,
}

#[derive(Debug, Clone)]
struct ActionChange {
    dimension: &'static str,
    from_status: String,
    to_status: &'static str,
}

#[derive(Debug, Clone)]
struct ActionPlan {
    guard_name: &'static str,
    changes: Vec<ActionChange>,
    legacy_status: Option<&'static str>,
}

impl<'a> ScopedOrderLifecycleRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn get(&self, order_id: &str) -> Result<Option<OrderLifecycleProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        self.session
            .read(|connection| load_lifecycle(connection, &tenant_id, &order_id))
    }

    pub fn operational_view(
        &self,
        order_id: &str,
    ) -> Result<LifecycleOperationalView, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        if order_id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "orderId must not be blank".into(),
            ));
        }
        self.session
            .read(|connection| {
                let lifecycle = load_lifecycle(connection, &tenant_id, &order_id)?
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                let allowed_actions = allowed_actions(connection, &tenant_id, &lifecycle)?;
                let blockers = readiness_blockers(connection, &tenant_id, &lifecycle)?;
                Ok(LifecycleOperationalView {
                    lifecycle,
                    allowed_actions,
                    blockers,
                })
            })
            .map_err(|error| match error {
                RepositoryError::Sqlite(detail) if detail.contains("Query returned no rows") => {
                    RepositoryError::ContractViolation("order lifecycle not found".into())
                }
                other => other,
            })
    }

    pub fn history(
        &self,
        order_id: &str,
    ) -> Result<Vec<LifecycleHistoryProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, order_id, dimension, from_status, to_status, guard_name,
                        actor_user_id, reason, expected_version, resulting_version, created_at
                 FROM order_lifecycle_history
                 WHERE tenant_id = ?1 AND order_id = ?2
                 ORDER BY resulting_version, created_at, id",
            )?;
            let rows = statement.query_map(params![tenant_id, order_id], |row| {
                Ok(LifecycleHistoryProjection {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    dimension: row.get(2)?,
                    from_status: row.get(3)?,
                    to_status: row.get(4)?,
                    guard_name: row.get(5)?,
                    actor_user_id: row.get(6)?,
                    reason: row.get(7)?,
                    expected_version: row.get(8)?,
                    resulting_version: row.get(9)?,
                    created_at: row.get(10)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn list_migration_exceptions(
        &self,
    ) -> Result<Vec<LifecycleMigrationExceptionProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, order_id, legacy_status, reason, status, created_at
                 FROM lifecycle_migration_exceptions
                 WHERE tenant_id = ?1
                 ORDER BY created_at, id",
            )?;
            let rows = statement.query_map(params![tenant_id], |row| {
                Ok(LifecycleMigrationExceptionProjection {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    legacy_status: row.get(2)?,
                    reason: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn apply_action(
        &self,
        order_id: &str,
        action: &str,
        expected_version: i64,
        actor_user_id: &str,
        reason: &str,
    ) -> Result<LifecycleOperationalView, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        let action = action.trim().to_owned();
        if order_id.is_empty() || action.is_empty() || expected_version < 1 {
            return Err(RepositoryError::ContractViolation(
                "orderId, named action and positive expectedVersion are required".into(),
            ));
        }
        let actor_user_id = actor_user_id.trim().to_owned();
        let reason = reason.trim().to_owned();
        let updated = self.session.write_immediate(|transaction| {
            apply_action_in_transaction(
                transaction,
                &tenant_id,
                &order_id,
                &action,
                expected_version,
                &actor_user_id,
                &reason,
            )
        })?;

        self.operational_view(&updated.order_id)
    }
}

/// Shared named lifecycle writer for composed business transactions.  Callers
/// must pass the trusted tenant and the transaction they already own; this
/// preserves the lifecycle history and legacy-order projection atomically.
pub(crate) fn apply_action_in_transaction(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    order_id: &str,
    action: &str,
    expected_version: i64,
    actor_user_id: &str,
    reason: &str,
) -> Result<OrderLifecycleProjection, RepositoryError> {
    if order_id.trim().is_empty() || action.trim().is_empty() || expected_version < 1 {
        return Err(RepositoryError::ContractViolation(
            "orderId, named action and positive expectedVersion are required".into(),
        ));
    }
    let current = load_lifecycle(transaction, tenant_id, order_id)
        .map_err(sqlite_error)?
        .ok_or_else(|| RepositoryError::ContractViolation("order lifecycle not found".into()))?;
    if current.version != expected_version {
        return Err(RepositoryError::ContractViolation(format!(
            "optimistic lifecycle version conflict: expected {expected_version}, actual {}",
            current.version
        )));
    }
    let plan = plan_action(transaction, tenant_id, &current, action)?;
    let now = utc_now();
    let resulting_version = expected_version + 1;
    let mut next = current.clone();
    for change in &plan.changes {
        set_dimension(&mut next, change.dimension, change.to_status)?;
    }
    next.version = resulting_version;
    next.updated_at = now.clone();
    let affected = transaction
        .execute(
            "UPDATE order_lifecycle
         SET commercial_status=?1,contract_status=?2,financial_status=?3,
             fulfilment_status=?4,risk_status=?5,version=?6,updated_at=?7
         WHERE tenant_id=?8 AND order_id=?9 AND version=?10",
            params![
                next.commercial_status,
                next.contract_status,
                next.financial_status,
                next.fulfilment_status,
                next.risk_status,
                resulting_version,
                now,
                tenant_id,
                order_id,
                expected_version
            ],
        )
        .map_err(sqlite_error)?;
    if affected != 1 {
        return Err(RepositoryError::ContractViolation(
            "optimistic lifecycle update lost its expected version".into(),
        ));
    }
    for change in &plan.changes {
        transaction
            .execute(
                "INSERT INTO order_lifecycle_history
             (id,tenant_id,order_id,dimension,from_status,to_status,guard_name,
              actor_user_id,reason,expected_version,resulting_version,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    Uuid::new_v4().to_string(),
                    tenant_id,
                    order_id,
                    change.dimension,
                    change.from_status,
                    change.to_status,
                    plan.guard_name,
                    actor_user_id,
                    reason,
                    expected_version,
                    resulting_version,
                    now
                ],
            )
            .map_err(sqlite_error)?;
    }
    if let Some(legacy_status) = plan.legacy_status {
        transaction
            .execute(
                "UPDATE orders SET status=?1 WHERE tenant_id=?2 AND id=?3",
                params![legacy_status, tenant_id, order_id],
            )
            .map_err(sqlite_error)?;
    }
    if action == "confirm_order" {
        append_outbox_tx(
            transaction,
            tenant_id,
            "order_lifecycle",
            order_id,
            "OrderConfirmed",
            &format!("order:{order_id}:confirmed:v{resulting_version}"),
            &serde_json::json!({"orderId": order_id, "lifecycleVersion": resulting_version}),
            &now,
        )?;
    }
    Ok(next)
}

const NAMED_ACTIONS: &[(&str, &str, &str, &str)] = &[
    ("submit_order", "commercial", "submitted", "提交订单"),
    ("confirm_order", "commercial", "confirmed", "确认订单"),
    ("cancel_order", "commercial", "cancelled", "取消订单"),
    ("require_contract", "contract", "draft", "要求合同"),
    ("generate_contract", "contract", "generated", "生成合同"),
    ("send_contract", "contract", "sent", "发送合同"),
    ("sign_contract", "contract", "signed", "签署合同"),
    ("verify_contract", "contract", "verified", "核验合同"),
    (
        "mark_awaiting_payment",
        "financial",
        "awaiting_payment",
        "等待付款",
    ),
    (
        "record_partial_payment",
        "financial",
        "partially_paid",
        "记录部分付款",
    ),
    ("record_paid", "financial", "paid", "确认已付款"),
    ("mark_reserved", "fulfilment", "reserved", "确认预留"),
    ("mark_allocated", "fulfilment", "allocated", "确认分配"),
    (
        "mark_ready_to_ship",
        "fulfilment",
        "ready_to_ship",
        "准备发货",
    ),
    ("mark_shipped", "fulfilment", "shipped", "确认发货"),
    ("mark_in_use", "fulfilment", "in_use", "进入使用"),
    (
        "mark_return_pending",
        "fulfilment",
        "return_pending",
        "等待归还",
    ),
    ("mark_returned", "fulfilment", "returned", "确认归还"),
    ("mark_inspected", "fulfilment", "inspected", "完成检查"),
    (
        "begin_settlement",
        "financial",
        "settlement_pending",
        "开始结算",
    ),
    ("settle_order", "financial", "settled", "完成结算"),
    ("mark_completed", "commercial", "completed", "业务完成"),
    ("close_order", "commercial", "closed", "关闭订单"),
    ("require_review", "risk", "review_required", "需要复核"),
    ("mark_overdue", "risk", "overdue", "标记逾期"),
    ("mark_damage_review", "risk", "damage_review", "损坏复核"),
    ("mark_repairing", "risk", "repairing", "进入维修"),
    ("mark_disputed", "risk", "disputed", "标记争议"),
    ("resolve_risk", "risk", "resolved", "解除风险"),
];

fn allowed_actions(
    connection: &Connection,
    tenant_id: &str,
    lifecycle: &OrderLifecycleProjection,
) -> rusqlite::Result<Vec<LifecycleAllowedAction>> {
    let mut actions = Vec::new();
    for (action, dimension, target_status, label) in NAMED_ACTIONS {
        if plan_action(connection, tenant_id, lifecycle, action).is_ok() {
            actions.push(LifecycleAllowedAction {
                action: (*action).into(),
                dimension: (*dimension).into(),
                target_status: (*target_status).into(),
                label: (*label).into(),
                expected_version: lifecycle.version,
            });
        }
    }
    Ok(actions)
}

fn plan_action(
    connection: &Connection,
    tenant_id: &str,
    lifecycle: &OrderLifecycleProjection,
    action: &str,
) -> Result<ActionPlan, RepositoryError> {
    let one = |dimension: &'static str,
               from_status: &str,
               to_status: &'static str,
               guard_name: &'static str,
               legacy_status: Option<&'static str>| {
        Ok(ActionPlan {
            guard_name,
            changes: vec![ActionChange {
                dimension,
                from_status: from_status.to_owned(),
                to_status,
            }],
            legacy_status,
        })
    };
    let require = |condition: bool, message: &str| -> Result<(), RepositoryError> {
        if condition {
            Ok(())
        } else {
            Err(RepositoryError::ContractViolation(message.into()))
        }
    };

    match action {
        "submit_order" => {
            require(
                lifecycle.commercial_status == "draft",
                "can_submit_order: commercial must be draft",
            )?;
            one(
                "commercial",
                &lifecycle.commercial_status,
                "submitted",
                "can_submit_order",
                None,
            )
        }
        "confirm_order" => {
            require(
                matches!(lifecycle.commercial_status.as_str(), "draft" | "submitted"),
                "can_confirm_order: commercial must be draft/submitted",
            )?;
            one(
                "commercial",
                &lifecycle.commercial_status,
                "confirmed",
                "can_confirm_order",
                Some("confirmed"),
            )
        }
        "cancel_order" => {
            require(
                matches!(
                    lifecycle.commercial_status.as_str(),
                    "draft" | "submitted" | "confirmed"
                ),
                "can_cancel_order: commercial state is terminal or completed",
            )?;
            require(
                matches!(
                    lifecycle.fulfilment_status.as_str(),
                    "unplanned" | "reserved" | "allocated" | "ready_to_ship"
                ),
                "can_cancel_order: fulfilment already shipped or later",
            )?;
            one(
                "commercial",
                &lifecycle.commercial_status,
                "cancelled",
                "can_cancel_order",
                Some("cancelled"),
            )
        }
        "require_contract" => {
            require(
                lifecycle.contract_status == "not_required",
                "can_require_contract: contract must be not_required",
            )?;
            one(
                "contract",
                &lifecycle.contract_status,
                "draft",
                "can_require_contract",
                None,
            )
        }
        "generate_contract" => {
            require(
                lifecycle.contract_status == "draft",
                "can_generate_contract: contract must be draft",
            )?;
            one(
                "contract",
                &lifecycle.contract_status,
                "generated",
                "can_generate_contract",
                None,
            )
        }
        "send_contract" => {
            require(
                lifecycle.contract_status == "generated",
                "can_send_contract: contract must be generated",
            )?;
            one(
                "contract",
                &lifecycle.contract_status,
                "sent",
                "can_send_contract",
                None,
            )
        }
        "sign_contract" => {
            require(
                lifecycle.contract_status == "sent",
                "can_sign_contract: contract must be sent",
            )?;
            one(
                "contract",
                &lifecycle.contract_status,
                "signed",
                "can_sign_contract",
                None,
            )
        }
        "verify_contract" => {
            require(
                lifecycle.contract_status == "signed",
                "can_verify_contract: contract must be signed",
            )?;
            one(
                "contract",
                &lifecycle.contract_status,
                "verified",
                "can_verify_contract",
                None,
            )
        }
        "mark_awaiting_payment" => {
            require(
                lifecycle.financial_status == "unbilled",
                "can_mark_awaiting_payment: financial must be unbilled",
            )?;
            one(
                "financial",
                &lifecycle.financial_status,
                "awaiting_payment",
                "can_mark_awaiting_payment",
                None,
            )
        }
        "record_partial_payment" => {
            require(
                lifecycle.financial_status == "awaiting_payment",
                "can_record_partial_payment: financial must await payment",
            )?;
            one(
                "financial",
                &lifecycle.financial_status,
                "partially_paid",
                "can_record_partial_payment",
                None,
            )
        }
        "record_paid" => {
            require(
                matches!(
                    lifecycle.financial_status.as_str(),
                    "awaiting_payment" | "partially_paid"
                ),
                "can_record_paid: financial must await payment",
            )?;
            one(
                "financial",
                &lifecycle.financial_status,
                "paid",
                "can_record_paid",
                Some("paid"),
            )
        }
        "mark_reserved" => {
            require(
                lifecycle.commercial_status == "confirmed",
                "can_mark_reserved: commercial must be confirmed",
            )?;
            require(
                lifecycle.fulfilment_status == "unplanned",
                "can_mark_reserved: fulfilment must be unplanned",
            )?;
            require(
                confirmed_reservation_exists(connection, tenant_id, &lifecycle.order_id)
                    .map_err(sqlite_error)?,
                "can_mark_reserved: no confirmed reservation",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "reserved",
                "can_mark_reserved",
                None,
            )
        }
        "mark_allocated" => {
            require(
                matches!(
                    lifecycle.fulfilment_status.as_str(),
                    "unplanned" | "reserved"
                ),
                "can_mark_allocated: fulfilment must be unplanned/reserved",
            )?;
            require(
                confirmed_reservation_exists(connection, tenant_id, &lifecycle.order_id)
                    .map_err(sqlite_error)?,
                "can_mark_allocated: no confirmed reservation",
            )?;
            require(
                allocations_complete(connection, tenant_id, &lifecycle.order_id)
                    .map_err(sqlite_error)?,
                "can_mark_allocated: reservation allocation incomplete",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "allocated",
                "can_mark_allocated",
                None,
            )
        }
        "mark_ready_to_ship" => {
            let blockers =
                readiness_blockers(connection, tenant_id, lifecycle).map_err(sqlite_error)?;
            require(
                blockers.is_empty(),
                &format!("can_mark_ready_to_ship blocked: {}", blockers.join(",")),
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "ready_to_ship",
                "can_mark_ready_to_ship",
                None,
            )
        }
        "mark_shipped" => {
            require(
                lifecycle.fulfilment_status == "ready_to_ship",
                "can_mark_shipped: fulfilment must be ready_to_ship",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "shipped",
                "can_mark_shipped",
                Some("shipped"),
            )
        }
        "mark_in_use" => {
            require(
                lifecycle.fulfilment_status == "shipped",
                "can_mark_in_use: fulfilment must be shipped",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "in_use",
                "can_mark_in_use",
                Some("in_use"),
            )
        }
        "mark_return_pending" => {
            require(
                lifecycle.fulfilment_status == "in_use",
                "can_mark_return_pending: fulfilment must be in_use",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "return_pending",
                "can_mark_return_pending",
                None,
            )
        }
        "mark_returned" => {
            require(
                lifecycle.fulfilment_status == "return_pending",
                "can_mark_returned: fulfilment must be return_pending",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "returned",
                "can_mark_returned",
                Some("returned"),
            )
        }
        "mark_inspected" => {
            require(
                lifecycle.fulfilment_status == "returned",
                "can_mark_inspected: fulfilment must be returned",
            )?;
            one(
                "fulfilment",
                &lifecycle.fulfilment_status,
                "inspected",
                "can_mark_inspected",
                Some("inspected"),
            )
        }
        "begin_settlement" => {
            require(
                lifecycle.financial_status == "paid",
                "can_begin_settlement: financial must be paid",
            )?;
            require(
                lifecycle.fulfilment_status == "inspected",
                "can_begin_settlement: fulfilment must be inspected",
            )?;
            one(
                "financial",
                &lifecycle.financial_status,
                "settlement_pending",
                "can_begin_settlement",
                None,
            )
        }
        "settle_order" => {
            require(
                lifecycle.financial_status == "settlement_pending",
                "can_settle_order: settlement must be pending",
            )?;
            one(
                "financial",
                &lifecycle.financial_status,
                "settled",
                "can_settle_order",
                None,
            )
        }
        "mark_completed" => {
            require(
                lifecycle.commercial_status == "confirmed",
                "can_mark_completed: commercial must be confirmed",
            )?;
            require(
                lifecycle.fulfilment_status == "inspected",
                "can_mark_completed: fulfilment must be inspected",
            )?;
            require(
                matches!(
                    lifecycle.financial_status.as_str(),
                    "paid" | "settlement_pending" | "settled"
                ),
                "can_mark_completed: financial obligation is not satisfied",
            )?;
            require(
                risk_is_clear(lifecycle),
                "can_mark_completed: blocking risk remains",
            )?;
            one(
                "commercial",
                &lifecycle.commercial_status,
                "completed",
                "can_mark_completed",
                Some("completed"),
            )
        }
        "close_order" => {
            require(
                lifecycle.commercial_status == "completed",
                "can_close_order: commercial must be completed",
            )?;
            require(
                lifecycle.fulfilment_status == "inspected",
                "can_close_order: fulfilment must be inspected",
            )?;
            require(
                lifecycle.financial_status == "settled",
                "can_close_order: financial must be settled",
            )?;
            require(
                contract_is_acceptable(lifecycle),
                "can_close_order: contract is not acceptable",
            )?;
            require(
                risk_is_clear(lifecycle),
                "can_close_order: blocking risk remains",
            )?;
            one(
                "commercial",
                &lifecycle.commercial_status,
                "closed",
                "can_close_order",
                Some("closed"),
            )
        }
        "require_review" | "mark_overdue" | "mark_damage_review" | "mark_repairing"
        | "mark_disputed" => {
            require(
                matches!(lifecycle.risk_status.as_str(), "clear" | "resolved"),
                "can_raise_risk: another blocking risk is active",
            )?;
            let target = match action {
                "require_review" => "review_required",
                "mark_overdue" => "overdue",
                "mark_damage_review" => "damage_review",
                "mark_repairing" => "repairing",
                _ => "disputed",
            };
            let legacy = if action == "mark_repairing" {
                Some("repairing")
            } else {
                None
            };
            one(
                "risk",
                &lifecycle.risk_status,
                target,
                "can_raise_risk",
                legacy,
            )
        }
        "resolve_risk" => {
            require(
                !risk_is_clear(lifecycle),
                "can_resolve_risk: no blocking risk exists",
            )?;
            one(
                "risk",
                &lifecycle.risk_status,
                "resolved",
                "can_resolve_risk",
                None,
            )
        }
        _ => Err(RepositoryError::ContractViolation(format!(
            "unknown named lifecycle action: {action}"
        ))),
    }
}

fn readiness_blockers(
    connection: &Connection,
    tenant_id: &str,
    lifecycle: &OrderLifecycleProjection,
) -> rusqlite::Result<Vec<String>> {
    let mut blockers = Vec::new();
    if lifecycle.commercial_status != "confirmed" {
        blockers.push("commercial_not_confirmed".into());
    }
    if !contract_is_acceptable(lifecycle) {
        blockers.push("contract_not_acceptable".into());
    }
    if !matches!(
        lifecycle.financial_status.as_str(),
        "paid" | "settlement_pending" | "settled"
    ) {
        blockers.push("financial_obligation_unsatisfied".into());
    }
    if !confirmed_reservation_exists(connection, tenant_id, &lifecycle.order_id)? {
        blockers.push("reservation_not_confirmed".into());
    }
    if !allocations_complete(connection, tenant_id, &lifecycle.order_id)? {
        blockers.push("allocation_incomplete".into());
    }
    if lifecycle.fulfilment_status != "allocated" {
        blockers.push("fulfilment_not_allocated".into());
    }
    if !risk_is_clear(lifecycle) {
        blockers.push("blocking_risk".into());
    }
    Ok(blockers)
}

fn confirmed_reservation_exists(
    connection: &Connection,
    tenant_id: &str,
    order_id: &str,
) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM rental_reservations
           WHERE tenant_id = ?1 AND order_id = ?2 AND status = 'confirmed'
         )",
        params![tenant_id, order_id],
        |row| row.get(0),
    )
}

fn allocations_complete(
    connection: &Connection,
    tenant_id: &str,
    order_id: &str,
) -> rusqlite::Result<bool> {
    let reservation_id: Option<String> = connection
        .query_row(
            "SELECT id FROM rental_reservations
             WHERE tenant_id = ?1 AND order_id = ?2 AND status = 'confirmed'
             ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![tenant_id, order_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(reservation_id) = reservation_id else {
        return Ok(false);
    };
    let requirement_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM reservation_requirements
         WHERE tenant_id = ?1 AND reservation_id = ?2",
        params![tenant_id, reservation_id],
        |row| row.get(0),
    )?;
    if requirement_count == 0 {
        return Ok(false);
    }
    let missing: i64 = connection.query_row(
        "SELECT COUNT(*) FROM reservation_requirements rr
         WHERE rr.tenant_id = ?1 AND rr.reservation_id = ?2
           AND rr.quantity > (
             SELECT COUNT(*) FROM allocations a
             WHERE a.tenant_id = rr.tenant_id
               AND a.reservation_id = rr.reservation_id
               AND a.model_id = rr.model_id
               AND a.status = 'allocated'
           )",
        params![tenant_id, reservation_id],
        |row| row.get(0),
    )?;
    Ok(missing == 0)
}

fn contract_is_acceptable(lifecycle: &OrderLifecycleProjection) -> bool {
    matches!(
        lifecycle.contract_status.as_str(),
        "not_required" | "verified"
    )
}

fn risk_is_clear(lifecycle: &OrderLifecycleProjection) -> bool {
    matches!(lifecycle.risk_status.as_str(), "clear" | "resolved")
}

fn set_dimension(
    lifecycle: &mut OrderLifecycleProjection,
    dimension: &str,
    target: &str,
) -> Result<(), RepositoryError> {
    match dimension {
        "commercial" => lifecycle.commercial_status = target.into(),
        "contract" => lifecycle.contract_status = target.into(),
        "financial" => lifecycle.financial_status = target.into(),
        "fulfilment" => lifecycle.fulfilment_status = target.into(),
        "risk" => lifecycle.risk_status = target.into(),
        _ => {
            return Err(RepositoryError::ContractViolation(format!(
                "unknown lifecycle dimension: {dimension}"
            )));
        }
    }
    Ok(())
}

fn load_lifecycle(
    connection: &Connection,
    tenant_id: &str,
    order_id: &str,
) -> rusqlite::Result<Option<OrderLifecycleProjection>> {
    connection
        .query_row(
            "SELECT order_id, commercial_status, contract_status, financial_status,
                    fulfilment_status, risk_status, version, updated_at
             FROM order_lifecycle
             WHERE tenant_id = ?1 AND order_id = ?2",
            params![tenant_id, order_id],
            |row| {
                Ok(OrderLifecycleProjection {
                    order_id: row.get(0)?,
                    commercial_status: row.get(1)?,
                    contract_status: row.get(2)?,
                    financial_status: row.get(3)?,
                    fulfilment_status: row.get(4)?,
                    risk_status: row.get(5)?,
                    version: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()
}

fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
