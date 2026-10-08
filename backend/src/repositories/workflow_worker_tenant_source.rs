use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

#[cfg(feature = "postgres")]
use std::future::Future;
#[cfg(feature = "postgres")]
use tokio::runtime::{Handle, RuntimeFlavor};

#[derive(Clone)]
enum WorkflowWorkerTenantSourceBackend {
    Sqlite(Pool<SqliteConnectionManager>),
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
}

#[derive(Clone)]
pub(crate) struct WorkflowWorkerTenantSource {
    backend: WorkflowWorkerTenantSourceBackend,
}

impl WorkflowWorkerTenantSource {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: WorkflowWorkerTenantSourceBackend::Sqlite(pool),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: WorkflowWorkerTenantSourceBackend::Postgres(pool),
        }
    }

    pub(crate) fn discover_due_tenants(&self, limit: usize) -> Result<Vec<String>, String> {
        if !(1..=1_000).contains(&limit) {
            return Err("WORKFLOW_WORKER_TENANT_LIMIT_INVALID".into());
        }

        match &self.backend {
            WorkflowWorkerTenantSourceBackend::Sqlite(pool) => {
                let connection = pool.get().map_err(|error| error.to_string())?;
                let mut statement = connection
                    .prepare(
                        "SELECT tenant_id
                         FROM (
                           SELECT tenant_id
                           FROM workflow_instances
                           WHERE status='active'
                           UNION
                           SELECT tenant_id
                           FROM domain_outbox
                           WHERE state='pending'
                         )
                         ORDER BY tenant_id
                         LIMIT ?1",
                    )
                    .map_err(|error| error.to_string())?;
                statement
                    .query_map([limit as i64], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())
            }
            #[cfg(feature = "postgres")]
            WorkflowWorkerTenantSourceBackend::Postgres(pool) => {
                let pool = pool.clone();
                run_pg_worker_source(async move {
                    sqlx::query_scalar::<_, String>(
                        "SELECT tenant_id
                         FROM (
                           SELECT tenant_id
                           FROM workflow_instances
                           WHERE status='active'
                           UNION
                           SELECT tenant_id
                           FROM domain_outbox
                           WHERE state='pending'
                         ) AS due_tenants
                         ORDER BY tenant_id
                         LIMIT $1",
                    )
                    .bind(limit as i64)
                    .fetch_all(&pool)
                    .await
                    .map_err(|_| "WORKFLOW_WORKER_TENANT_SOURCE_UNAVAILABLE".to_string())
                })
            }
        }
    }
}

#[cfg(feature = "postgres")]
fn run_pg_worker_source<T, F>(future: F) -> Result<T, String>
where
    T: Send,
    F: Future<Output = Result<T, String>> + Send,
{
    let handle = Handle::try_current()
        .map_err(|_| "WORKFLOW_WORKER_POSTGRES_RUNTIME_UNAVAILABLE".to_string())?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err("WORKFLOW_WORKER_POSTGRES_RUNTIME_UNAVAILABLE".to_string());
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
