#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::repair::{
    RepairMutationError, RepairTransitionOutcome, contract, map_mutation_error,
};
use crate::repositories::repair_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn start(
    session: &RepositorySession,
    repair_id: &str,
    now: &str,
) -> Result<RepairTransitionOutcome, RepairMutationError> {
    transition(session, repair_id, Transition::Start, "", 0.0, now)
}

pub(in crate::repositories) fn complete(
    session: &RepositorySession,
    repair_id: &str,
    repair_cost: f64,
    operator: &str,
    now: &str,
) -> Result<RepairTransitionOutcome, RepairMutationError> {
    transition(
        session,
        repair_id,
        Transition::Complete,
        operator,
        repair_cost,
        now,
    )
}

pub(in crate::repositories) fn return_to_stock(
    session: &RepositorySession,
    repair_id: &str,
    operator: &str,
    now: &str,
) -> Result<RepairTransitionOutcome, RepairMutationError> {
    transition(
        session,
        repair_id,
        Transition::ReturnToStock,
        operator,
        0.0,
        now,
    )
}

#[derive(Clone, Copy)]
enum Transition {
    Start,
    Complete,
    ReturnToStock,
}

fn transition(
    session: &RepositorySession,
    repair_id: &str,
    transition: Transition,
    operator: &str,
    repair_cost: f64,
    now: &str,
) -> Result<RepairTransitionOutcome, RepairMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let repair_id = repair_id.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT status,device_serial_no
                     FROM repair_orders
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&repair_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("repair-not-found".into()))?;
                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                let device_serial_no = row
                    .try_get::<String, _>("device_serial_no")
                    .map_err(pg_error)?;

                match transition {
                    Transition::Start => {
                        if status != "pending" {
                            return Err(contract(format!("repair-not-pending:{status}")));
                        }
                        sqlx::query(
                            "UPDATE repair_orders
                             SET status='in_progress',started_at=$1,updated_at=$1
                             WHERE tenant_id=$2 AND id=$3 AND status='pending'",
                        )
                        .bind(&now)
                        .bind(&tenant_id)
                        .bind(&repair_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;

                        Ok(RepairTransitionOutcome {
                            repair_id,
                            status: "in_progress".into(),
                            device_serial_no: None,
                            repair_cost: None,
                        })
                    }
                    Transition::Complete => {
                        if status != "in_progress" {
                            return Err(contract(format!("repair-not-in-progress:{status}")));
                        }
                        sqlx::query(
                            "UPDATE repair_orders
                             SET status='completed',repair_cost=$1,completed_by=$2,
                                 completed_at=$3,updated_at=$3
                             WHERE tenant_id=$4 AND id=$5 AND status='in_progress'",
                        )
                        .bind(repair_cost)
                        .bind(&operator)
                        .bind(&now)
                        .bind(&tenant_id)
                        .bind(&repair_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;

                        Ok(RepairTransitionOutcome {
                            repair_id,
                            status: "completed".into(),
                            device_serial_no: None,
                            repair_cost: Some(repair_cost),
                        })
                    }
                    Transition::ReturnToStock => {
                        if status != "completed" {
                            return Err(contract(format!("repair-not-completed:{status}")));
                        }

                        let device_updated = sqlx::query(
                            "UPDATE devices SET rentalstatus='available'
                             WHERE tenant_id=$1 AND serialno=$2",
                        )
                        .bind(&tenant_id)
                        .bind(&device_serial_no)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected();
                        if device_updated != 1 {
                            return Err(contract("repair-device-not-found".into()));
                        }

                        let repair_updated = sqlx::query(
                            "UPDATE repair_orders
                             SET status='returned',returned_by=$1,returned_at=$2,updated_at=$2
                             WHERE tenant_id=$3 AND id=$4 AND status='completed'",
                        )
                        .bind(&operator)
                        .bind(&now)
                        .bind(&tenant_id)
                        .bind(&repair_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected();
                        if repair_updated != 1 {
                            return Err(contract("repair-not-completed:changed".into()));
                        }

                        Ok(RepairTransitionOutcome {
                            repair_id,
                            status: "returned".into(),
                            device_serial_no: Some(device_serial_no),
                            repair_cost: None,
                        })
                    }
                }
            })
        })
        .map_err(map_mutation_error)
}
