use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

pub(in crate::repositories) const ALREADY_RUN_PREFIX: &str = "depreciation-period-already-run:";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepreciationSnapshot {
    pub purchase_price: f64,
    pub total_depreciation: f64,
    pub depreciation_months: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepreciationLogProjection {
    pub id: String,
    pub device_serial_no: String,
    pub period: String,
    pub opening_value: f64,
    pub depreciation_amount: f64,
    pub closing_value: f64,
    pub method: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyDepreciationRun {
    pub period: String,
    pub devices_processed: i64,
    pub total_devices: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum DepreciationMutationError {
    #[error("depreciation already ran for this period ({0} rows)")]
    AlreadyRun(i64),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteDepreciationRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteDepreciationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn snapshot(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<DepreciationSnapshot>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT ap.purchase_price,
                            COALESCE((
                                SELECT SUM(dl.depreciation_amount)
                                FROM depreciation_log dl
                                WHERE dl.tenant_id=ap.tenant_id
                                  AND dl.device_serial_no=ap.device_serial_no
                            ),0),
                            (
                                SELECT COUNT(*)
                                FROM depreciation_log dl
                                WHERE dl.tenant_id=ap.tenant_id
                                  AND dl.device_serial_no=ap.device_serial_no
                            )
                     FROM asset_purchases ap
                     WHERE ap.tenant_id=?1 AND ap.device_serial_no=?2
                     LIMIT 1",
                    params![tenant_id, serial],
                    |row| {
                        Ok(DepreciationSnapshot {
                            purchase_price: row.get(0)?,
                            total_depreciation: row.get(1)?,
                            depreciation_months: row.get(2)?,
                        })
                    },
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn logs(
        &self,
        device_serial_no: &str,
    ) -> Result<Vec<DepreciationLogProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,device_serial_no,period,opening_value,depreciation_amount,
                        closing_value,method,created_at
                 FROM depreciation_log
                 WHERE tenant_id=?1 AND device_serial_no=?2
                 ORDER BY period DESC",
            )?;
            statement
                .query_map(params![tenant_id, serial], map_log)?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn run_monthly(
        &self,
        period: &str,
        useful_life_months: i32,
        now: &str,
    ) -> Result<MonthlyDepreciationRun, DepreciationMutationError> {
        let tenant_id = self.tenant_id();
        let period = period.to_owned();
        let now = now.to_owned();
        let useful_life = f64::from(useful_life_months);

        self.session
            .write_immediate(move |transaction| {
                let existing_count: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*) FROM depreciation_log
                         WHERE tenant_id=?1 AND period=?2",
                        params![tenant_id, period],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_error)?;
                if existing_count > 0 {
                    return Err(RepositoryError::ContractViolation(format!(
                        "{ALREADY_RUN_PREFIX}{existing_count}"
                    )));
                }

                let purchases = {
                    let mut statement = transaction
                        .prepare(
                            "SELECT device_serial_no,purchase_price
                             FROM asset_purchases
                             WHERE tenant_id=?1
                             ORDER BY device_serial_no",
                        )
                        .map_err(sqlite_error)?;
                    statement
                        .query_map([tenant_id.as_str()], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                        })
                        .map_err(sqlite_error)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(sqlite_error)?
                };

                let total_devices = i64::try_from(purchases.len()).unwrap_or(i64::MAX);
                let mut processed = 0_i64;
                for (device_serial_no, purchase_price) in purchases {
                    let total_depreciation: f64 = transaction
                        .query_row(
                            "SELECT COALESCE(SUM(depreciation_amount),0)
                             FROM depreciation_log
                             WHERE tenant_id=?1 AND device_serial_no=?2",
                            params![tenant_id, device_serial_no],
                            |row| row.get(0),
                        )
                        .map_err(sqlite_error)?;
                    let opening_value = (purchase_price - total_depreciation).max(0.0);
                    if opening_value <= 0.0 {
                        continue;
                    }
                    let monthly_depreciation = purchase_price / useful_life;
                    let closing_value = (opening_value - monthly_depreciation).max(0.0);
                    transaction
                        .execute(
                            "INSERT INTO depreciation_log
                             (id,device_serial_no,period,opening_value,depreciation_amount,
                              closing_value,method,created_at,tenant_id)
                             VALUES (?1,?2,?3,?4,?5,?6,'straight_line',?7,?8)",
                            params![
                                Uuid::new_v4().to_string(),
                                device_serial_no,
                                period,
                                opening_value,
                                monthly_depreciation,
                                closing_value,
                                now,
                                tenant_id,
                            ],
                        )
                        .map_err(sqlite_error)?;
                    processed += 1;
                }

                Ok(MonthlyDepreciationRun {
                    period,
                    devices_processed: processed,
                    total_devices,
                })
            })
            .map_err(map_monthly_error)
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn map_monthly_error(
    error: RepositoryError,
) -> DepreciationMutationError {
    if let RepositoryError::ContractViolation(message) = &error
        && let Some(count) = message.strip_prefix(ALREADY_RUN_PREFIX)
        && let Ok(count) = count.parse::<i64>()
    {
        return DepreciationMutationError::AlreadyRun(count);
    }
    DepreciationMutationError::Storage(error)
}

fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<DepreciationLogProjection> {
    Ok(DepreciationLogProjection {
        id: row.get(0)?,
        device_serial_no: row.get(1)?,
        period: row.get(2)?,
        opening_value: row.get(3)?,
        depreciation_amount: row.get(4)?,
        closing_value: row.get(5)?,
        method: row.get(6)?,
        created_at: row.get(7)?,
    })
}
