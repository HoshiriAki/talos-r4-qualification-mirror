#![cfg(feature = "postgres")]

use uuid::Uuid;

use crate::repositories::damage::{
    DamageMutationError, DamageReportOutcome, contract, map_mutation_error,
};
use crate::repositories::damage_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn report(
    session: &RepositorySession,
    order_id: &str,
    device_serial_no: &str,
    appearance_ok: bool,
    accessories_ok: bool,
    function_ok: bool,
    damage_description: &str,
    operator: &str,
    now: &str,
) -> Result<DamageReportOutcome, DamageMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let device_serial_no = device_serial_no.to_owned();
    let damage_description = damage_description.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let exists: Option<i32> = sqlx::query_scalar(
                    "SELECT 1
                     FROM orders o JOIN devices d ON d.serialno=$1
                     WHERE o.tenant_id=$2 AND d.tenant_id=$2 AND o.id=$3
                     LIMIT 1",
                )
                .bind(&device_serial_no)
                .bind(&tenant_id)
                .bind(&order_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if exists.is_none() {
                    return Err(contract("damage-resource-not-found".into()));
                }

                let damage_id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO damage_reports
                     (id,order_id,device_serial_no,appearance_ok,accessories_ok,function_ok,
                      damage_description,status,reported_by,reported_at,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,'reported',$8,$9,$9,$9,$10)",
                )
                .bind(&damage_id)
                .bind(&order_id)
                .bind(&device_serial_no)
                .bind(if appearance_ok { 1_i32 } else { 0_i32 })
                .bind(if accessories_ok { 1_i32 } else { 0_i32 })
                .bind(if function_ok { 1_i32 } else { 0_i32 })
                .bind(&damage_description)
                .bind(&operator)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(DamageReportOutcome {
                    damage_id,
                    order_id,
                    device_serial_no,
                })
            })
        })
        .map_err(map_mutation_error)
}
