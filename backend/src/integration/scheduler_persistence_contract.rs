use super::types::IntegrationError;

pub(crate) const INTEGRATION_WORKER_SCHEDULER_ID: &str = "fixture_integration_worker";
pub(crate) const INTEGRATION_TENANT_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StartupRecoveryPage {
    pub tenants: u64,
    pub recovered_operations: u64,
    pub recovered_webhooks: u64,
}

pub(crate) trait IntegrationSchedulerPersistence: Send + Sync {
    fn begin_startup_recovery_snapshot(&self) -> Result<String, IntegrationError>;

    fn recover_next_startup_snapshot_page(
        &self,
        recovery_id: &str,
    ) -> Result<StartupRecoveryPage, IntegrationError>;

    fn next_tenant_work_page(&self) -> Result<Vec<String>, IntegrationError>;
}
