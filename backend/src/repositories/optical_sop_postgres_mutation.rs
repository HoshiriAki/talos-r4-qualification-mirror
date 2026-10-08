#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::optical_sop::{
    OpticalCompleteOutcome, OpticalCreateOutcome, OpticalSopMutationError, OpticalUpdateOutcome,
    compute_overall_grade, contract, damage_description, map_mutation_error,
};
use crate::repositories::optical_sop_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn create(
    session: &RepositorySession,
    order_id: &str,
    device_serial_no: &str,
    inspector_id: &str,
    now: &str,
) -> Result<OpticalCreateOutcome, OpticalSopMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let device_serial_no = device_serial_no.to_owned();
    let inspector_id = inspector_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let order_status: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM orders
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                let order_status =
                    order_status.ok_or_else(|| contract("optical-sop-order-not-found".into()))?;
                if order_status != "in_use" && order_status != "shipped" {
                    return Err(contract(format!("optical-sop-order-status:{order_status}")));
                }

                let assigned: Option<String> = sqlx::query_scalar(
                    "SELECT d.serialno
                     FROM order_devices od
                     JOIN devices d
                       ON d.serialno=od.serialno
                      AND d.tenant_id=od.tenant_id
                     WHERE od.tenant_id=$1
                       AND od.orderid=$2
                       AND od.serialno=$3
                     LIMIT 1
                     FOR KEY SHARE OF d",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .bind(&device_serial_no)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if assigned.is_none() {
                    return Err(contract("optical-sop-device-not-in-order".into()));
                }

                let existing: Option<i64> = sqlx::query_scalar(
                    "SELECT id FROM inspection_checklists
                     WHERE tenant_id=$1 AND order_id=$2 AND device_serial_no=$3
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .bind(&device_serial_no)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if existing.is_some() {
                    return Err(contract("optical-sop-duplicate".into()));
                }

                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO inspection_checklists
                     (order_id,device_serial_no,inspector_id,
                      body_ok,lens_ok,screen_ok,accessory_ok,function_ok,
                      overall_grade,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,$3,1,1,1,1,1,'pass',$4,$4,$5)
                     RETURNING id",
                )
                .bind(&order_id)
                .bind(&device_serial_no)
                .bind(&inspector_id)
                .bind(&now)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(OpticalCreateOutcome {
                    id,
                    order_id,
                    device_serial_no,
                    overall_grade: "pass".into(),
                    created_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn update_step(
    session: &RepositorySession,
    id: i64,
    step: &str,
    ok: bool,
    note: Option<&str>,
    now: &str,
) -> Result<OpticalUpdateOutcome, OpticalSopMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let step = step.to_owned();
    let note = note.map(str::to_owned);
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT (body_ok<>0) AS body_ok,
                            (lens_ok<>0) AS lens_ok,
                            (screen_ok<>0) AS screen_ok,
                            (accessory_ok<>0) AS accessory_ok,
                            (function_ok<>0) AS function_ok,
                            completed_at
                     FROM inspection_checklists
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("optical-sop-checklist-not-found".into()))?;

                let mut body_ok = row.try_get::<bool, _>("body_ok").map_err(pg_error)?;
                let mut lens_ok = row.try_get::<bool, _>("lens_ok").map_err(pg_error)?;
                let mut screen_ok = row.try_get::<bool, _>("screen_ok").map_err(pg_error)?;
                let mut accessory_ok = row.try_get::<bool, _>("accessory_ok").map_err(pg_error)?;
                let mut function_ok = row.try_get::<bool, _>("function_ok").map_err(pg_error)?;
                let completed_at = row
                    .try_get::<Option<String>, _>("completed_at")
                    .map_err(pg_error)?;
                if completed_at.is_some() {
                    return Err(contract("optical-sop-completed".into()));
                }

                let (column, note_column) = match step.as_str() {
                    "body" => {
                        body_ok = ok;
                        ("body_ok", "body_note")
                    }
                    "lens" => {
                        lens_ok = ok;
                        ("lens_ok", "lens_note")
                    }
                    "screen" => {
                        screen_ok = ok;
                        ("screen_ok", "screen_note")
                    }
                    "accessory" => {
                        accessory_ok = ok;
                        ("accessory_ok", "accessory_note")
                    }
                    "function" => {
                        function_ok = ok;
                        ("function_ok", "function_note")
                    }
                    _ => return Err(contract("optical-sop-invalid-step".into())),
                };
                let grade =
                    compute_overall_grade(body_ok, lens_ok, screen_ok, accessory_ok, function_ok);

                let sql = format!(
                    "UPDATE inspection_checklists
                     SET {column}=$1,{note_column}=$2,overall_grade=$3,updated_at=$4
                     WHERE tenant_id=$5 AND id=$6 AND completed_at IS NULL"
                );
                let changed = sqlx::query(&sql)
                    .bind(if ok { 1_i32 } else { 0_i32 })
                    .bind(&note)
                    .bind(grade)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();
                if changed != 1 {
                    return Err(contract("optical-sop-completed".into()));
                }

                Ok(OpticalUpdateOutcome {
                    id,
                    step,
                    ok,
                    overall_grade: grade.into(),
                    updated_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn complete(
    session: &RepositorySession,
    id: i64,
    operator: &str,
    now: &str,
) -> Result<OpticalCompleteOutcome, OpticalSopMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT order_id,device_serial_no,
                            (body_ok<>0) AS body_ok,
                            (lens_ok<>0) AS lens_ok,
                            (screen_ok<>0) AS screen_ok,
                            (accessory_ok<>0) AS accessory_ok,
                            (function_ok<>0) AS function_ok,
                            body_note,lens_note,screen_note,accessory_note,function_note,
                            damage_report_id,completed_at
                     FROM inspection_checklists
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("optical-sop-checklist-not-found".into()))?;

                let order_id = row.try_get::<String, _>("order_id").map_err(pg_error)?;
                let device_serial_no = row
                    .try_get::<String, _>("device_serial_no")
                    .map_err(pg_error)?;
                let body_ok = row.try_get::<bool, _>("body_ok").map_err(pg_error)?;
                let lens_ok = row.try_get::<bool, _>("lens_ok").map_err(pg_error)?;
                let screen_ok = row.try_get::<bool, _>("screen_ok").map_err(pg_error)?;
                let accessory_ok = row.try_get::<bool, _>("accessory_ok").map_err(pg_error)?;
                let function_ok = row.try_get::<bool, _>("function_ok").map_err(pg_error)?;
                let body_note = row
                    .try_get::<Option<String>, _>("body_note")
                    .map_err(pg_error)?;
                let lens_note = row
                    .try_get::<Option<String>, _>("lens_note")
                    .map_err(pg_error)?;
                let screen_note = row
                    .try_get::<Option<String>, _>("screen_note")
                    .map_err(pg_error)?;
                let accessory_note = row
                    .try_get::<Option<String>, _>("accessory_note")
                    .map_err(pg_error)?;
                let function_note = row
                    .try_get::<Option<String>, _>("function_note")
                    .map_err(pg_error)?;
                let persisted_damage_id = row
                    .try_get::<Option<String>, _>("damage_report_id")
                    .map_err(pg_error)?;
                let completed_at = row
                    .try_get::<Option<String>, _>("completed_at")
                    .map_err(pg_error)?;

                let grade =
                    compute_overall_grade(body_ok, lens_ok, screen_ok, accessory_ok, function_ok);
                if let Some(completed_at) = completed_at {
                    return Ok(OpticalCompleteOutcome {
                        id,
                        order_id,
                        device_serial_no,
                        overall_grade: grade.into(),
                        damage_report_id: persisted_damage_id,
                        completed_at,
                    });
                }

                let damage_report_id = if grade == "pass" {
                    None
                } else {
                    let damage_id = Uuid::new_v4().to_string();
                    let description = damage_description(
                        body_ok,
                        lens_ok,
                        screen_ok,
                        accessory_ok,
                        function_ok,
                        body_note.as_deref(),
                        lens_note.as_deref(),
                        screen_note.as_deref(),
                        accessory_note.as_deref(),
                        function_note.as_deref(),
                    );
                    sqlx::query(
                        "INSERT INTO damage_reports
                         (id,order_id,device_serial_no,
                          appearance_ok,accessories_ok,function_ok,
                          damage_description,status,reported_by,reported_at,
                          created_at,updated_at,tenant_id)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,'reported',$8,$9,$9,$9,$10)",
                    )
                    .bind(&damage_id)
                    .bind(&order_id)
                    .bind(&device_serial_no)
                    .bind(if body_ok && lens_ok && screen_ok {
                        1_i32
                    } else {
                        0_i32
                    })
                    .bind(if accessory_ok { 1_i32 } else { 0_i32 })
                    .bind(if function_ok { 1_i32 } else { 0_i32 })
                    .bind(&description)
                    .bind(&operator)
                    .bind(&now)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    Some(damage_id)
                };

                let changed = sqlx::query(
                    "UPDATE inspection_checklists
                     SET overall_grade=$1,damage_report_id=$2,completed_at=$3,updated_at=$3
                     WHERE tenant_id=$4 AND id=$5 AND completed_at IS NULL",
                )
                .bind(grade)
                .bind(&damage_report_id)
                .bind(&now)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected();
                if changed != 1 {
                    return Err(contract("optical-sop-completed".into()));
                }

                Ok(OpticalCompleteOutcome {
                    id,
                    order_id,
                    device_serial_no,
                    overall_grade: grade.into(),
                    damage_report_id,
                    completed_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}
