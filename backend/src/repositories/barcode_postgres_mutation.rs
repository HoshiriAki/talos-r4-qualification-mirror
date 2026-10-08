#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::barcode::{
    BarcodeLabelProjection, BarcodeMutationError, ScanEventProjection, barcode_text, contract,
    map_mutation_error,
};
use crate::repositories::barcode_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn generate(
    session: &RepositorySession,
    device_serial_no: &str,
    now: &str,
) -> Result<(BarcodeLabelProjection, bool), BarcodeMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let device_serial_no = device_serial_no.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let device: Option<String> = sqlx::query_scalar(
                    "SELECT serialno FROM devices
                     WHERE tenant_id=$1 AND serialno=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&device_serial_no)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if device.is_none() {
                    return Err(contract("barcode-device-not-found".into()));
                }

                let existing = sqlx::query(
                    "SELECT id,device_serial_no,barcode_text,barcode_type,label_format,generated_at
                     FROM barcode_labels
                     WHERE device_serial_no=$1
                     LIMIT 1",
                )
                .bind(&device_serial_no)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if let Some(row) = existing {
                    return Ok((
                        BarcodeLabelProjection {
                            id: row.try_get("id").map_err(pg_error)?,
                            device_serial_no: row.try_get("device_serial_no").map_err(pg_error)?,
                            barcode_text: row.try_get("barcode_text").map_err(pg_error)?,
                            barcode_type: row.try_get("barcode_type").map_err(pg_error)?,
                            label_format: row.try_get("label_format").map_err(pg_error)?,
                            generated_at: row.try_get("generated_at").map_err(pg_error)?,
                        },
                        true,
                    ));
                }

                let text = barcode_text(&device_serial_no);
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO barcode_labels
                     (device_serial_no,barcode_text,barcode_type,label_format,generated_at)
                     VALUES ($1,$2,'CODE128','50x25mm',$3)
                     RETURNING id",
                )
                .bind(&device_serial_no)
                .bind(&text)
                .bind(&now)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok((
                    BarcodeLabelProjection {
                        id,
                        device_serial_no,
                        barcode_text: text,
                        barcode_type: "CODE128".into(),
                        label_format: "50x25mm".into(),
                        generated_at: now,
                    },
                    false,
                ))
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn batch_generate(
    session: &RepositorySession,
    now: &str,
) -> Result<i64, BarcodeMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT serialno FROM devices
                     WHERE tenant_id=$1
                     ORDER BY serialno
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .fetch_all(&mut *connection)
                .await
                .map_err(pg_error)?;

                let mut generated = 0_i64;
                for row in rows {
                    let serial_no = row.try_get::<String, _>("serialno").map_err(pg_error)?;
                    let exists: Option<i64> = sqlx::query_scalar(
                        "SELECT id FROM barcode_labels
                         WHERE device_serial_no=$1
                         LIMIT 1",
                    )
                    .bind(&serial_no)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    if exists.is_some() {
                        continue;
                    }

                    let text = barcode_text(&serial_no);
                    sqlx::query(
                        "INSERT INTO barcode_labels
                         (device_serial_no,barcode_text,barcode_type,label_format,generated_at)
                         VALUES ($1,$2,'CODE128','50x25mm',$3)",
                    )
                    .bind(&serial_no)
                    .bind(&text)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    generated += 1;
                }

                Ok(generated)
            })
        })
        .map_err(map_mutation_error)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::repositories) fn record_scan(
    session: &RepositorySession,
    device_serial_no: &str,
    barcode_text: Option<&str>,
    scan_type: &str,
    scanned_by: &str,
    warehouse_id: Option<&str>,
    notes: Option<&str>,
    now: &str,
) -> Result<ScanEventProjection, BarcodeMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let device_serial_no = device_serial_no.to_owned();
    let barcode_text = barcode_text.map(str::to_owned);
    let scan_type = scan_type.to_owned();
    let scanned_by = scanned_by.to_owned();
    let warehouse_id = warehouse_id.map(str::to_owned);
    let notes = notes.map(str::to_owned);
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let device: Option<String> = sqlx::query_scalar(
                    "SELECT serialno FROM devices
                     WHERE tenant_id=$1 AND serialno=$2
                     LIMIT 1
                     FOR KEY SHARE",
                )
                .bind(&tenant_id)
                .bind(&device_serial_no)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;

                let membership: Option<String> = sqlx::query_scalar(
                    "SELECT identity_id FROM tenant_memberships
                     WHERE tenant_id=$1 AND identity_id=$2 AND status='active'
                     LIMIT 1
                     FOR KEY SHARE",
                )
                .bind(&tenant_id)
                .bind(&scanned_by)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;

                let warehouse_exists = if let Some(warehouse_id) = &warehouse_id {
                    let warehouse: Option<String> = sqlx::query_scalar(
                        "SELECT id FROM warehouses
                         WHERE tenant_id=$1 AND id=$2
                         LIMIT 1
                         FOR KEY SHARE",
                    )
                    .bind(&tenant_id)
                    .bind(warehouse_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    warehouse.is_some()
                } else {
                    true
                };

                if device.is_none() || membership.is_none() || !warehouse_exists {
                    return Err(contract("barcode-scan-references-not-found".into()));
                }

                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO scan_events
                     (device_serial_no,barcode_text,scan_type,scanned_by,warehouse_id,
                      notes,created_at,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                     RETURNING id",
                )
                .bind(&device_serial_no)
                .bind(&barcode_text)
                .bind(&scan_type)
                .bind(&scanned_by)
                .bind(&warehouse_id)
                .bind(&notes)
                .bind(&now)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(ScanEventProjection {
                    id,
                    device_serial_no,
                    barcode_text,
                    scan_type,
                    scanned_by,
                    warehouse_id,
                    notes,
                    created_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}
