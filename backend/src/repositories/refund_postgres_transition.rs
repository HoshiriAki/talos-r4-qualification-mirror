#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::refund::{
    RefundMutationError, RefundStatusOutcome, contract, map_mutation_error,
};
use crate::repositories::refund_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn approve(
    session: &RepositorySession,
    refund_id: &str,
    operator: &str,
    now: &str,
) -> Result<RefundStatusOutcome, RefundMutationError> {
    transition(session, refund_id, "", operator, now, true)
}

pub(in crate::repositories) fn reject(
    session: &RepositorySession,
    refund_id: &str,
    reason: &str,
    operator: &str,
    now: &str,
) -> Result<RefundStatusOutcome, RefundMutationError> {
    transition(session, refund_id, reason, operator, now, false)
}

fn transition(
    session: &RepositorySession,
    refund_id: &str,
    reason: &str,
    operator: &str,
    now: &str,
    approve: bool,
) -> Result<RefundStatusOutcome, RefundMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let refund_id = refund_id.to_owned();
    let reason = reason.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT status FROM refunds
                     WHERE id=$1 AND tenant_id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&refund_id)
                .bind(&tenant_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract(format!("refund-not-found:{refund_id}")))?;

                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                if status != "pending" {
                    return Err(contract(format!("refund-not-pending:{status}")));
                }

                let next_status = if approve { "approved" } else { "rejected" };
                if approve {
                    sqlx::query(
                        "UPDATE refunds
                         SET status='approved',approved_by=$1,approved_at=$2,updated_at=$2
                         WHERE id=$3 AND tenant_id=$4",
                    )
                    .bind(&operator)
                    .bind(&now)
                    .bind(&refund_id)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                } else {
                    sqlx::query(
                        "UPDATE refunds
                         SET status='rejected',rejected_by=$1,rejected_at=$2,
                             reason=CASE WHEN $3!='' THEN reason || ' | 驳回: ' || $3 ELSE reason END,
                             updated_at=$2
                         WHERE id=$4 AND tenant_id=$5",
                    )
                    .bind(&operator)
                    .bind(&now)
                    .bind(&reason)
                    .bind(&refund_id)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                }

                Ok(RefundStatusOutcome {
                    refund_id,
                    status: next_status.into(),
                })
            })
        })
        .map_err(map_mutation_error)
}
