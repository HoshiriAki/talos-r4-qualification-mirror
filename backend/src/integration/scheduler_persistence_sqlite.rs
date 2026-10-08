use super::scheduler_persistence_contract::{IntegrationSchedulerPersistence, StartupRecoveryPage};
use super::store::IntegrationStore;
use super::types::IntegrationError;

impl IntegrationSchedulerPersistence for IntegrationStore {
    fn begin_startup_recovery_snapshot(&self) -> Result<String, IntegrationError> {
        IntegrationStore::begin_startup_recovery_snapshot(self)
    }

    fn recover_next_startup_snapshot_page(
        &self,
        recovery_id: &str,
    ) -> Result<StartupRecoveryPage, IntegrationError> {
        IntegrationStore::recover_next_startup_snapshot_page(self, recovery_id)
    }

    fn next_tenant_work_page(&self) -> Result<Vec<String>, IntegrationError> {
        IntegrationStore::next_tenant_work_page(self)
    }
}
