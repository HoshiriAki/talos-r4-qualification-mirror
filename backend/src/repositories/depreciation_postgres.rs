#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::depreciation::{
    ALREADY_RUN_PREFIX, DepreciationLogProjection, DepreciationMutationError, DepreciationSnapshot,
    MonthlyDepreciationRun, map_monthly_error,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresDepreciationRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresDepreciationRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn snapshot(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<DepreciationSnapshot>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT ap.purchase_price::double precision AS purchase_price,
                            COALESCE((
                                SELECT SUM(dl.depreciation_amount)::double precision
                                FROM depreciation_log dl
                                WHERE dl.tenant_id=ap.tenant_id
                                  AND dl.device_serial_no=ap.device_serial_no
                            ),0::double precision) AS total_depreciation,
                            (
                                SELECT COUNT(*)::bigint
                                FROM depreciation_log dl
                                WHERE dl.tenant_id=ap.tenant_id
                                  AND dl.device_serial_no=ap.device_serial_no
                            ) AS depreciation_months
                     FROM asset_purchases ap
                     WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2
                     LIMIT 1",
                )
                .bind(tenant_id)
                .bind(serial)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_snapshot).transpose()
            })
        })
    }

    pub(in crate::repositories) fn logs(
        &self,
        device_serial_no: &str,
    ) -> Result<Vec<DepreciationLogProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT id,device_serial_no,period,
                            opening_value::double precision AS opening_value,
                            depreciation_amount::double precision AS depreciation_amount,
                            closing_value::double precision AS closing_value,
                            method,created_at
                     FROM depreciation_log
                     WHERE tenant_id=$1 AND device_serial_no=$2
                     ORDER BY period DESC",
                )
                .bind(tenant_id)
                .bind(serial)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_log)
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn run_monthly(
        &self,
        period: &str,
        useful_life_months: i32,
        now: &str,
    ) -> Result<MonthlyDepreciationRun, DepreciationMutationError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let period = period.to_owned();
        let now = now.to_owned();
        let useful_life = f64::from(useful_life_months);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let existing_count = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint
                         FROM depreciation_log
                         WHERE tenant_id=$1 AND period=$2",
                    )
                    .bind(&tenant_id)
                    .bind(&period)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    if existing_count > 0 {
                        return Err(RepositoryError::ContractViolation(format!(
                            "{ALREADY_RUN_PREFIX}{existing_count}"
                        )));
                    }

                    let purchases = sqlx::query(
                        "SELECT device_serial_no,
                                purchase_price::double precision AS purchase_price
                         FROM asset_purchases
                         WHERE tenant_id=$1
                         ORDER BY device_serial_no",
                    )
                    .bind(&tenant_id)
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let total_devices = i64::try_from(purchases.len()).unwrap_or(i64::MAX);
                    let mut processed = 0_i64;
                    for purchase in purchases {
                        let device_serial_no: String =
                            purchase.try_get("device_serial_no").map_err(pg_error)?;
                        let purchase_price: f64 =
                            purchase.try_get("purchase_price").map_err(pg_error)?;

                        let total_depreciation = sqlx::query_scalar::<_, f64>(
                            "SELECT COALESCE(
                                SUM(depreciation_amount)::double precision,
                                0::double precision
                             )
                             FROM depreciation_log
                             WHERE tenant_id=$1 AND device_serial_no=$2",
                        )
                        .bind(&tenant_id)
                        .bind(&device_serial_no)
                        .fetch_one(&mut *connection)
                        .await
                        .map_err(pg_error)?;

                        let opening_value = (purchase_price - total_depreciation).max(0.0);
                        if opening_value <= 0.0 {
                            continue;
                        }
                        let monthly_depreciation = purchase_price / useful_life;
                        let closing_value = (opening_value - monthly_depreciation).max(0.0);

                        sqlx::query(
                            "INSERT INTO depreciation_log
                             (id,device_serial_no,period,opening_value,depreciation_amount,
                              closing_value,method,created_at,tenant_id)
                             VALUES ($1,$2,$3,$4,$5,$6,'straight_line',$7,$8)",
                        )
                        .bind(uuid::Uuid::new_v4().to_string())
                        .bind(&device_serial_no)
                        .bind(&period)
                        .bind(opening_value)
                        .bind(monthly_depreciation)
                        .bind(closing_value)
                        .bind(&now)
                        .bind(&tenant_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                        processed += 1;
                    }

                    Ok(MonthlyDepreciationRun {
                        period,
                        devices_processed: processed,
                        total_devices,
                    })
                })
            })
            .map_err(map_monthly_error)
    }
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn map_snapshot(row: &sqlx::postgres::PgRow) -> Result<DepreciationSnapshot, sqlx::Error> {
    Ok(DepreciationSnapshot {
        purchase_price: row.try_get("purchase_price")?,
        total_depreciation: row.try_get("total_depreciation")?,
        depreciation_months: row.try_get("depreciation_months")?,
    })
}

fn map_log(row: &sqlx::postgres::PgRow) -> Result<DepreciationLogProjection, sqlx::Error> {
    Ok(DepreciationLogProjection {
        id: row.try_get("id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        period: row.try_get("period")?,
        opening_value: row.try_get("opening_value")?,
        depreciation_amount: row.try_get("depreciation_amount")?,
        closing_value: row.try_get("closing_value")?,
        method: row.try_get("method")?,
        created_at: row.try_get("created_at")?,
    })
}
