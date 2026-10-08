#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::lifecycle::{
    LifecycleAllowedAction, LifecycleHistoryProjection, LifecycleMigrationExceptionProjection,
    LifecycleOperationalView, OrderLifecycleProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresOrderLifecycleRepository<'a> {
    session: &'a RepositorySession,
}

#[derive(Debug, Clone, Copy)]
struct LifecycleGuardFacts {
    confirmed_reservation: bool,
    allocations_complete: bool,
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

impl<'a> PostgresOrderLifecycleRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn get(
        &self,
        order_id: &str,
    ) -> Result<Option<OrderLifecycleProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move { load_lifecycle_pg(connection, &tenant_id, &order_id).await })
        })
    }

    pub(in crate::repositories) fn operational_view(
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

        let view = self.session.pg_read(move |connection| {
            Box::pin(async move {
                let Some(lifecycle) = load_lifecycle_pg(connection, &tenant_id, &order_id).await?
                else {
                    return Ok(None);
                };
                let facts = load_guard_facts_pg(connection, &tenant_id, &order_id).await?;
                let allowed_actions = allowed_actions_pg(&lifecycle, facts);
                let blockers = readiness_blockers_pg(&lifecycle, facts);
                Ok(Some(LifecycleOperationalView {
                    lifecycle,
                    allowed_actions,
                    blockers,
                }))
            })
        })?;

        view.ok_or_else(|| RepositoryError::ContractViolation("order lifecycle not found".into()))
    }

    pub(in crate::repositories) fn history(
        &self,
        order_id: &str,
    ) -> Result<Vec<LifecycleHistoryProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT id, order_id, dimension, from_status, to_status, guard_name, \
                     actor_user_id, reason, expected_version, resulting_version, created_at \
                     FROM order_lifecycle_history \
                     WHERE tenant_id = $1 AND order_id = $2 \
                     ORDER BY resulting_version, created_at, id",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .fetch_all(connection)
                .await?;

                rows.into_iter()
                    .map(|row| {
                        Ok(LifecycleHistoryProjection {
                            id: row.try_get("id")?,
                            order_id: row.try_get("order_id")?,
                            dimension: row.try_get("dimension")?,
                            from_status: row.try_get("from_status")?,
                            to_status: row.try_get("to_status")?,
                            guard_name: row.try_get("guard_name")?,
                            actor_user_id: row.try_get("actor_user_id")?,
                            reason: row.try_get("reason")?,
                            expected_version: row.try_get("expected_version")?,
                            resulting_version: row.try_get("resulting_version")?,
                            created_at: row.try_get("created_at")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn list_migration_exceptions(
        &self,
    ) -> Result<Vec<LifecycleMigrationExceptionProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT id, order_id, legacy_status, reason, status, created_at \
                     FROM lifecycle_migration_exceptions \
                     WHERE tenant_id = $1 ORDER BY created_at, id",
                )
                .bind(&tenant_id)
                .fetch_all(connection)
                .await?;

                rows.into_iter()
                    .map(|row| {
                        Ok(LifecycleMigrationExceptionProjection {
                            id: row.try_get("id")?,
                            order_id: row.try_get("order_id")?,
                            legacy_status: row.try_get("legacy_status")?,
                            reason: row.try_get("reason")?,
                            status: row.try_get("status")?,
                            created_at: row.try_get("created_at")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn apply_action(
        &self,
        _order_id: &str,
        _action: &str,
        _expected_version: i64,
        _actor_user_id: &str,
        _reason: &str,
    ) -> Result<LifecycleOperationalView, RepositoryError> {
        Err(RepositoryError::AdapterUnavailable(
            "PostgreSQL lifecycle write adapter is not migrated yet; P8-C keeps this path fail-closed"
                .into(),
        ))
    }
}

async fn load_lifecycle_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
) -> Result<Option<OrderLifecycleProjection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT order_id, commercial_status, contract_status, financial_status, \
         fulfilment_status, risk_status, version, updated_at \
         FROM order_lifecycle WHERE tenant_id = $1 AND order_id = $2",
    )
    .bind(tenant_id)
    .bind(order_id)
    .fetch_optional(connection)
    .await?;

    row.map(|row| {
        Ok(OrderLifecycleProjection {
            order_id: row.try_get("order_id")?,
            commercial_status: row.try_get("commercial_status")?,
            contract_status: row.try_get("contract_status")?,
            financial_status: row.try_get("financial_status")?,
            fulfilment_status: row.try_get("fulfilment_status")?,
            risk_status: row.try_get("risk_status")?,
            version: row.try_get("version")?,
            updated_at: row.try_get("updated_at")?,
        })
    })
    .transpose()
}

async fn load_guard_facts_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
) -> Result<LifecycleGuardFacts, sqlx::Error> {
    let confirmed_reservation = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM rental_reservations \
         WHERE tenant_id = $1 AND order_id = $2 AND status = 'confirmed')",
    )
    .bind(tenant_id)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let reservation_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM rental_reservations \
         WHERE tenant_id = $1 AND order_id = $2 AND status = 'confirmed' \
         ORDER BY updated_at DESC, id DESC LIMIT 1",
    )
    .bind(tenant_id)
    .bind(order_id)
    .fetch_optional(&mut *connection)
    .await?;

    let allocations_complete = if let Some(reservation_id) = reservation_id {
        let requirement_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM reservation_requirements \
             WHERE tenant_id = $1 AND reservation_id = $2",
        )
        .bind(tenant_id)
        .bind(&reservation_id)
        .fetch_one(&mut *connection)
        .await?;

        if requirement_count == 0 {
            false
        } else {
            let missing = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*)::bigint FROM reservation_requirements rr \
                 WHERE rr.tenant_id = $1 AND rr.reservation_id = $2 \
                   AND rr.quantity > ( \
                     SELECT COUNT(*) FROM allocations a \
                     WHERE a.tenant_id = rr.tenant_id \
                       AND a.reservation_id = rr.reservation_id \
                       AND a.model_id = rr.model_id \
                       AND a.status = 'allocated' \
                   )",
            )
            .bind(tenant_id)
            .bind(&reservation_id)
            .fetch_one(&mut *connection)
            .await?;
            missing == 0
        }
    } else {
        false
    };

    Ok(LifecycleGuardFacts {
        confirmed_reservation,
        allocations_complete,
    })
}

