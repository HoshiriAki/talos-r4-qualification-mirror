use std::future::Future;

use sqlx::Transaction;
use sqlx::postgres::PgPool;
use tokio::runtime::{Handle, RuntimeFlavor};

use super::scheduler_persistence_contract::{
    INTEGRATION_TENANT_PAGE_SIZE, INTEGRATION_WORKER_SCHEDULER_ID, IntegrationSchedulerPersistence,
    StartupRecoveryPage,
};
use super::types::{ExternalOperationId, IntegrationError};

#[derive(Clone)]
pub(crate) struct PostgresIntegrationSchedulerPersistence {
    pool: PgPool,
}

impl PostgresIntegrationSchedulerPersistence {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl IntegrationSchedulerPersistence for PostgresIntegrationSchedulerPersistence {
    fn begin_startup_recovery_snapshot(&self) -> Result<String, IntegrationError> {
        let pool = self.pool.clone();
        run_pg_scheduler(async move {
            let mut transaction = serializable(&pool).await?;
            let recovery_id = uuid::Uuid::new_v4().to_string();
            let timestamp = now();

            sqlx::query(
                "DELETE FROM integration_startup_recovery_snapshot
                 WHERE scheduler_id=$1",
            )
            .bind(INTEGRATION_WORKER_SCHEDULER_ID)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            sqlx::query(
                "INSERT INTO integration_startup_recovery_snapshot
                    (scheduler_id,recovery_id,tenant_id,work_kind,work_id,captured_at)
                 SELECT $1,$2,tenant_id,'external_operation',id,$3
                 FROM external_operations
                 WHERE state='dispatching'",
            )
            .bind(INTEGRATION_WORKER_SCHEDULER_ID)
            .bind(&recovery_id)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            sqlx::query(
                "INSERT INTO integration_startup_recovery_snapshot
                    (scheduler_id,recovery_id,tenant_id,work_kind,work_id,captured_at)
                 SELECT $1,$2,tenant_id,'webhook',id,$3
                 FROM webhook_inbox
                 WHERE status='processing'",
            )
            .bind(INTEGRATION_WORKER_SCHEDULER_ID)
            .bind(&recovery_id)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)?;
            Ok(recovery_id)
        })
    }

