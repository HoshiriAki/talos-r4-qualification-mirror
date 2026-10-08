#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::procurement::{
    NewProcurementRecord, ProcurementMutationError, ProcurementProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresProcurementRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresProcurementRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn get(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<ProcurementProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id,device_serial_no,
                            purchase_price::double precision AS purchase_price,
                            purchase_date,vendor,invoice_no,
                            replacement_value::double precision AS replacement_value,
                            notes,created_at
                     FROM asset_purchases
                     WHERE tenant_id=$1 AND device_serial_no=$2
                     LIMIT 1",
                )
                .bind(tenant_id)
                .bind(serial)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_procurement).transpose()
            })
        })
    }

    pub(in crate::repositories) fn create(
        &self,
        input: &NewProcurementRecord,
    ) -> Result<ProcurementProjection, ProcurementMutationError> {
        if self.get(&input.device_serial_no)?.is_some() {
            return Err(ProcurementMutationError::Duplicate);
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let input = input.clone();
        let serial = input.device_serial_no.clone();
        self.session
            .pg_write(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "INSERT INTO asset_purchases
                         (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
                          replacement_value,notes,created_at,tenant_id)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                    )
                    .bind(input.id)
                    .bind(input.device_serial_no)
                    .bind(input.purchase_price)
                    .bind(input.purchase_date)
                    .bind(input.vendor)
                    .bind(input.invoice_no)
                    .bind(input.replacement_value)
                    .bind(input.notes)
                    .bind(input.created_at)
                    .bind(tenant_id)
                    .execute(&mut *connection)
                    .await?;
                    Ok(())
                })
            })
            .map_err(map_duplicate)?;

        self.get(&serial)?.ok_or_else(|| {
            RepositoryError::ContractViolation(
                "created procurement record could not be reloaded".into(),
            )
            .into()
        })
    }

    pub(in crate::repositories) fn set_replacement_value(
        &self,
        device_serial_no: &str,
        replacement_value: f64,
    ) -> Result<ProcurementProjection, ProcurementMutationError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        let serial_for_write = serial.clone();
        let changed = self.session.pg_write(move |connection| {
            Box::pin(async move {
                let result = sqlx::query(
                    "UPDATE asset_purchases
                     SET replacement_value=$1
                     WHERE tenant_id=$2 AND device_serial_no=$3",
                )
                .bind(replacement_value)
                .bind(tenant_id)
                .bind(serial_for_write)
                .execute(&mut *connection)
                .await?;
                Ok(result.rows_affected())
            })
        })?;
        if changed == 0 {
            return Err(ProcurementMutationError::NotFound);
        }

        self.get(&serial)?.ok_or_else(|| {
            RepositoryError::ContractViolation(
                "updated procurement record could not be reloaded".into(),
            )
            .into()
        })
    }
}

fn map_duplicate(error: RepositoryError) -> ProcurementMutationError {
    let message = error.to_string();
    if message.contains("idx_asset_purchases_tenant_serial_unique")
        || message.contains("asset_purchases_tenant_id_device_serial_no")
    {
        ProcurementMutationError::Duplicate
    } else {
        ProcurementMutationError::Storage(error)
    }
}

fn map_procurement(row: &sqlx::postgres::PgRow) -> Result<ProcurementProjection, sqlx::Error> {
    Ok(ProcurementProjection {
        id: row.try_get("id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        purchase_price: row.try_get("purchase_price")?,
        purchase_date: row.try_get("purchase_date")?,
        vendor: row.try_get("vendor")?,
        invoice_no: row.try_get("invoice_no")?,
        replacement_value: row.try_get("replacement_value")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
    })
}