fn allowed_actions_pg(
    lifecycle: &OrderLifecycleProjection,
    facts: LifecycleGuardFacts,
) -> Vec<LifecycleAllowedAction> {
    NAMED_ACTIONS
        .iter()
        .filter(|(action, _, _, _)| action_allowed_pg(lifecycle, facts, action))
        .map(
            |(action, dimension, target_status, label)| LifecycleAllowedAction {
                action: (*action).into(),
                dimension: (*dimension).into(),
                target_status: (*target_status).into(),
                label: (*label).into(),
                expected_version: lifecycle.version,
            },
        )
        .collect()
}

fn action_allowed_pg(
    lifecycle: &OrderLifecycleProjection,
    facts: LifecycleGuardFacts,
    action: &str,
) -> bool {
    match action {
        "submit_order" => lifecycle.commercial_status == "draft",
        "confirm_order" => matches!(lifecycle.commercial_status.as_str(), "draft" | "submitted"),
        "cancel_order" => {
            matches!(
                lifecycle.commercial_status.as_str(),
                "draft" | "submitted" | "confirmed"
            ) && matches!(
                lifecycle.fulfilment_status.as_str(),
                "unplanned" | "reserved" | "allocated" | "ready_to_ship"
            )
        }
        "require_contract" => lifecycle.contract_status == "not_required",
        "generate_contract" => lifecycle.contract_status == "draft",
        "send_contract" => lifecycle.contract_status == "generated",
        "sign_contract" => lifecycle.contract_status == "sent",
        "verify_contract" => lifecycle.contract_status == "signed",
        "mark_awaiting_payment" => lifecycle.financial_status == "unbilled",
        "record_partial_payment" => lifecycle.financial_status == "awaiting_payment",
        "record_paid" => matches!(
            lifecycle.financial_status.as_str(),
            "awaiting_payment" | "partially_paid"
        ),
        "mark_reserved" => {
            lifecycle.commercial_status == "confirmed"
                && lifecycle.fulfilment_status == "unplanned"
                && facts.confirmed_reservation
        }
        "mark_allocated" => {
            matches!(
                lifecycle.fulfilment_status.as_str(),
                "unplanned" | "reserved"
            ) && facts.confirmed_reservation
                && facts.allocations_complete
        }
        "mark_ready_to_ship" => readiness_blockers_pg(lifecycle, facts).is_empty(),
        "mark_shipped" => lifecycle.fulfilment_status == "ready_to_ship",
        "mark_in_use" => lifecycle.fulfilment_status == "shipped",
        "mark_return_pending" => lifecycle.fulfilment_status == "in_use",
        "mark_returned" => lifecycle.fulfilment_status == "return_pending",
        "mark_inspected" => lifecycle.fulfilment_status == "returned",
        "begin_settlement" => {
            lifecycle.financial_status == "paid" && lifecycle.fulfilment_status == "inspected"
        }
        "settle_order" => lifecycle.financial_status == "settlement_pending",
        "mark_completed" => {
            lifecycle.commercial_status == "confirmed"
                && lifecycle.fulfilment_status == "inspected"
                && matches!(
                    lifecycle.financial_status.as_str(),
                    "paid" | "settlement_pending" | "settled"
                )
                && risk_is_clear(lifecycle)
        }
        "close_order" => {
            lifecycle.commercial_status == "completed"
                && lifecycle.fulfilment_status == "inspected"
                && lifecycle.financial_status == "settled"
                && contract_is_acceptable(lifecycle)
                && risk_is_clear(lifecycle)
        }
        "require_review" | "mark_overdue" | "mark_damage_review" | "mark_repairing"
        | "mark_disputed" => {
            matches!(lifecycle.risk_status.as_str(), "clear" | "resolved")
        }
        "resolve_risk" => !risk_is_clear(lifecycle),
        _ => false,
    }
}

fn readiness_blockers_pg(
    lifecycle: &OrderLifecycleProjection,
    facts: LifecycleGuardFacts,
) -> Vec<String> {
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
    if !facts.confirmed_reservation {
        blockers.push("reservation_not_confirmed".into());
    }
    if !facts.allocations_complete {
        blockers.push("allocation_incomplete".into());
    }
    if lifecycle.fulfilment_status != "allocated" {
        blockers.push("fulfilment_not_allocated".into());
    }
    if !risk_is_clear(lifecycle) {
        blockers.push("blocking_risk".into());
    }
    blockers
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