    fn recover_next_startup_snapshot_page(
        &self,
        recovery_id: &str,
    ) -> Result<StartupRecoveryPage, IntegrationError> {
        let pool = self.pool.clone();
        let recovery_id = recovery_id.to_owned();

        run_pg_scheduler(async move {
            let mut transaction = serializable(&pool).await?;
            let tenant_ids = startup_recovery_tenant_page(&mut transaction, &recovery_id).await?;
            if tenant_ids.is_empty() {
                transaction.commit().await.map_err(persistence)?;
                return Ok(StartupRecoveryPage::default());
            }

            let timestamp = now();
            let mut page = StartupRecoveryPage {
                tenants: u64::try_from(tenant_ids.len()).map_err(persistence)?,
                ..Default::default()
            };

            for tenant_id in &tenant_ids {
                let operation_ids = sqlx::query_scalar::<_, String>(
                    "SELECT work_id
                     FROM integration_startup_recovery_snapshot
                     WHERE scheduler_id=$1
                       AND recovery_id=$2
                       AND tenant_id=$3
                       AND work_kind='external_operation'
                     ORDER BY work_id
                     FOR UPDATE",
                )
                .bind(INTEGRATION_WORKER_SCHEDULER_ID)
                .bind(&recovery_id)
                .bind(tenant_id)
                .fetch_all(&mut *transaction)
                .await
                .map_err(persistence)?;

                for id in operation_ids {
                    let operation_id = ExternalOperationId::new(id)?;
                    let changed = sqlx::query(
                        "UPDATE external_operations
                         SET state='unknown_outcome',
                             classification='worker_restarted_after_dispatch',
                             next_retry_at=NULL,
                             updated_at=$1
                         WHERE tenant_id=$2 AND id=$3 AND state='dispatching'",
                    )
                    .bind(&timestamp)
                    .bind(tenant_id)
                    .bind(operation_id.as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(persistence)?
                    .rows_affected();

                    if changed == 1 {
                        sqlx::query(
                            "UPDATE external_operation_attempts
                             SET state='unknown_outcome',
                                 classification='worker_restarted_after_dispatch',
                                 completed_at=$1
                             WHERE tenant_id=$2
                               AND operation_id=$3
                               AND state='dispatching'",
                        )
                        .bind(&timestamp)
                        .bind(tenant_id)
                        .bind(operation_id.as_str())
                        .execute(&mut *transaction)
                        .await
                        .map_err(persistence)?;

                        insert_runtime_event(
                            &mut transaction,
                            tenant_id,
                            &operation_id,
                            "recovered_after_restart",
                            "worker_restarted_after_dispatch",
                            &timestamp,
                        )
                        .await?;
                        page.recovered_operations += 1;
                    }
                }

                let webhook_ids = sqlx::query_scalar::<_, String>(
                    "SELECT work_id
                     FROM integration_startup_recovery_snapshot
                     WHERE scheduler_id=$1
                       AND recovery_id=$2
                       AND tenant_id=$3
                       AND work_kind='webhook'
                     ORDER BY work_id
                     FOR UPDATE",
                )
                .bind(INTEGRATION_WORKER_SCHEDULER_ID)
                .bind(&recovery_id)
                .bind(tenant_id)
                .fetch_all(&mut *transaction)
                .await
                .map_err(persistence)?;

                for inbox_id in webhook_ids {
                    let changed = sqlx::query(
                        "UPDATE webhook_inbox
                         SET status='verified',
                             error_classification='worker_restarted_during_processing'
                         WHERE tenant_id=$1 AND id=$2 AND status='processing'",
                    )
                    .bind(tenant_id)
                    .bind(&inbox_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(persistence)?
                    .rows_affected();

                    if changed == 1 {
                        sqlx::query(
                            "UPDATE webhook_processing_attempts
                             SET state='retryable_failure',
                                 classification='worker_restarted_during_processing',
                                 completed_at=$1
                             WHERE tenant_id=$2
                               AND inbox_id=$3
                               AND state='processing'",
                        )
                        .bind(&timestamp)
                        .bind(tenant_id)
                        .bind(&inbox_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(persistence)?;
                        page.recovered_webhooks += 1;
                    }
                }

                sqlx::query(
                    "DELETE FROM integration_startup_recovery_snapshot
                     WHERE scheduler_id=$1 AND recovery_id=$2 AND tenant_id=$3",
                )
                .bind(INTEGRATION_WORKER_SCHEDULER_ID)
                .bind(&recovery_id)
                .bind(tenant_id)
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;
            }

            transaction.commit().await.map_err(persistence)?;
            Ok(page)
        })
    }

    fn next_tenant_work_page(&self) -> Result<Vec<String>, IntegrationError> {
        let pool = self.pool.clone();
        run_pg_scheduler(async move {
            let mut transaction = serializable(&pool).await?;
            let timestamp = now();

            sqlx::query(
                "INSERT INTO integration_scheduler_cursor
                    (scheduler_id,tenant_id,updated_at)
                 VALUES ($1,NULL,$2)
                 ON CONFLICT (scheduler_id) DO NOTHING",
            )
            .bind(INTEGRATION_WORKER_SCHEDULER_ID)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            let cursor: Option<String> = sqlx::query_scalar(
                "SELECT tenant_id
                 FROM integration_scheduler_cursor
                 WHERE scheduler_id=$1
                 FOR UPDATE",
            )
            .bind(INTEGRATION_WORKER_SCHEDULER_ID)
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;

            let mut tenants = tenant_work_page_after(&mut transaction, cursor.as_deref()).await?;
            if tenants.is_empty() && cursor.is_some() {
                tenants = tenant_work_page_after(&mut transaction, None).await?;
            }

            let next_cursor = tenants.last().cloned();
            sqlx::query(
                "UPDATE integration_scheduler_cursor
                 SET tenant_id=$1,updated_at=$2
                 WHERE scheduler_id=$3",
            )
            .bind(next_cursor.as_deref())
            .bind(now())
            .bind(INTEGRATION_WORKER_SCHEDULER_ID)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)?;
            Ok(tenants)
        })
    }
}

async fn tenant_work_page_after(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    after_tenant_id: Option<&str>,
) -> Result<Vec<String>, IntegrationError> {
    match after_tenant_id {
        Some(tenant_id) => sqlx::query_scalar::<_, String>(
            "SELECT tenant_id
             FROM (
                 SELECT tenant_id FROM external_operations
                  WHERE state IN ('ready','retryable_failure','dispatching')
                 UNION
                 SELECT tenant_id FROM webhook_inbox
                  WHERE status IN ('verified','processing')
             ) work
             WHERE tenant_id>$1
             ORDER BY tenant_id
             LIMIT $2",
        )
        .bind(tenant_id)
        .bind(INTEGRATION_TENANT_PAGE_SIZE)
        .fetch_all(&mut **transaction)
        .await
        .map_err(persistence),
        None => sqlx::query_scalar::<_, String>(
            "SELECT tenant_id
             FROM (
                 SELECT tenant_id FROM external_operations
                  WHERE state IN ('ready','retryable_failure','dispatching')
                 UNION
                 SELECT tenant_id FROM webhook_inbox
                  WHERE status IN ('verified','processing')
             ) work
             ORDER BY tenant_id
             LIMIT $1",
        )
        .bind(INTEGRATION_TENANT_PAGE_SIZE)
        .fetch_all(&mut **transaction)
        .await
        .map_err(persistence),
    }
}

async fn startup_recovery_tenant_page(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    recovery_id: &str,
) -> Result<Vec<String>, IntegrationError> {
    sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT tenant_id
         FROM integration_startup_recovery_snapshot
         WHERE scheduler_id=$1 AND recovery_id=$2
         ORDER BY tenant_id
         LIMIT $3",
    )
    .bind(INTEGRATION_WORKER_SCHEDULER_ID)
    .bind(recovery_id)
    .bind(INTEGRATION_TENANT_PAGE_SIZE)
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence)
}

async fn insert_runtime_event(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    operation_id: &ExternalOperationId,
    event_type: &str,
    classification: &str,
    timestamp: &str,
) -> Result<(), IntegrationError> {
    sqlx::query(
        "INSERT INTO external_operation_runtime_events
            (id,tenant_id,operation_id,attempt_id,event_type,classification,occurred_at)
         VALUES ($1,$2,$3,NULL,$4,$5,$6)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(tenant_id)
    .bind(operation_id.as_str())
    .bind(event_type)
    .bind(classification)
    .bind(timestamp)
    .execute(&mut **transaction)
    .await
    .map_err(persistence)?;
    Ok(())
}

async fn serializable(pool: &PgPool) -> Result<Transaction<'_, sqlx::Postgres>, IntegrationError> {
    let mut transaction = pool.begin().await.map_err(persistence)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(persistence)?;
    Ok(transaction)
}

fn run_pg_scheduler<T, F>(future: F) -> Result<T, IntegrationError>
where
    T: Send,
    F: Future<Output = Result<T, IntegrationError>> + Send,
{
    let handle = Handle::try_current().map_err(persistence)?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(IntegrationError::Persistence);
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}
