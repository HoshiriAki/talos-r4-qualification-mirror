use std::sync::Arc;

use sqlx::PgPool;
use system_core::ExecutionContext;

use crate::observability::{MetricsSink, NoopMetrics};
use crate::repositories::{
    RepositoryBinding, RepositoryError, RepositoryProvider, ScopedRepositories,
};

#[derive(Clone)]
pub struct PostgresRepositoryProvider {
    pool: PgPool,
    metrics: Arc<dyn MetricsSink>,
}

impl PostgresRepositoryProvider {
    pub fn new(pool: PgPool) -> Self {
        Self::new_with_metrics(pool, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(pool: PgPool, metrics: Arc<dyn MetricsSink>) -> Self {
        Self { pool, metrics }
    }
}

impl RepositoryProvider for PostgresRepositoryProvider {
    fn bind(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, RepositoryError> {
        let binding = RepositoryBinding::from_execution(ctx)?;
        Ok(ScopedRepositories::postgres(
            binding,
            self.pool.clone(),
            self.metrics.clone(),
        ))
    }
}
