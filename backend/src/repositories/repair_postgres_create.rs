#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::repair::{
    RepairCreateOutcome, RepairMutationError, contract, map_mutation_error,
};
use crate::repositories::repair_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn create(
    session: &RepositorySession,
    damage_report_id: &str,
    repair_description: &str,
    vendor: &str,
    operator: &str,
    now: &str,
) -> Result<RepairCreateOutcome, RepairMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let damage_report_id = damage_report_id.to_owned();
    let repair_description = repair_description.to_owned();
    let vendor = vendor.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let damage = sqlx::query(
                    "SELECT device_serial_no,status
                     FROM damage_reports
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&damage_report_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("repair-damage-not-found".into()))?;
                let device_serial_no = damage
                    .try_get::<String, _>("device_serial_no")
                    .map_err(pg_error)?;
                let damage_status = damage.try_get::<String, _>("status").map_err(pg_error)?;

                let existing: Option<String> = sqlx::query_scalar(
                    "SELECT id FROM repair_orders
                     WHERE tenant_id=$1 AND damage_report_id=$2
                     LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&damage_report_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if let Some(existing) = existing {
                    return Err(contract(format!("repair-already-exists:{existing}")));
                }
                if damage_status != "adjudicated" {
                    return Err(contract(format!(
                        "repair-damage-not-adjudicated:{damage_status}"
                    )));
                }

                let repair_id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO repair_orders
                     (id,damage_report_id,device_serial_no,repair_description,vendor,
                      created_by,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$7,$8)",
                )
                .bind(&repair_id)
                .bind(&damage_report_id)
                .bind(&device_serial_no)
                .bind(&repair_description)
                .bind(&vendor)
                .bind(&operator)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(RepairCreateOutcome {
                    repair_id,
                    damage_report_id,
                    device_serial_no,
                })
            })
        })
        .map_err(map_mutation_error)
}
