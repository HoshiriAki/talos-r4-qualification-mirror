use std::future::Future;
use std::sync::Arc;

use sqlx::postgres::PgPool;
use sqlx::{Row, Transaction};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::deposit::{
    DepositLedgerEntryKind, DepositReconciliationOutcome, DepositState, RefundState,
    require_positive_minor_units,
};
use super::deposit_refund_persistence_contract::DepositRefundPersistence;
use super::types::{DepositId, ExternalOperationId, IntegrationError, Money, RefundId};
use crate::observability::{FinancialEvent, MetricsSink, NoopMetrics};

#[derive(Clone)]
pub(crate) struct PostgresDepositRefundPersistence {
    pool: PgPool,
    metrics: Arc<dyn MetricsSink>,
}

impl PostgresDepositRefundPersistence {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self::new_with_metrics(pool, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(pool: PgPool, metrics: Arc<dyn MetricsSink>) -> Self {
        Self { pool, metrics }
    }
}

impl DepositRefundPersistence for PostgresDepositRefundPersistence {
    fn create_deposit(
        &self,
        tenant_id: &str,
        authority_kind: &str,
        authority_id: &str,
        expected: Money,
    ) -> Result<DepositId, IntegrationError> {
        if !matches!(authority_kind, "order" | "settlement")
            || authority_id.trim().is_empty()
            || expected.minor < 0
        {
            return Err(IntegrationError::InvalidOperationTransition);
        }

        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let authority_kind = authority_kind.to_owned();
        let authority_id = authority_id.to_owned();
        let expected_for_tx = expected.clone();

        let id = run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            let id = DepositId::new(uuid::Uuid::new_v4().to_string())?;
            let timestamp = now();

            sqlx::query(
                "INSERT INTO integration_deposits
                    (id,tenant_id,authority_kind,authority_id,expected_amount_minor,currency,
                     state,created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,'expected',$7,$7)",
            )
            .bind(id.as_str())
            .bind(&tenant_id)
            .bind(&authority_kind)
            .bind(&authority_id)
            .bind(expected_for_tx.minor)
            .bind(&expected_for_tx.currency)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            append_deposit_ledger_tx(
                &mut transaction,
                &tenant_id,
                &id,
                DepositLedgerEntryKind::Expected,
                &expected_for_tx,
                None,
                "deposit_created",
            )
            .await?;

            transaction.commit().await.map_err(persistence)?;
            Ok(id)
        })?;
        self.metrics.financial(FinancialEvent::DepositRecorded);
        Ok(id)
    }

    fn record_deposit_received(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let deposit_id = deposit_id.clone();
        let audit_ref = audit_ref.to_owned();

        run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            assert_deposit_currency(&mut transaction, &tenant_id, &deposit_id, &money).await?;
            append_deposit_ledger_tx(
                &mut transaction,
                &tenant_id,
                &deposit_id,
                DepositLedgerEntryKind::Received,
                &money,
                None,
                &audit_ref,
            )
            .await?;

            sqlx::query(
                "UPDATE integration_deposits
                 SET state=$1,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4 AND state IN ('expected','recorded')",
            )
            .bind(DepositState::Recorded.as_str())
            .bind(now())
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)
        })?;
        self.metrics
            .financial(FinancialEvent::DepositReceivedLedger);
        Ok(())
    }

    fn hold_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let deposit_id = deposit_id.clone();
        let audit_ref = audit_ref.to_owned();

        run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            assert_deposit_currency(&mut transaction, &tenant_id, &deposit_id, &money).await?;
            append_deposit_ledger_tx(
                &mut transaction,
                &tenant_id,
                &deposit_id,
                DepositLedgerEntryKind::Held,
                &money,
                None,
                &audit_ref,
            )
            .await?;

            let changed = sqlx::query(
                "UPDATE integration_deposits
                 SET state=$1,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4 AND state IN ('recorded','held')",
            )
            .bind(DepositState::Held.as_str())
            .bind(now())
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            transaction.commit().await.map_err(persistence)
        })
    }

    fn deduct_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let deposit_id = deposit_id.clone();
        let audit_ref = audit_ref.to_owned();

        run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            assert_deposit_currency(&mut transaction, &tenant_id, &deposit_id, &money).await?;
            if available_refundable_minor(&mut transaction, &tenant_id, &deposit_id).await?
                < money.minor
            {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::RefundAmountExceedsAvailable);
            }

            append_deposit_ledger_tx(
                &mut transaction,
                &tenant_id,
                &deposit_id,
                DepositLedgerEntryKind::Deducted,
                &money,
                None,
                &audit_ref,
            )
            .await?;

            let changed = sqlx::query(
                "UPDATE integration_deposits
                 SET state=$1,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4
                   AND state IN ('recorded','held','partially_deducted')",
            )
            .bind(DepositState::PartiallyDeducted.as_str())
            .bind(now())
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            transaction.commit().await.map_err(persistence)
        })?;
        self.metrics
            .financial(FinancialEvent::DepositDeductionAdmitted);
        Ok(())
    }

    fn release_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let deposit_id = deposit_id.clone();
        let audit_ref = audit_ref.to_owned();

        run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            assert_deposit_currency(&mut transaction, &tenant_id, &deposit_id, &money).await?;
            append_deposit_ledger_tx(
                &mut transaction,
                &tenant_id,
                &deposit_id,
                DepositLedgerEntryKind::Released,
                &money,
                None,
                &audit_ref,
            )
            .await?;

            let changed = sqlx::query(
                "UPDATE integration_deposits
                 SET state=$1,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4
                   AND state IN ('recorded','held','partially_deducted')",
            )
            .bind(DepositState::Released.as_str())
            .bind(now())
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            transaction.commit().await.map_err(persistence)
        })
    }

    fn request_refund(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        idempotency_key: &str,
        reason: &str,
    ) -> Result<RefundId, IntegrationError> {
        if idempotency_key.trim().is_empty() || reason.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        require_positive_minor_units(money.minor)?;

        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let deposit_id = deposit_id.clone();
        let idempotency_key = idempotency_key.to_owned();
        let reason = reason.to_owned();

        let (refund_id, admitted) = run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            assert_deposit_currency(&mut transaction, &tenant_id, &deposit_id, &money).await?;

            let existing = sqlx::query(
                "SELECT id,amount_minor,currency,reason
                 FROM refund_intents
                 WHERE tenant_id=$1 AND deposit_id=$2 AND idempotency_key=$3
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .bind(&idempotency_key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            if let Some(row) = existing {
                let existing_id: String = row.try_get(0).map_err(persistence)?;
                let amount_minor: i64 = row.try_get(1).map_err(persistence)?;
                let currency: String = row.try_get(2).map_err(persistence)?;
                let existing_reason: String = row.try_get(3).map_err(persistence)?;
                if amount_minor != money.minor
                    || currency != money.currency
                    || existing_reason != reason
                {
                    let _ = transaction.rollback().await;
                    return Err(IntegrationError::InvalidOperationTransition);
                }
                transaction.commit().await.map_err(persistence)?;
                return Ok((RefundId::new(existing_id)?, false));
            }

            let reserved: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(amount_minor),0)::BIGINT
                 FROM refund_intents
                 WHERE tenant_id=$1
                   AND deposit_id=$2
                   AND state IN ('requested','approved','dispatching','unknown_outcome')",
            )
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;

            let available =
                available_refundable_minor(&mut transaction, &tenant_id, &deposit_id).await?;
            if money.minor > available.saturating_sub(reserved) {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::RefundAmountExceedsAvailable);
            }

            let refund_id = RefundId::new(uuid::Uuid::new_v4().to_string())?;
            let timestamp = now();
            sqlx::query(
                "INSERT INTO refund_intents
                    (id,tenant_id,deposit_id,amount_minor,currency,idempotency_key,state,reason,
                     created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,'requested',$7,$8,$8)",
            )
            .bind(refund_id.as_str())
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .bind(money.minor)
            .bind(&money.currency)
            .bind(&idempotency_key)
            .bind(&reason)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)?;
            Ok((refund_id, true))
        })?;
        if admitted {
            self.metrics.financial(FinancialEvent::RefundIntentAdmitted);
        }
        Ok(refund_id)
    }

    fn link_refund_operation(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        operation_id: &ExternalOperationId,
    ) -> Result<(), IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let refund_id = refund_id.clone();
        let operation_id = operation_id.clone();

        let linked = run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            let exists = sqlx::query_scalar::<_, String>(
                "SELECT id
                 FROM external_operations
                 WHERE tenant_id=$1
                   AND id=$2
                   AND operation_type='refund'
                   AND state IN ('ready','retryable_failure','dispatching','unknown_outcome','reconciling')
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;
            if exists.is_none() {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::RefundOperationUnavailable);
            }

            let changed = sqlx::query(
                "UPDATE refund_intents
                 SET state=$1,external_operation_id=$2,updated_at=$3
                 WHERE tenant_id=$4 AND id=$5 AND state='requested'",
            )
            .bind(RefundState::Approved.as_str())
            .bind(operation_id.as_str())
            .bind(now())
            .bind(&tenant_id)
            .bind(refund_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();

            if changed != 1 {
                let row = sqlx::query(
                    "SELECT state,external_operation_id
                     FROM refund_intents
                     WHERE tenant_id=$1 AND id=$2
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(refund_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(persistence)?;

                let idempotent = row.is_some_and(|row| {
                    let state = row.try_get::<String, _>(0).ok();
                    let existing_id = row.try_get::<Option<String>, _>(1).ok().flatten();
                    state.as_deref() == Some(RefundState::Approved.as_str())
                        && existing_id.as_deref() == Some(operation_id.as_str())
                });
                if !idempotent {
                    let _ = transaction.rollback().await;
                    return Err(IntegrationError::InvalidOperationTransition);
                }
            }

            transaction.commit().await.map_err(persistence)?;
            Ok(changed == 1)
        })?;
        if linked {
            self.metrics
                .financial(FinancialEvent::RefundOperationPlanned);
        }
        Ok(())
    }

    fn reconcile_refund(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        outcome: DepositReconciliationOutcome,
        evidence_ref: &str,
        actor_ref: Option<&str>,
    ) -> Result<(), IntegrationError> {
        if evidence_ref.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }

        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let refund_id = refund_id.clone();
        let evidence_ref = evidence_ref.to_owned();
        let actor_ref = actor_ref.map(str::to_owned);

        run_pg_finance(async move {
            let mut transaction = serializable(&pool).await?;
            let row = sqlx::query(
                "SELECT deposit_id,amount_minor,currency,external_operation_id,state
                 FROM refund_intents
                 WHERE tenant_id=$1 AND id=$2
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .bind(refund_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;

            let deposit_id = DepositId::new(row.try_get::<String, _>(0).map_err(persistence)?)?;
            let amount = Money::new(
                row.try_get::<i64, _>(1).map_err(persistence)?,
                row.try_get::<String, _>(2).map_err(persistence)?,
            )?;
            let external_operation_id: Option<String> = row.try_get(3).map_err(persistence)?;
            let current_state: String = row.try_get(4).map_err(persistence)?;

            if matches!(
                current_state.as_str(),
                "completed" | "failed" | "manual_resolution_required"
            ) {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            assert_deposit_currency(&mut transaction, &tenant_id, &deposit_id, &amount).await?;

            if matches!(outcome, DepositReconciliationOutcome::Matched)
                && available_refundable_minor(&mut transaction, &tenant_id, &deposit_id).await?
                    < amount.minor
            {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::RefundAmountExceedsAvailable);
            }

            let next_state = match outcome {
                DepositReconciliationOutcome::Pending => RefundState::Dispatching,
                DepositReconciliationOutcome::Matched => RefundState::Completed,
                DepositReconciliationOutcome::Unknown => RefundState::UnknownOutcome,
                DepositReconciliationOutcome::ManualRequired => {
                    RefundState::ManualResolutionRequired
                }
            };
            let timestamp = now();

            sqlx::query(
                "UPDATE refund_intents
                 SET state=$1,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4",
            )
            .bind(next_state.as_str())
            .bind(&timestamp)
            .bind(&tenant_id)
            .bind(refund_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            let resolved_at = if matches!(
                outcome,
                DepositReconciliationOutcome::Pending | DepositReconciliationOutcome::Unknown
            ) {
                None
            } else {
                Some(timestamp.clone())
            };
            sqlx::query(
                "INSERT INTO integration_deposit_reconciliations
                    (id,tenant_id,deposit_id,refund_id,external_operation_id,outcome,evidence_ref,
                     resolved_by,created_at,resolved_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&tenant_id)
            .bind(deposit_id.as_str())
            .bind(refund_id.as_str())
            .bind(external_operation_id.as_deref())
            .bind(outcome.as_str())
            .bind(&evidence_ref)
            .bind(actor_ref.as_deref())
            .bind(&timestamp)
            .bind(resolved_at.as_deref())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            if matches!(outcome, DepositReconciliationOutcome::Matched) {
                let external_operation = external_operation_id
                    .map(ExternalOperationId::new)
                    .transpose()?;
                append_deposit_ledger_tx(
                    &mut transaction,
                    &tenant_id,
                    &deposit_id,
                    DepositLedgerEntryKind::RefundCompleted,
                    &amount,
                    external_operation.as_ref(),
                    "refund_reconciliation_matched",
                )
                .await?;
            }

            if matches!(outcome, DepositReconciliationOutcome::ManualRequired) {
                sqlx::query(
                    "UPDATE integration_deposits
                     SET state=$1,updated_at=$2
                     WHERE tenant_id=$3 AND id=$4",
                )
                .bind(DepositState::ManualResolutionRequired.as_str())
                .bind(&timestamp)
                .bind(&tenant_id)
                .bind(deposit_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;
            }

            transaction.commit().await.map_err(persistence)
        })?;
        self.metrics.financial(FinancialEvent::RefundReconciliation);
        Ok(())
    }
}

async fn assert_deposit_currency(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    deposit_id: &DepositId,
    money: &Money,
) -> Result<(), IntegrationError> {
    let currency = sqlx::query_scalar::<_, String>(
        "SELECT currency
         FROM integration_deposits
         WHERE tenant_id=$1 AND id=$2
         FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(deposit_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence)?;

    if currency != money.currency {
        return Err(IntegrationError::CurrencyMismatch);
    }
    Ok(())
}

async fn append_deposit_ledger_tx(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    deposit_id: &DepositId,
    kind: DepositLedgerEntryKind,
    money: &Money,
    external_operation_id: Option<&ExternalOperationId>,
    audit_ref: &str,
) -> Result<(), IntegrationError> {
    if audit_ref.trim().is_empty() {
        return Err(IntegrationError::InvalidOperationTransition);
    }

    sqlx::query(
        "INSERT INTO integration_deposit_ledger
            (id,tenant_id,deposit_id,entry_type,amount_minor,currency,external_operation_id,
             audit_ref,created_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(tenant_id)
    .bind(deposit_id.as_str())
    .bind(kind.as_str())
    .bind(money.minor)
    .bind(&money.currency)
    .bind(external_operation_id.map(ExternalOperationId::as_str))
    .bind(audit_ref)
    .bind(now())
    .execute(&mut **transaction)
    .await
    .map_err(persistence)?;
    Ok(())
}

async fn available_refundable_minor(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    deposit_id: &DepositId,
) -> Result<i64, IntegrationError> {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(
            CASE entry_type
                WHEN 'received' THEN amount_minor
                WHEN 'manual_adjustment' THEN amount_minor
                WHEN 'deducted' THEN -amount_minor
                WHEN 'refund_completed' THEN -amount_minor
                ELSE 0
            END
         ),0)::BIGINT
         FROM integration_deposit_ledger
         WHERE tenant_id=$1 AND deposit_id=$2",
    )
    .bind(tenant_id)
    .bind(deposit_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence)
}

async fn serializable(pool: &PgPool) -> Result<Transaction<'_, sqlx::Postgres>, IntegrationError> {
    let mut transaction = pool.begin().await.map_err(persistence)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(persistence)?;
    Ok(transaction)
}

fn run_pg_finance<T, F>(future: F) -> Result<T, IntegrationError>
where
    T: Send,
    F: Future<Output = Result<T, IntegrationError>> + Send,
{
    let handle = Handle::try_current().map_err(persistence)?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(IntegrationError::Persistence);
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}
