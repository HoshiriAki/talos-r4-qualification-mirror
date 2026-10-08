use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::sync::Arc;
use system_core::ExecutionContext;

use crate::observability::{MetricsSink, NoopMetrics};
use crate::repositories::{
    RepositoryBinding, RepositoryError, RepositoryProvider, ScopedRepositories,
};

#[derive(Clone)]
pub struct SqliteRepositoryProvider {
    pool: Pool<SqliteConnectionManager>,
    metrics: Arc<dyn MetricsSink>,
}

impl SqliteRepositoryProvider {
    pub fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self::new_with_metrics(pool, Arc::new(NoopMetrics))
    }

    /// Production composition passes the shared bounded metrics sink here so
    /// transaction-owned R3 admission can emit only after commit. The default
    /// constructor remains intentionally silent for ordinary repository tests.
    pub(crate) fn new_with_metrics(
        pool: Pool<SqliteConnectionManager>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self { pool, metrics }
    }
}

impl RepositoryProvider for SqliteRepositoryProvider {
    fn bind(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, RepositoryError> {
        let binding = RepositoryBinding::from_execution(ctx)?;
        Ok(ScopedRepositories::sqlite(
            binding,
            self.pool.clone(),
            self.metrics.clone(),
        ))
    }
}
