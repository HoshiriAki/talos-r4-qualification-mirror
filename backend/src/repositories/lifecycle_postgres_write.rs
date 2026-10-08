#![cfg(feature = "postgres")]

use chrono::{SecondsFormat, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::repositories::RepositoryError;
use crate::repositories::lifecycle::OrderLifecycleProjection;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresLifecycleWriteRepository<'a> {
    session: &'a RepositorySession,
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

#[derive(Debug, Clone, Copy)]
struct LifecycleGuardFacts {
    confirmed_reservation: bool,
    allocations_complete: bool,
}

impl<'a> PostgresLifecycleWriteRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn apply_action(
        &self,
        order_id: &str,
        action: &str,
        expected_version: i64,
        actor_user_id: &str,
        reason: &str,
    ) -> Result<OrderLifecycleProjection, RepositoryError> {
        let order_id = order_id.trim().to_owned();
        let action = action.trim().to_owned();
        if order_id.is_empty() || action.is_empty() || expected_version < 1 {
            return Err(RepositoryError::ContractViolation(
                "orderId, named action and positive expectedVersion are required".into(),
            ));
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let actor_user_id = actor_user_id.trim().to_owned();
        let reason = reason.trim().to_owned();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    apply_action_in_pg_transaction(
                        connection,
                        &tenant_id,
                        &order_id,
                        &action,
                        expected_version,
                        &actor_user_id,
                        &reason,
                    )
                    .await
                })
            })
    }
}

