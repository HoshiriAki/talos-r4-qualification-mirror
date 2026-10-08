use std::sync::Arc;

use crate::registry::ModuleRegistry;
use crate::repositories::RepositoryProvider;

use super::{
    Clock, DashboardReadAuthorityService, DeviceCandidateQueryService,
    DeviceMutationAuthorityService, DeviceReadAuthorityService, ModelAuthorityService,
    ModuleClient, OrderQueryService, RegistryModuleClient, WarehouseAuthorityService, WorkerRunner,
};

pub struct ApplicationServices {
    clock: Arc<dyn Clock>,
    module_client: Arc<dyn ModuleClient>,
    worker_runner: Arc<dyn WorkerRunner>,
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl ApplicationServices {
    pub fn new(
        clock: Arc<dyn Clock>,
        module_client: Arc<dyn ModuleClient>,
        worker_runner: Arc<dyn WorkerRunner>,
        repository_provider: Arc<dyn RepositoryProvider>,
    ) -> Self {
        Self {
            clock,
            module_client,
            worker_runner,
            repository_provider,
        }
    }

    pub fn production(
        registry: Arc<ModuleRegistry>,
        clock: Arc<dyn Clock>,
        worker_runner: Arc<dyn WorkerRunner>,
        repository_provider: Arc<dyn RepositoryProvider>,
    ) -> Self {
        Self::new(
            clock,
            Arc::new(RegistryModuleClient::new(registry)),
            worker_runner,
            repository_provider,
        )
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    pub fn module_client(&self) -> Arc<dyn ModuleClient> {
        self.module_client.clone()
    }

    pub fn worker_runner(&self) -> Arc<dyn WorkerRunner> {
        self.worker_runner.clone()
    }

    pub fn repository_provider(&self) -> Arc<dyn RepositoryProvider> {
        self.repository_provider.clone()
    }

    pub fn dashboards(&self) -> DashboardReadAuthorityService {
        DashboardReadAuthorityService::new(self.repository_provider())
    }

    pub fn device_candidates(&self) -> DeviceCandidateQueryService {
        DeviceCandidateQueryService::new(self.repository_provider())
    }

    pub fn device_reads(&self) -> DeviceReadAuthorityService {
        DeviceReadAuthorityService::new(self.repository_provider())
    }

    pub fn device_mutations(&self) -> DeviceMutationAuthorityService {
        DeviceMutationAuthorityService::new(self.repository_provider())
    }

    pub fn order_queries(&self) -> OrderQueryService {
        OrderQueryService::new(self.repository_provider())
    }

    pub fn model_authority(&self) -> ModelAuthorityService {
        ModelAuthorityService::new(self.repository_provider())
    }

    pub fn warehouse_authority(&self) -> WarehouseAuthorityService {
        WarehouseAuthorityService::new(self.repository_provider())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use serde_json::{Value, json};
    use system_core::ExecutionContext;

    use crate::application::workers::WorkerRunReport;
    use crate::application::{Clock, FixedClock, ModuleClient, SystemClock, WorkerRunner};
    use crate::registry::ModuleRegistry;
    use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

    use super::ApplicationServices;

    struct ProbeClient;

    struct ProbeWorkerRunner;

    impl WorkerRunner for ProbeWorkerRunner {
        fn run_startup(&self) -> Vec<WorkerRunReport> {
            vec![]
        }

        fn run_periodic_tick(&self) -> Vec<WorkerRunReport> {
            vec![]
        }

        fn spawn_periodic(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async {})
        }
    }

    impl ModuleClient for ProbeClient {
        fn execute(
            &self,
            module_name: &str,
            command: &str,
            payload: Value,
            _ctx: &ExecutionContext,
        ) -> Result<Value, String> {
            Ok(json!({ "module": module_name, "command": command, "payload": payload }))
        }
    }

    fn assert_send_sync<T: Send + Sync>() {}

    fn repository_provider() -> Arc<dyn RepositoryProvider> {
        Arc::new(SqliteRepositoryProvider::new(
            Pool::builder()
                .max_size(1)
                .build(SqliteConnectionManager::memory())
                .unwrap(),
        ))
    }

    #[test]
    fn injected_services_return_cloned_handles_to_the_same_instances() {
        let instant = Utc.with_ymd_and_hms(2026, 8, 3, 1, 2, 3).unwrap();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(instant));
        let module_client: Arc<dyn ModuleClient> = Arc::new(ProbeClient);
        let worker_runner: Arc<dyn WorkerRunner> = Arc::new(ProbeWorkerRunner);
        let repository_provider = repository_provider();
        let services = ApplicationServices::new(
            clock.clone(),
            module_client.clone(),
            worker_runner.clone(),
            repository_provider.clone(),
        );

        let returned_clock = services.clock();
        let returned_client = services.module_client();
        assert!(Arc::ptr_eq(&clock, &returned_clock));
        assert!(Arc::ptr_eq(&module_client, &returned_client));
        assert!(Arc::ptr_eq(&worker_runner, &services.worker_runner()));
        assert!(Arc::ptr_eq(
            &repository_provider,
            &services.repository_provider()
        ));
        let _device_candidates = services.device_candidates();
        let _order_queries = services.order_queries();
        assert_eq!(returned_clock.now_utc(), instant);
        assert_send_sync::<ApplicationServices>();
    }

    #[test]
    fn production_uses_system_clock_and_registry_module_client() {
        let registry = Arc::new(ModuleRegistry::new(HashMap::new()).unwrap());
        let services = ApplicationServices::production(
            registry,
            Arc::new(SystemClock),
            Arc::new(ProbeWorkerRunner),
            repository_provider(),
        );
        let before = Utc::now();
        let actual = services.clock().now_utc();
        let after = Utc::now();

        assert!(actual >= before && actual <= after);
        // An empty production Registry preserves its own not-found error,
        // proving the application-facing port is the Registry adapter.
        let tenant_id = system_core::TenantId::new("tenant-services").unwrap();
        let ctx = ExecutionContext::new(
            system_core::ActorIdentity::authenticated("actor", "staff").unwrap(),
            system_core::TenantScope::tenant(tenant_id.clone()),
            system_core::DataScope::production(
                tenant_id,
                system_core::Revision::new("revision-services").unwrap(),
            )
            .unwrap(),
            system_core::ExecutionMode::Normal,
            system_core::RequestId::new("request-services").unwrap(),
            None,
            Arc::new(system_core::NoopHttpClient),
        )
        .unwrap();
        let error = services
            .module_client()
            .execute("missing", "read", json!({}), &ctx)
            .unwrap_err();
        assert!(error.contains("SYS_MODULE_NOT_FOUND"));
    }
}
