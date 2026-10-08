#[cfg(feature = "postgres")]
use std::future::Future;

use sqlx::Row;
use sqlx::postgres::PgPool;
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::error::AppError;

use super::maintenance_compatibility::{MaintenanceStatusSync, overdue_capabilities, overdue_risk};

const OVERDUE_GRACE_DAYS: i32 = 4;

#[derive(Clone)]
pub(crate) struct PostgresMaintenanceCompatibilityRepository {
    pool: PgPool,
}

impl PostgresMaintenanceCompatibilityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn sync_order_statuses(
        &self,
        today: &str,
    ) -> Result<MaintenanceStatusSync, AppError> {
        let pool = self.pool.clone();
        let today = today.to_owned();
        run_pg_maintenance(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let reserved = sqlx::query(
                "UPDATE orders
                 SET status='reserved'
                 WHERE deliveryDate > $1 AND status != 'completed'",
            )
            .bind(&today)
            .execute(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .rows_affected();
            let activated = sqlx::query(
                "UPDATE orders
                 SET status='active'
                 WHERE deliveryDate <= $1 AND status='reserved'",
            )
            .bind(&today)
            .execute(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .rows_affected();
            tx.commit().await.map_err(pg_persistence)?;
            Ok(MaintenanceStatusSync {
                reserved: usize::try_from(reserved).map_err(|_| {
                    AppError::Internal("maintenance reserved count overflow".into())
                })?,
                activated: usize::try_from(activated).map_err(|_| {
                    AppError::Internal("maintenance activated count overflow".into())
                })?,
            })
        })
    }

    pub(crate) fn seed_overdue_tasks(&self, today: &str, now: &str) -> Result<usize, AppError> {
        let pool = self.pool.clone();
        let today = today.to_owned();
        let now = now.to_owned();
        run_pg_maintenance(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let rows = sqlx::query(
                "SELECT COALESCE(od.serialNo, o.deviceSerialNo), o.id, o.orderNo, o.endDate,
                        (CAST($1 AS date) - CAST(o.endDate AS date))::BIGINT, o.tenant_id
                 FROM orders o
                 LEFT JOIN order_devices od
                   ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                 WHERE COALESCE(od.serialNo, o.deviceSerialNo) IS NOT NULL
                   AND CAST(o.endDate AS date) < CAST($1 AS date) - $2::integer
                   AND o.status IN ('active','in_use')
                 ORDER BY o.endDate ASC",
            )
            .bind(&today)
            .bind(OVERDUE_GRACE_DAYS)
            .fetch_all(&mut *tx)
            .await
            .map_err(pg_persistence)?;

            let mut created = 0usize;
            for row in rows {
                let serial_no: String = row.try_get(0).map_err(pg_persistence)?;
                let order_id: String = row.try_get(1).map_err(pg_persistence)?;
                let order_no: String = row.try_get(2).map_err(pg_persistence)?;
                let end_date: String = row.try_get(3).map_err(pg_persistence)?;
                let overdue_days: i64 = row.try_get(4).map_err(pg_persistence)?;
                let tenant_id: String = row.try_get(5).map_err(pg_persistence)?;
                let task_id = format!("overdue-device-{order_id}-{serial_no}");
                let risk = overdue_risk(overdue_days);
                let capabilities = overdue_capabilities(risk);
                let work_route = serde_json::json!({
                    "name": "customers",
                    "query": { "orderId": order_id }
                })
                .to_string();

                let inserted = sqlx::query(
                    "INSERT INTO work_tasks
                     (id,kind,status,risk,source_type,source_id,title,summary,reason,due_at,
                      capabilities_json,work_route_json,created_at,updated_at,tenant_id)
                     VALUES ($1,'overdue-device','queued',$2,'device',$3,$4,$5,$6,$7,$8,$9,
                             CAST($10 AS TIMESTAMPTZ),CAST($10 AS TIMESTAMPTZ),$11)
                     ON CONFLICT (id) DO NOTHING",
                )
                .bind(task_id)
                .bind(risk)
                .bind(&serial_no)
                .bind(format!("{serial_no} / {order_no}"))
                .bind(format!("逾期{overdue_days}天 (截止{end_date})"))
                .bind(format!("设备超期，截止{end_date}"))
                .bind(format!("{end_date}T23:59:59+08:00"))
                .bind(capabilities)
                .bind(work_route)
                .bind(&now)
                .bind(tenant_id)
                .execute(&mut *tx)
                .await
                .map_err(pg_persistence)?
                .rows_affected();
                created = created
                    .checked_add(usize::try_from(inserted).map_err(|_| {
                        AppError::Internal("maintenance overdue count overflow".into())
                    })?)
                    .ok_or_else(|| {
                        AppError::Internal("maintenance overdue count overflow".into())
                    })?;
            }
            tx.commit().await.map_err(pg_persistence)?;
            Ok(created)
        })
    }
}

async fn set_serializable(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await
        .map_err(pg_persistence)?;
    Ok(())
}

fn run_pg_maintenance<T, F>(future: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    let handle = Handle::try_current().map_err(|_| AppError::ServiceError {
        code: "SYS_MAINTENANCE_RUNTIME".into(),
        message: "maintenance persistence runtime unavailable".into(),
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(AppError::ServiceError {
            code: "SYS_MAINTENANCE_RUNTIME".into(),
            message: "maintenance persistence requires the multi-thread runtime".into(),
        });
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn pg_persistence<T>(_error: T) -> AppError {
    AppError::ServiceError {
        code: "SYS_MAINTENANCE_PERSISTENCE".into(),
        message: "maintenance persistence operation failed".into(),
    }
}