pub(in crate::repositories) async fn apply_action_in_pg_transaction(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
    action: &str,
    expected_version: i64,
    actor_user_id: &str,
    reason: &str,
) -> Result<OrderLifecycleProjection, RepositoryError> {
    let Some(current) = load_lifecycle_for_update_pg(connection, tenant_id, order_id)
        .await
        .map_err(pg_error)?
    else {
        return Err(RepositoryError::ContractViolation(
            "order lifecycle not found".into(),
        ));
    };

    if current.version != expected_version {
        return Err(RepositoryError::ContractViolation(format!(
            "optimistic lifecycle version conflict: expected {expected_version}, actual {}",
            current.version
        )));
    }

    let facts = load_guard_facts_pg(connection, tenant_id, order_id)
        .await
        .map_err(pg_error)?;
    let plan = plan_action_pg(&current, facts, action)?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let resulting_version = expected_version + 1;
    let mut next = current.clone();
    for change in &plan.changes {
        set_dimension(&mut next, change.dimension, change.to_status)?;
    }
    next.version = resulting_version;
    next.updated_at = now.clone();

    let affected = sqlx::query(
        "UPDATE order_lifecycle \
         SET commercial_status = $1, contract_status = $2, financial_status = $3, \
             fulfilment_status = $4, risk_status = $5, version = $6, updated_at = $7 \
         WHERE tenant_id = $8 AND order_id = $9 AND version = $10",
    )
    .bind(&next.commercial_status)
    .bind(&next.contract_status)
    .bind(&next.financial_status)
    .bind(&next.fulfilment_status)
    .bind(&next.risk_status)
    .bind(resulting_version)
    .bind(&now)
    .bind(tenant_id)
    .bind(order_id)
    .bind(expected_version)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?
    .rows_affected();

    if affected != 1 {
        return Err(RepositoryError::ContractViolation(
            "optimistic lifecycle update lost its expected version".into(),
        ));
    }

    for change in &plan.changes {
        sqlx::query(
            "INSERT INTO order_lifecycle_history \
             (id, tenant_id, order_id, dimension, from_status, to_status, guard_name, \
              actor_user_id, reason, expected_version, resulting_version, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(tenant_id)
        .bind(order_id)
        .bind(change.dimension)
        .bind(&change.from_status)
        .bind(change.to_status)
        .bind(plan.guard_name)
        .bind(actor_user_id)
        .bind(reason)
        .bind(expected_version)
        .bind(resulting_version)
        .bind(&now)
        .execute(&mut *connection)
        .await
        .map_err(pg_error)?;
    }

    if let Some(legacy_status) = plan.legacy_status {
        sqlx::query("UPDATE orders SET status = $1 WHERE tenant_id = $2 AND id = $3")
            .bind(legacy_status)
            .bind(tenant_id)
            .bind(order_id)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;
    }

    if action == "confirm_order" {
        let idempotency_key = format!("order:{order_id}:confirmed:v{resulting_version}");
        let payload = serde_json::json!({
            "orderId": order_id,
            "lifecycleVersion": resulting_version
        })
        .to_string();
        sqlx::query(
            "INSERT INTO domain_outbox \
             (id, tenant_id, source_kind, source_id, message_type, idempotency_key, \
              payload_json, payload_version, state, available_at, created_at) \
             VALUES ($1,$2,'order_lifecycle',$3,'OrderConfirmed',$4,$5,1,'pending',$6,$6) \
             ON CONFLICT (tenant_id, idempotency_key) DO NOTHING",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(tenant_id)
        .bind(order_id)
        .bind(idempotency_key)
        .bind(payload)
        .bind(&now)
        .execute(&mut *connection)
        .await
        .map_err(pg_error)?;
    }

    Ok(next)
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

async fn load_lifecycle_for_update_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
) -> Result<Option<OrderLifecycleProjection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT order_id, commercial_status, contract_status, financial_status, \
                fulfilment_status, risk_status, version, updated_at \
         FROM order_lifecycle \
         WHERE tenant_id = $1 AND order_id = $2 \
         FOR UPDATE",
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

fn plan_action_pg(
    lifecycle: &OrderLifecycleProjection,
    facts: LifecycleGuardFacts,
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
        "require_contract" => transition(
            &lifecycle.contract_status,
            "not_required",
            "contract",
            "draft",
            "can_require_contract",
            "can_require_contract: contract must be not_required",
        ),
        "generate_contract" => transition(
            &lifecycle.contract_status,
            "draft",
            "contract",
            "generated",
            "can_generate_contract",
            "can_generate_contract: contract must be draft",
        ),
        "send_contract" => transition(
            &lifecycle.contract_status,
            "generated",
            "contract",
            "sent",
            "can_send_contract",
            "can_send_contract: contract must be generated",
        ),
        "sign_contract" => transition(
            &lifecycle.contract_status,
            "sent",
            "contract",
            "signed",
            "can_sign_contract",
            "can_sign_contract: contract must be sent",
        ),
        "verify_contract" => transition(
            &lifecycle.contract_status,
            "signed",
            "contract",
            "verified",
            "can_verify_contract",
            "can_verify_contract: contract must be signed",
        ),
        "mark_awaiting_payment" => transition(
            &lifecycle.financial_status,
            "unbilled",
            "financial",
            "awaiting_payment",
            "can_mark_awaiting_payment",
            "can_mark_awaiting_payment: financial must be unbilled",
        ),
        "record_partial_payment" => transition(
            &lifecycle.financial_status,
            "awaiting_payment",
            "financial",
            "partially_paid",
            "can_record_partial_payment",
            "can_record_partial_payment: financial must await payment",
        ),
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
                facts.confirmed_reservation,
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
                facts.confirmed_reservation,
                "can_mark_allocated: no confirmed reservation",
            )?;
            require(
                facts.allocations_complete,
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
            let blockers = readiness_blockers_pg(lifecycle, facts);
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
        "mark_shipped" => transition_with_legacy(
            &lifecycle.fulfilment_status,
            "ready_to_ship",
            "fulfilment",
            "shipped",
            "can_mark_shipped",
            "can_mark_shipped: fulfilment must be ready_to_ship",
            "shipped",
        ),
        "mark_in_use" => transition_with_legacy(
            &lifecycle.fulfilment_status,
            "shipped",
            "fulfilment",
            "in_use",
            "can_mark_in_use",
            "can_mark_in_use: fulfilment must be shipped",
            "in_use",
        ),
        "mark_return_pending" => transition(
            &lifecycle.fulfilment_status,
            "in_use",
            "fulfilment",
            "return_pending",
            "can_mark_return_pending",
            "can_mark_return_pending: fulfilment must be in_use",
        ),
        "mark_returned" => transition_with_legacy(
            &lifecycle.fulfilment_status,
            "return_pending",
            "fulfilment",
            "returned",
            "can_mark_returned",
            "can_mark_returned: fulfilment must be return_pending",
            "returned",
        ),
        "mark_inspected" => transition_with_legacy(
            &lifecycle.fulfilment_status,
            "returned",
            "fulfilment",
            "inspected",
            "can_mark_inspected",
            "can_mark_inspected: fulfilment must be returned",
            "inspected",
        ),
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
        "settle_order" => transition(
            &lifecycle.financial_status,
            "settlement_pending",
            "financial",
            "settled",
            "can_settle_order",
            "can_settle_order: settlement must be pending",
        ),
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
            let legacy = (action == "mark_repairing").then_some("repairing");
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

fn transition(
    current: &str,
    expected: &str,
    dimension: &'static str,
    target: &'static str,
    guard_name: &'static str,
    error: &str,
) -> Result<ActionPlan, RepositoryError> {
    if current != expected {
        return Err(RepositoryError::ContractViolation(error.into()));
    }
    Ok(ActionPlan {
        guard_name,
        changes: vec![ActionChange {
            dimension,
            from_status: current.to_owned(),
            to_status: target,
        }],
        legacy_status: None,
    })
}

fn transition_with_legacy(
    current: &str,
    expected: &str,
    dimension: &'static str,
    target: &'static str,
    guard_name: &'static str,
    error: &str,
    legacy_status: &'static str,
) -> Result<ActionPlan, RepositoryError> {
    let mut plan = transition(current, expected, dimension, target, guard_name, error)?;
    plan.legacy_status = Some(legacy_status);
    Ok(plan)
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
