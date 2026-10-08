use feature_tenant_simulation::FeatureTenantSimulation;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::Value;
use system_core::{CommandMetadata, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

pub(crate) struct TenantSimulationCompatibilityModule {
    inner: FeatureTenantSimulation,
}

impl TenantSimulationCompatibilityModule {
    pub(crate) fn new() -> Self {
        Self {
            inner: FeatureTenantSimulation::new(),
        }
    }

    pub(crate) fn with_pool(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            inner: FeatureTenantSimulation::with_pool(pool),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn with_postgres(pool: sqlx::PgPool) -> Self {
        Self {
            inner: FeatureTenantSimulation::with_postgres(pool),
        }
    }
}

impl SystemModule for TenantSimulationCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        self.inner.metadata()
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        self.inner.commands()
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        self.inner.init(config)
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        self.inner.execute(command, payload, ctx)
    }

    fn schema(&self) -> ModuleSchema {
        self.inner.schema()
    }
}
