use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::transport::http_client::HttpClient;

use super::ModuleRegistry;
use super::audit_sink::{AuditSink, SqliteAuditSink};
use super::factory::ModuleFactory;
use super::provider_config::ProviderDeploymentConfig;
use crate::application::{
    AuditCompatibilityModule, ConsentCompatibilityModule, DeletionCompatibilityModule,
    ReportCompatibilityModule, TenantGovernanceCompatibilityModule,
    TenantMembershipCompatibilityModule, TenantPreviewCompatibilityModule,
    TenantSimulationCompatibilityModule, TwoFaCompatibilityModule, UserSettingsCompatibilityModule,
};
use crate::integration::{IntegrationModule, store::IntegrationStore};
use crate::observability::MetricsSink;
use crate::repositories::RepositoryProvider;

impl ModuleRegistry {
    /// Assemble the validated descriptor projection and publish the trusted
    /// Registry. The caller must supply the configured IntegrationStore so all
    /// integration entrypoints share one KeyStore admission policy.
    pub fn assemble(
        pool: Pool<SqliteConnectionManager>,
        http_client: Arc<dyn HttpClient>,
        integration_store: IntegrationStore,
    ) -> Result<Self, String> {
        let provider_config = ProviderDeploymentConfig;
        let built = ModuleFactory::new(pool.clone(), http_client, provider_config)
            .with_integration_store(integration_store)
            .with_tenant_simulation_module(TenantSimulationCompatibilityModule::with_pool(
                pool.clone(),
            ))
            .build()?;
        Self::new_with_audit_sink(built.into_modules(), Arc::new(SqliteAuditSink::new(pool)?))
    }

    pub(crate) fn assemble_with_metrics(
        pool: Pool<SqliteConnectionManager>,
        http_client: Arc<dyn HttpClient>,
        integration_store: IntegrationStore,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, String> {
        let audit_sink: Arc<dyn AuditSink> = Arc::new(SqliteAuditSink::new(pool.clone())?);
        Self::assemble_with_metrics_and_audit_sink(
            pool,
            http_client,
            integration_store,
            audit_sink,
            metrics,
        )
    }

    pub(crate) fn assemble_with_metrics_and_audit_sink(
        pool: Pool<SqliteConnectionManager>,
        http_client: Arc<dyn HttpClient>,
        integration_store: IntegrationStore,
        audit_sink: Arc<dyn AuditSink>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, String> {
        let integration_module = IntegrationModule::new(integration_store.clone());
        Self::assemble_with_metrics_audit_sink_and_integration_module(
            pool,
            http_client,
            integration_module,
            audit_sink,
            metrics,
        )
    }

    pub(crate) fn assemble_with_metrics_audit_sink_and_integration_module(
        pool: Pool<SqliteConnectionManager>,
        http_client: Arc<dyn HttpClient>,
        integration_module: IntegrationModule,
        audit_sink: Arc<dyn AuditSink>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, String> {
        let provider_config = ProviderDeploymentConfig;
        let tenant_simulation_module = TenantSimulationCompatibilityModule::with_pool(pool.clone());
        let built = ModuleFactory::new(pool, http_client, provider_config)
            .with_integration_module(integration_module)
            .with_tenant_simulation_module(tenant_simulation_module)
            .with_metrics(metrics.clone())
            .build()?;
        Self::new_with_audit_sink_and_metrics(built.into_modules(), audit_sink, metrics)
    }
    pub(crate) fn assemble_with_metrics_audit_sink_integration_and_staff_module(
        sqlite_pool: Option<Pool<SqliteConnectionManager>>,
        http_client: Arc<dyn HttpClient>,
        integration_module: IntegrationModule,
        audit_module: AuditCompatibilityModule,
        consent_module: ConsentCompatibilityModule,
        deletion_module: DeletionCompatibilityModule,
        report_module: ReportCompatibilityModule,
        two_fa_module: TwoFaCompatibilityModule,
        staff_module: TenantMembershipCompatibilityModule,
        user_settings_module: UserSettingsCompatibilityModule,
        tenant_governance_module: TenantGovernanceCompatibilityModule,
        tenant_preview_module: TenantPreviewCompatibilityModule,
        tenant_simulation_module: TenantSimulationCompatibilityModule,
        repository_provider: Arc<dyn RepositoryProvider>,
        audit_sink: Arc<dyn AuditSink>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, String> {
        let provider_config = ProviderDeploymentConfig;
        let factory = match sqlite_pool {
            Some(pool) => ModuleFactory::new(pool, http_client, provider_config),
            None => ModuleFactory::new_without_sqlite(http_client, provider_config),
        };
        let built = factory
            .with_integration_module(integration_module)
            .with_audit_module(audit_module)
            .with_consent_module(consent_module)
            .with_deletion_module(deletion_module)
            .with_report_module(report_module)
            .with_two_fa_module(two_fa_module)
            .with_staff_module(staff_module)
            .with_user_settings_module(user_settings_module)
            .with_tenant_governance_module(tenant_governance_module)
            .with_tenant_preview_module(tenant_preview_module)
            .with_tenant_simulation_module(tenant_simulation_module)
            .with_repository_provider(repository_provider)
            .with_metrics(metrics.clone())
            .build()?;
        Self::new_with_audit_sink_and_metrics(built.into_modules(), audit_sink, metrics)
    }
}
