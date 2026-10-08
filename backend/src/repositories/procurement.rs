use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcurementProjection {
    pub id: String,
    pub device_serial_no: String,
    pub purchase_price: f64,
    pub purchase_date: String,
    pub vendor: String,
    pub invoice_no: String,
    pub replacement_value: f64,
    pub notes: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewProcurementRecord {
    pub id: String,
    pub device_serial_no: String,
    pub purchase_price: f64,
    pub purchase_date: String,
    pub vendor: String,
    pub invoice_no: String,
    pub replacement_value: f64,
    pub notes: String,
    pub created_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcurementMutationError {
    #[error("asset purchase already exists")]
    Duplicate,
    #[error("asset purchase not found")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteProcurementRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteProcurementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn get(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<ProcurementProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
                            replacement_value,notes,created_at
                     FROM asset_purchases
                     WHERE tenant_id=?1 AND device_serial_no=?2
                     LIMIT 1",
                    params![tenant_id, serial],
                    map_procurement,
                )
                .optional()
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
            .write(move |transaction| {
                transaction.execute(
                    "INSERT INTO asset_purchases
                     (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
                      replacement_value,notes,created_at,tenant_id)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        input.id,
                        input.device_serial_no,
                        input.purchase_price,
                        input.purchase_date,
                        input.vendor,
                        input.invoice_no,
                        input.replacement_value,
                        input.notes,
                        input.created_at,
                        tenant_id,
                    ],
                )?;
                Ok(())
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
        let changed = self.session.write(move |transaction| {
            transaction.execute(
                "UPDATE asset_purchases
                 SET replacement_value=?1
                 WHERE tenant_id=?2 AND device_serial_no=?3",
                params![replacement_value, tenant_id, serial_for_write],
            )
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
    if message.contains("asset_purchases.tenant_id, asset_purchases.device_serial_no")
        || message.contains("idx_asset_purchases_tenant_serial_unique")
    {
        ProcurementMutationError::Duplicate
    } else {
        ProcurementMutationError::Storage(error)
    }
}

fn map_procurement(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProcurementProjection> {
    Ok(ProcurementProjection {
        id: row.get(0)?,
        device_serial_no: row.get(1)?,
        purchase_price: row.get(2)?,
        purchase_date: row.get(3)?,
        vendor: row.get(4)?,
        invoice_no: row.get(5)?,
        replacement_value: row.get(6)?,
        notes: row.get(7)?,
        created_at: row.get(8)?,
    })
}
