#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::damage::{
    DamageMutationError, DamageTransitionOutcome, contract, map_mutation_error,
};
use crate::repositories::damage_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn assess(
    session: &RepositorySession,
    damage_id: &str,
    estimated_damage_amount: f64,
    liability: &str,
    notes: &str,
    operator: &str,
    now: &str,
) -> Result<DamageTransitionOutcome, DamageMutationError> {
    transition(
        session,
        damage_id,
        Some((estimated_damage_amount, liability, notes)),
        operator,
        now,
    )
}

pub(in crate::repositories) fn adjudicate(
    session: &RepositorySession,
    damage_id: &str,
    operator: &str,
    now: &str,
) -> Result<DamageTransitionOutcome, DamageMutationError> {
    transition(session, damage_id, None, operator, now)
}

fn transition(
    session: &RepositorySession,
    damage_id: &str,
    assessment: Option<(f64, &str, &str)>,
    operator: &str,
    now: &str,
) -> Result<DamageTransitionOutcome, DamageMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let damage_id = damage_id.to_owned();
    let assessment = assessment
        .map(|(amount, liability, notes)| (amount, liability.to_owned(), notes.to_owned()));
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT status,order_id,device_serial_no
                     FROM damage_reports
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&damage_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract(format!("damage-not-found:{damage_id}")))?;

                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                let order_id = row.try_get::<String, _>("order_id").map_err(pg_error)?;
                let device_serial_no = row
                    .try_get::<String, _>("device_serial_no")
                    .map_err(pg_error)?;

                let next_status = if let Some((amount, liability, notes)) = assessment {
                    if status != "reported" {
                        return Err(contract(format!("damage-not-reported:{status}")));
                    }
                    let note = if notes.is_empty() {
                        String::new()
                    } else {
                        format!(" | 定损备注: {notes}")
                    };
                    sqlx::query(
                        "UPDATE damage_reports
                         SET estimated_damage_amount=$1,liability=$2,
                             damage_description=damage_description || $3,
                             status='assessed',assessed_by=$4,assessed_at=$5,updated_at=$5
                         WHERE tenant_id=$6 AND id=$7",
                    )
                    .bind(amount)
                    .bind(&liability)
                    .bind(&note)
                    .bind(&operator)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(&damage_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    "assessed"
                } else {
                    if status != "assessed" {
                        return Err(contract(format!("damage-not-assessed:{status}")));
                    }
                    sqlx::query(
                        "UPDATE damage_reports
                         SET status='adjudicated',adjudicated_by=$1,adjudicated_at=$2,updated_at=$2
                         WHERE tenant_id=$3 AND id=$4",
                    )
                    .bind(&operator)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(&damage_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    "adjudicated"
                };

                Ok(DamageTransitionOutcome {
                    damage_id,
                    order_id,
                    device_serial_no,
                    status: next_status.into(),
                })
            })
        })
        .map_err(map_mutation_error)
}
