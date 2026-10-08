use std::collections::HashMap;
use std::sync::Arc;

use official_logistics::{FeatureLogistics, FeatureWarehouseRouting};
use official_warehouse::FeatureWarehouseAdvanced;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::SystemModule;
use system_core::transport::http_client::HttpClient;
// These type-only imports keep the descriptor/factory projection complete for
// deferred provider crates. No legacy provider value is constructed here.
use thirdparty_alipay::FeatureAlipay;
use thirdparty_miniapp::FeatureMiniapp;
use thirdparty_sf_express::FeatureSfExpress;
use thirdparty_wechat_pay::FeatureWechatPay;

use crate::application::{
    AuditCompatibilityModule, BarcodeCompatibilityModule, BookingCompatibilityModule,
    ConsentCompatibilityModule, ContractCompatibilityModule, CreditCompatibilityModule,
    CustomerModule, DamageCompatibilityModule, DeletionCompatibilityModule,
    DepositCompatibilityModule, DepreciationCompatibilityModule, DeviceCompatibilityModule,
    ExcelImportCompatibilityModule, InvoiceCompatibilityModule, ModelCompatibilityModule,
    NotifyCompatibilityModule, OpticalSopCompatibilityModule, OrderLifecycleCompatibilityModule,
    OrderLifecycleV2Module, OrderQueryV2Module, OrderReadCompatibilityModule,
    OverdueCompatibilityModule, PricingCompatibilityModule, ProcurementCompatibilityModule,
    QuoteModule, R3SettlementModule, RefundCompatibilityModule, RepairCompatibilityModule,
    ReportCompatibilityModule, ReservationCompatibilityModule, ReservationV2Module,
    RoaCompatibilityModule, SettlementCompatibilityModule, TaxCompatibilityModule,
    TenantGovernanceCompatibilityModule, TenantMembershipCompatibilityModule,
    TenantPreviewCompatibilityModule, TenantSimulationCompatibilityModule,
    TwoFaCompatibilityModule, UserSettingsCompatibilityModule,
    WarehouseAdvancedCompatibilityModule, WarehouseCompatibilityModule,
    WorkTaskCompatibilityModule,
};
use crate::integration::{IntegrationModule, store::IntegrationStore};
use crate::observability::{MetricsSink, NoopMetrics};
use crate::repositories::{
    AuditCompatibilityRepository, AuthSecurityRepository, ConsentCompatibilityRepository,
    DeletionCompatibilityRepository, ReportCompatibilityRepository, RepositoryProvider,
    SqliteRepositoryProvider, TenantMembershipAuthorityRepository, UserSettingsProfileRepository,
};

use super::descriptors::{ModuleDescriptor, ModuleFactoryId, module_descriptors};
use super::provider_config::ProviderDeploymentConfig;
use super::validation::{
    validate_build_order, validate_descriptor_catalog, validate_descriptor_projection,
};

pub struct BuiltModuleSet {
    modules: HashMap<String, Arc<dyn SystemModule>>,
    descriptors: Vec<ModuleDescriptor>,
    build_order: Vec<&'static str>,
    construction_receipts: Vec<ConstructionReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstructionReceipt {
    pub factory: ModuleFactoryId,
    pub registry_key: &'static str,
}

struct ConstructedModule {
    receipt: ConstructionReceipt,
    module: Arc<dyn SystemModule>,
}

trait ConstructionIdentity {
    const FACTORY: ModuleFactoryId;
    const REGISTRY_KEY: &'static str;
}

macro_rules! construction_identities {
    ($(($module:ty, $factory:ident, $key:literal)),+ $(,)?) => {
        $(
            impl ConstructionIdentity for $module {
                const FACTORY: ModuleFactoryId = ModuleFactoryId::$factory;
                const REGISTRY_KEY: &'static str = $key;
            }
        )+
    };
}

construction_identities!(
    (AuditCompatibilityModule, Audit, "audit"),
    (DamageCompatibilityModule, Damage, "damage"),
    (
        DepreciationCompatibilityModule,
        Depreciation,
        "depreciation"
    ),
    (DepositCompatibilityModule, Deposit, "deposit"),
    (DeviceCompatibilityModule, Device, "device"),
    (InvoiceCompatibilityModule, Invoice, "invoice"),
    (IntegrationModule, Integration, "integration"),
    (FeatureLogistics, Logistics, "logistics"),
    (ModelCompatibilityModule, Model, "model"),
    (ProcurementCompatibilityModule, Procurement, "procurement"),
    (RefundCompatibilityModule, Refund, "refund"),
    (ReportCompatibilityModule, Report, "report"),
    (RoaCompatibilityModule, Roa, "roa"),
    (SettlementCompatibilityModule, Settlement, "settlement"),
    (TenantMembershipCompatibilityModule, Staff, "staff"),
    (TaxCompatibilityModule, Tax, "tax"),
    (
        UserSettingsCompatibilityModule,
        UserSettings,
        "user_settings"
    ),
    (WorkTaskCompatibilityModule, WorkTask, "work_task"),
    (NotifyCompatibilityModule, Notify, "notify"),
    (WarehouseCompatibilityModule, Warehouse, "warehouse"),
    (
        FeatureWarehouseRouting,
        WarehouseRouting,
        "warehouse_routing"
    ),
    (ConsentCompatibilityModule, Consent, "consent"),
    (ContractCompatibilityModule, Contract, "contract"),
    (CustomerModule, Customer, "customer"),
    (QuoteModule, Quote, "quote"),
    (DeletionCompatibilityModule, Deletion, "deletion"),
    (TwoFaCompatibilityModule, TwoFa, "two_fa"),
    (BarcodeCompatibilityModule, Barcode, "barcode"),
    (BookingCompatibilityModule, Booking, "booking"),
    (CreditCompatibilityModule, Credit, "credit"),
    (OverdueCompatibilityModule, Overdue, "overdue"),
    (OpticalSopCompatibilityModule, OpticalSop, "optical_sop"),
    (ReservationCompatibilityModule, Reservation, "reservation"),
    (ReservationV2Module, ReservationV2, "reservation_v2"),
    (R3SettlementModule, R3Settlement, "r3_settlement"),
    (
        TenantGovernanceCompatibilityModule,
        TenantGovernance,
        "tenant_governance"
    ),
    (
        TenantPreviewCompatibilityModule,
        TenantPreview,
        "tenant_preview"
    ),
    (
        TenantSimulationCompatibilityModule,
        TenantSimulation,
        "tenant_simulation"
    ),
    (PricingCompatibilityModule, Pricing, "pricing"),
    (ExcelImportCompatibilityModule, ExcelImport, "excel_import"),
    (
        FeatureWarehouseAdvanced,
        WarehouseAdvanced,
        "warehouse_advanced"
    ),
    (
        WarehouseAdvancedCompatibilityModule,
        WarehouseAdvanced,
        "warehouse_advanced"
    ),
    (OrderLifecycleCompatibilityModule, Order, "order"),
    (
        OrderLifecycleV2Module,
        OrderLifecycleV2,
        "order_lifecycle_v2"
    ),
    (OrderQueryV2Module, OrderQueryV2, "order_query_v2"),
    (
        OrderReadCompatibilityModule,
        OrderReadCompatibility,
        "order_read_compatibility"
    ),
    (RepairCompatibilityModule, Repair, "repair"),
    (FeatureSfExpress, SfExpress, "sf_express"),
    (FeatureWechatPay, WechatPay, "wechat_pay"),
    (FeatureAlipay, Alipay, "alipay"),
    (FeatureMiniapp, Miniapp, "miniapp"),
);

fn constructed<T>(module: T) -> ConstructedModule
where
    T: SystemModule + ConstructionIdentity + 'static,
{
    ConstructedModule {
        receipt: ConstructionReceipt {
            factory: T::FACTORY,
            registry_key: T::REGISTRY_KEY,
        },
        module: Arc::new(module),
    }
}

impl BuiltModuleSet {
    pub(crate) fn into_modules(self) -> HashMap<String, Arc<dyn SystemModule>> {
        self.modules
    }

    #[cfg(test)]
    fn module_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.modules.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

pub struct ModuleFactory {
    pool: Option<Pool<SqliteConnectionManager>>,
    integration_store: Option<IntegrationStore>,
    integration_module: Option<IntegrationModule>,
    audit_module: Option<AuditCompatibilityModule>,
    consent_module: Option<ConsentCompatibilityModule>,
    deletion_module: Option<DeletionCompatibilityModule>,
    report_module: Option<ReportCompatibilityModule>,
    two_fa_module: Option<TwoFaCompatibilityModule>,
    staff_module: Option<TenantMembershipCompatibilityModule>,
    user_settings_module: Option<UserSettingsCompatibilityModule>,
    tenant_governance_module: Option<TenantGovernanceCompatibilityModule>,
    tenant_preview_module: Option<TenantPreviewCompatibilityModule>,
    tenant_simulation_module: Option<TenantSimulationCompatibilityModule>,
    repository_provider: Option<Arc<dyn RepositoryProvider>>,
    _http_client: Arc<dyn HttpClient>,
    provider_config: ProviderDeploymentConfig,
    descriptors: &'static [ModuleDescriptor],
    metrics: Arc<dyn MetricsSink>,
}

impl ModuleFactory {
    pub fn new(
        pool: Pool<SqliteConnectionManager>,
        http_client: Arc<dyn HttpClient>,
        provider_config: ProviderDeploymentConfig,
    ) -> Self {
        Self {
            integration_store: None,
            integration_module: None,
            audit_module: None,
            consent_module: None,
            deletion_module: None,
            report_module: None,
            two_fa_module: None,
            staff_module: None,
            user_settings_module: None,
            tenant_governance_module: None,
            tenant_preview_module: None,
            tenant_simulation_module: None,
            repository_provider: None,
            pool: Some(pool),
            _http_client: http_client,
            provider_config,
            descriptors: module_descriptors(),
            metrics: Arc::new(NoopMetrics),
        }
    }

    /// PostgreSQL production composition must not manufacture a SQLite pool
    /// merely to satisfy Registry construction. Every authority that historically
    /// used the local pool must be injected explicitly before build().
    pub(crate) fn new_without_sqlite(
        http_client: Arc<dyn HttpClient>,
        provider_config: ProviderDeploymentConfig,
    ) -> Self {
        Self {
            integration_store: None,
            integration_module: None,
            audit_module: None,
            consent_module: None,
            deletion_module: None,
            report_module: None,
            two_fa_module: None,
            staff_module: None,
            user_settings_module: None,
            tenant_governance_module: None,
            tenant_preview_module: None,
            tenant_simulation_module: None,
            repository_provider: None,
            pool: None,
            _http_client: http_client,
            provider_config,
            descriptors: module_descriptors(),
            metrics: Arc::new(NoopMetrics),
        }
    }

    fn require_sqlite_pool(
        &self,
        consumer: &'static str,
    ) -> Result<Pool<SqliteConnectionManager>, String> {
        self.pool.clone().ok_or_else(|| {
            format!(
                "CFG_SQLITE_CAPABILITY_REQUIRED: {consumer} requires explicit injection when ModuleFactory is constructed without SQLite"
            )
        })
    }

    /// Legacy/local composition can inject its SQLite-backed IntegrationStore.
    /// Production PostgreSQL composition injects IntegrationModule directly and
    /// never needs to construct this compatibility store.
    pub fn with_integration_store(mut self, integration_store: IntegrationStore) -> Self {
        self.integration_store = Some(integration_store);
        self
    }

    /// Production composition can inject an IntegrationModule whose persistence
    /// authorities were selected independently of the compatibility SQLite store.
    pub(crate) fn with_integration_module(mut self, integration_module: IntegrationModule) -> Self {
        self.integration_module = Some(integration_module);
        self
    }

    pub(crate) fn with_audit_module(mut self, audit_module: AuditCompatibilityModule) -> Self {
        self.audit_module = Some(audit_module);
        self
    }

    pub(crate) fn with_consent_module(
        mut self,
        consent_module: ConsentCompatibilityModule,
    ) -> Self {
        self.consent_module = Some(consent_module);
        self
    }

    pub(crate) fn with_deletion_module(
        mut self,
        deletion_module: DeletionCompatibilityModule,
    ) -> Self {
        self.deletion_module = Some(deletion_module);
        self
    }

    pub(crate) fn with_report_module(mut self, report_module: ReportCompatibilityModule) -> Self {
        self.report_module = Some(report_module);
        self
    }

    pub(crate) fn with_two_fa_module(mut self, two_fa_module: TwoFaCompatibilityModule) -> Self {
        self.two_fa_module = Some(two_fa_module);
        self
    }

    pub(crate) fn with_staff_module(
        mut self,
        staff_module: TenantMembershipCompatibilityModule,
    ) -> Self {
        self.staff_module = Some(staff_module);
        self
    }

    pub(crate) fn with_user_settings_module(
        mut self,
        user_settings_module: UserSettingsCompatibilityModule,
    ) -> Self {
        self.user_settings_module = Some(user_settings_module);
        self
    }

    pub(crate) fn with_tenant_governance_module(
        mut self,
        tenant_governance_module: TenantGovernanceCompatibilityModule,
    ) -> Self {
        self.tenant_governance_module = Some(tenant_governance_module);
        self
    }

    pub(crate) fn with_tenant_preview_module(
        mut self,
        tenant_preview_module: TenantPreviewCompatibilityModule,
    ) -> Self {
        self.tenant_preview_module = Some(tenant_preview_module);
        self
    }

    pub(crate) fn with_tenant_simulation_module(
        mut self,
        tenant_simulation_module: TenantSimulationCompatibilityModule,
    ) -> Self {
        self.tenant_simulation_module = Some(tenant_simulation_module);
        self
    }

    /// Production composition injects the backend-selected scoped repository
    /// provider. Legacy/local assembly leaves this empty and receives a lazy
    /// SQLite compatibility provider inside build().
    pub(crate) fn with_repository_provider(
        mut self,
        repository_provider: Arc<dyn RepositoryProvider>,
    ) -> Self {
        self.repository_provider = Some(repository_provider);
        self
    }

    /// Keep transaction-owned business admission on the process-wide bounded
    /// sink without introducing an ambient/global metrics dependency.
    pub(crate) fn with_metrics(mut self, metrics: Arc<dyn MetricsSink>) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn build(self) -> Result<BuiltModuleSet, String> {
        validate_descriptor_catalog(self.descriptors)?;

        let audit_built = match self.audit_module.clone() {
            Some(module) => constructed(module),
            None => constructed(AuditCompatibilityModule::new(
                AuditCompatibilityRepository::new(self.require_sqlite_pool("audit compatibility")?),
            )),
        };
        let report_built = match self.report_module.clone() {
            Some(module) => constructed(module),
            None => constructed(ReportCompatibilityModule::new(
                ReportCompatibilityRepository::new(
                    self.require_sqlite_pool("report compatibility")?,
                ),
            )),
        };
        let logistics_concrete = FeatureLogistics;
        let warehouse_routing_concrete = FeatureWarehouseRouting;
        let user_settings_concrete = match self.user_settings_module.clone() {
            Some(module) => module,
            None => UserSettingsCompatibilityModule::new(UserSettingsProfileRepository::new(
                self.require_sqlite_pool("user settings compatibility")?,
            )),
        };
        let repository_provider: Arc<dyn RepositoryProvider> =
            match self.repository_provider.clone() {
                Some(provider) => provider,
                None => Arc::new(SqliteRepositoryProvider::new(
                    self.require_sqlite_pool("repository provider")?,
                )),
            };
        let device_concrete = DeviceCompatibilityModule::new(repository_provider.clone());
        let model_concrete = ModelCompatibilityModule::new(repository_provider.clone());
        let warehouse_concrete = WarehouseCompatibilityModule::new(repository_provider.clone());
        // Production injects the process-wide metrics-aware provider selected
        // by main. Legacy/local composition keeps the historical dedicated R3
        // metrics-aware SQLite fallback.
        let r3_repository_provider: Arc<dyn RepositoryProvider> =
            match self.repository_provider.clone() {
                Some(provider) => provider,
                None => Arc::new(SqliteRepositoryProvider::new_with_metrics(
                    self.require_sqlite_pool("R3 settlement repository provider")?,
                    self.metrics.clone(),
                )),
            };
        let customer_concrete = CustomerModule::new(repository_provider.clone());
        let order_lifecycle_v2_concrete = OrderLifecycleV2Module::new(repository_provider.clone());
        let order_query_v2_concrete = OrderQueryV2Module::new(repository_provider.clone());
        let order_read_compatibility_concrete =
            OrderReadCompatibilityModule::new(repository_provider.clone());
        let work_task_compatibility_concrete =
            WorkTaskCompatibilityModule::new(repository_provider.clone());
        let staff_built = match self.staff_module.clone() {
            Some(module) => constructed(module),
            None => constructed(TenantMembershipCompatibilityModule::new(
                TenantMembershipAuthorityRepository::new(
                    self.require_sqlite_pool("staff membership compatibility")?,
                ),
            )),
        };
        let excel_import_concrete =
            ExcelImportCompatibilityModule::new(repository_provider.clone());
        let warehouse_advanced_concrete =
            WarehouseAdvancedCompatibilityModule::new(repository_provider.clone());
        let deposit_concrete = DepositCompatibilityModule::new(repository_provider.clone());
        let refund_concrete = RefundCompatibilityModule::new(repository_provider.clone());
        let damage_concrete = DamageCompatibilityModule::new(repository_provider.clone());
        let repair_concrete = RepairCompatibilityModule::new(repository_provider.clone());
        let notify_concrete = NotifyCompatibilityModule::new(repository_provider.clone());
        let procurement_concrete = ProcurementCompatibilityModule::new(repository_provider.clone());
        let depreciation_concrete =
            DepreciationCompatibilityModule::new(repository_provider.clone());
        let roa_concrete = RoaCompatibilityModule::new(repository_provider.clone());
        let invoice_concrete = InvoiceCompatibilityModule::new(repository_provider.clone());
        let integration_concrete = match self.integration_module.clone() {
            Some(module) => module,
            None => match self.integration_store.clone() {
                Some(store) => IntegrationModule::new(store),
                None => IntegrationModule::new(IntegrationStore::new(
                    self.require_sqlite_pool("integration compatibility store")?,
                )),
            },
        };
        let consent_built = match self.consent_module.clone() {
            Some(module) => constructed(module),
            None => constructed(ConsentCompatibilityModule::new(
                ConsentCompatibilityRepository::new(
                    self.require_sqlite_pool("consent compatibility")?,
                ),
            )),
        };
        let deletion_built = match self.deletion_module.clone() {
            Some(module) => constructed(module),
            None => constructed(DeletionCompatibilityModule::new(
                DeletionCompatibilityRepository::new(
                    self.require_sqlite_pool("deletion compatibility")?,
                ),
            )),
        };
        let two_fa_built = match self.two_fa_module.clone() {
            Some(module) => constructed(module),
            None => constructed(TwoFaCompatibilityModule::new(AuthSecurityRepository::new(
                self.require_sqlite_pool("two-factor compatibility")?,
            ))),
        };
        let barcode_concrete = BarcodeCompatibilityModule::new(repository_provider.clone());
        let credit_concrete = CreditCompatibilityModule::new(repository_provider.clone());
        let overdue_concrete = OverdueCompatibilityModule::new(repository_provider.clone());
        let contract_concrete = ContractCompatibilityModule::new(repository_provider.clone());
        let optical_sop_concrete = OpticalSopCompatibilityModule::new(repository_provider.clone());
        let booking_concrete = BookingCompatibilityModule::new(repository_provider.clone());
        let reservation_concrete = ReservationCompatibilityModule::new(repository_provider.clone());
        let reservation_v2_concrete = ReservationV2Module::new(repository_provider.clone());
        let r3_settlement_concrete = R3SettlementModule::new(r3_repository_provider);
        let settlement_concrete = SettlementCompatibilityModule::new(repository_provider.clone());
        let tax_concrete = TaxCompatibilityModule::new(repository_provider.clone());
        let tenant_governance_concrete = match self.tenant_governance_module.clone() {
            Some(module) => module,
            None => TenantGovernanceCompatibilityModule::new(
                crate::repositories::TenantGovernanceRepository::new(
                    self.require_sqlite_pool("tenant governance compatibility")?,
                ),
            ),
        };
        let tenant_preview_concrete = match self.tenant_preview_module.clone() {
            Some(module) => module,
            None => TenantPreviewCompatibilityModule::new(
                crate::repositories::TenantPreviewRepository::new(
                    self.require_sqlite_pool("tenant preview compatibility")?,
                ),
            ),
        };
        let tenant_simulation_concrete = self
            .tenant_simulation_module
            .unwrap_or_else(TenantSimulationCompatibilityModule::new);

        let device_built = constructed(device_concrete);
        let warehouse_built = constructed(warehouse_concrete);
        let model_built = constructed(model_concrete);
        let logistics_built = constructed(logistics_concrete);
        let warehouse_routing_built = constructed(warehouse_routing_concrete);
        let user_settings_built = constructed(user_settings_concrete);
        let deposit_built = constructed(deposit_concrete);

        let handles = BuiltModuleHandles {
            device: device_built.module.clone(),
            warehouse: warehouse_built.module.clone(),
            logistics: logistics_built.module.clone(),
            warehouse_routing: warehouse_routing_built.module.clone(),
        };

        let pricing_built = constructed(PricingCompatibilityModule::new(
            repository_provider.clone(),
            Some(handles.logistics.clone()),
            Some(handles.warehouse_routing.clone()),
        ));
        let pricing = pricing_built.module.clone();
        let quote_built = constructed(QuoteModule::new(
            repository_provider.clone(),
            pricing.clone(),
        ));

        let excel_import_built = constructed(excel_import_concrete);
        let warehouse_advanced_built = constructed(warehouse_advanced_concrete);

        let order_built = constructed(OrderLifecycleCompatibilityModule::new(
            repository_provider.clone(),
        ));
        let customer_built = constructed(customer_concrete);
        let order_lifecycle_v2_built = constructed(order_lifecycle_v2_concrete);
        let order_query_v2_built = constructed(order_query_v2_concrete);
        let order_read_compatibility_built = constructed(order_read_compatibility_concrete);
        let work_task_compatibility_built = constructed(work_task_compatibility_concrete);
        let reservation_v2_built = constructed(reservation_v2_concrete);
        let r3_settlement_built = constructed(r3_settlement_concrete);

        let repair_built = constructed(repair_concrete);

        let mut modules = HashMap::new();
        let mut build_order = Vec::new();
        let mut construction_receipts = Vec::new();
        let mut insert = |constructed: ConstructedModule| {
            build_order.push(constructed.receipt.registry_key);
            construction_receipts.push(constructed.receipt);
            modules.insert(
                constructed.receipt.registry_key.to_string(),
                constructed.module,
            );
        };

        insert(audit_built);
        insert(constructed(damage_concrete));
        insert(constructed(depreciation_concrete));
        insert(deposit_built);
        insert(device_built);
        insert(constructed(invoice_concrete));
        insert(constructed(integration_concrete));
        insert(logistics_built);
        insert(model_built);
        insert(constructed(procurement_concrete));
        insert(constructed(refund_concrete));
        insert(report_built);
        insert(constructed(roa_concrete));
        insert(constructed(settlement_concrete));
        insert(staff_built);
        insert(constructed(tax_concrete));
        insert(user_settings_built);
        insert(work_task_compatibility_built);
        insert(constructed(notify_concrete));
        insert(warehouse_built);
        insert(warehouse_routing_built);
        insert(consent_built);
        insert(constructed(contract_concrete));
        insert(customer_built);
        insert(deletion_built);
        insert(two_fa_built);
        insert(constructed(barcode_concrete));
        insert(constructed(credit_concrete));
        insert(constructed(overdue_concrete));
        insert(constructed(optical_sop_concrete));
        insert(constructed(booking_concrete));
        insert(constructed(reservation_concrete));
        insert(reservation_v2_built);
        insert(r3_settlement_built);
        insert(constructed(tenant_governance_concrete));
        insert(constructed(tenant_preview_concrete));
        insert(constructed(tenant_simulation_concrete));
        insert(pricing_built);
        insert(quote_built);
        insert(excel_import_built);
        insert(warehouse_advanced_built);
        insert(order_built);
        insert(order_lifecycle_v2_built);
        insert(order_query_v2_built);
        insert(order_read_compatibility_built);
        insert(repair_built);

        drop(insert);

        let descriptors = self.descriptors.to_vec();
        validate_build_order(&descriptors, &build_order)?;
        validate_descriptor_projection(
            &descriptors,
            &modules,
            &build_order,
            &construction_receipts,
            &self.provider_config,
        )?;

        Ok(BuiltModuleSet {
            modules,
            descriptors,
            build_order,
            construction_receipts,
        })
    }
}

struct BuiltModuleHandles {
    device: Arc<dyn SystemModule>,
    warehouse: Arc<dyn SystemModule>,
    logistics: Arc<dyn SystemModule>,
    warehouse_routing: Arc<dyn SystemModule>,
}

#[cfg(test)]
mod tests {
    use super::ModuleFactory;
    use crate::registry::provider_config::ProviderDeploymentConfig;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;
    use system_core::NoopHttpClient;

    const BASELINE_NON_PROVIDER_MODULES: &[&str] = &[
        "audit",
        "barcode",
        "booking",
        "consent",
        "contract",
        "credit",
        "customer",
        "damage",
        "deletion",
        "deposit",
        "depreciation",
        "device",
        "excel_import",
        "integration",
        "invoice",
        "logistics",
        "model",
        "notify",
        "optical_sop",
        "order",
        "order_lifecycle_v2",
        "order_query_v2",
        "order_read_compatibility",
        "overdue",
        "pricing",
        "procurement",
        "quote",
        "r3_settlement",
        "refund",
        "repair",
        "report",
        "reservation",
        "reservation_v2",
        "roa",
        "settlement",
        "staff",
        "tax",
        "tenant_governance",
        "tenant_preview",
        "tenant_simulation",
        "two_fa",
        "user_settings",
        "warehouse",
        "warehouse_advanced",
        "warehouse_routing",
        "work_task",
    ];

    fn pool() -> Pool<SqliteConnectionManager> {
        Pool::new(SqliteConnectionManager::memory()).unwrap()
    }
    fn build(config: ProviderDeploymentConfig) -> super::BuiltModuleSet {
        ModuleFactory::new(pool(), Arc::new(NoopHttpClient), config)
            .build()
            .unwrap()
    }

    #[test]
    fn factory_does_not_eagerly_construct_legacy_repository_provider() {
        let factory = ModuleFactory::new(
            pool(),
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        );
        assert!(factory.repository_provider.is_none());
    }

    #[test]
    fn factory_retains_explicit_repository_provider_injection() {
        use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

        let pool = pool();
        let repository_provider: Arc<dyn RepositoryProvider> =
            Arc::new(SqliteRepositoryProvider::new(pool.clone()));
        let factory = ModuleFactory::new(
            pool,
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_repository_provider(repository_provider);
        assert!(factory.repository_provider.is_some());
    }

    #[test]
    fn factory_does_not_eagerly_construct_legacy_integration_store() {
        let factory = ModuleFactory::new(
            pool(),
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        );
        assert!(factory.integration_store.is_none());
    }

    #[test]
    fn disabled_provider_build_matches_the_exact_baseline_module_set() {
        let built = build(ProviderDeploymentConfig::default());
        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.descriptors.len(), 50);
        assert_eq!(built.build_order.len(), 46);
        assert_eq!(built.construction_receipts.len(), 46);
    }

    #[test]
    fn legacy_provider_modules_are_not_built_even_when_the_factory_is_constructed() {
        let built = build(ProviderDeploymentConfig);
        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.construction_receipts.len(), 46);
    }

    #[test]
    fn injected_consent_compatibility_preserves_factory_identity_and_module_set() {
        use crate::application::ConsentCompatibilityModule;
        use crate::repositories::ConsentCompatibilityRepository;

        let pool = pool();
        let consent =
            ConsentCompatibilityModule::new(ConsentCompatibilityRepository::new(pool.clone()));
        let built = ModuleFactory::new(
            pool,
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_consent_module(consent)
        .build()
        .unwrap();

        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        let consent_receipts = built
            .construction_receipts
            .iter()
            .filter(|receipt| receipt.registry_key == "consent")
            .collect::<Vec<_>>();
        assert_eq!(consent_receipts.len(), 1);
        assert_eq!(consent_receipts[0].factory, super::ModuleFactoryId::Consent);
    }

    #[test]
    fn injected_audit_compatibility_preserves_factory_identity_and_module_set() {
        use crate::application::AuditCompatibilityModule;
        use crate::repositories::AuditCompatibilityRepository;

        let pool = pool();
        let audit = AuditCompatibilityModule::new(AuditCompatibilityRepository::new(pool.clone()));
        let built = ModuleFactory::new(
            pool,
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_audit_module(audit)
        .build()
        .unwrap();

        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.construction_receipts.len(), 46);
        let audit_receipts = built
            .construction_receipts
            .iter()
            .filter(|receipt| receipt.registry_key == "audit")
            .collect::<Vec<_>>();
        assert_eq!(audit_receipts.len(), 1);
        assert_eq!(audit_receipts[0].factory, super::ModuleFactoryId::Audit);
    }

    #[test]
    fn injected_deletion_compatibility_preserves_factory_identity_and_module_set() {
        use crate::application::DeletionCompatibilityModule;
        use crate::repositories::DeletionCompatibilityRepository;

        let pool = pool();
        let deletion =
            DeletionCompatibilityModule::new(DeletionCompatibilityRepository::new(pool.clone()));
        let built = ModuleFactory::new(
            pool,
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_deletion_module(deletion)
        .build()
        .unwrap();

        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.construction_receipts.len(), 46);
        let receipts = built
            .construction_receipts
            .iter()
            .filter(|receipt| receipt.registry_key == "deletion")
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].factory, super::ModuleFactoryId::Deletion);
    }

    #[test]
    fn injected_two_fa_compatibility_preserves_factory_identity_and_module_set() {
        use crate::application::TwoFaCompatibilityModule;
        use crate::repositories::AuthSecurityRepository;

        let pool = pool();
        let two_fa = TwoFaCompatibilityModule::new(AuthSecurityRepository::new(pool.clone()));
        let built = ModuleFactory::new(
            pool,
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_two_fa_module(two_fa)
        .build()
        .unwrap();

        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.construction_receipts.len(), 46);
        let receipts = built
            .construction_receipts
            .iter()
            .filter(|receipt| receipt.registry_key == "two_fa")
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].factory, super::ModuleFactoryId::TwoFa);
    }

    #[test]
    fn injected_tenant_membership_staff_preserves_factory_identity_and_module_set() {
        use crate::application::TenantMembershipCompatibilityModule;
        use crate::repositories::TenantMembershipAuthorityRepository;

        let pool = pool();
        let staff = TenantMembershipCompatibilityModule::new(
            TenantMembershipAuthorityRepository::new(pool.clone()),
        );
        let built = ModuleFactory::new(
            pool,
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_staff_module(staff)
        .build()
        .unwrap();

        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.construction_receipts.len(), 46);
        let staff_receipts = built
            .construction_receipts
            .iter()
            .filter(|receipt| receipt.registry_key == "staff")
            .collect::<Vec<_>>();
        assert_eq!(staff_receipts.len(), 1);
        assert_eq!(staff_receipts[0].factory, super::ModuleFactoryId::Staff);
    }

    #[test]
    fn sqlite_free_factory_fails_closed_when_legacy_authority_is_not_injected() {
        let result = ModuleFactory::new_without_sqlite(
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .build();
        let error = match result {
            Ok(_) => panic!("sqlite-free factory unexpectedly synthesized a legacy fallback"),
            Err(error) => error,
        };
        assert!(error.contains("CFG_SQLITE_CAPABILITY_REQUIRED"));
        assert!(error.contains("audit compatibility"));
    }

    #[test]
    fn sqlite_free_factory_builds_with_explicit_authorities() {
        use crate::application::{
            AuditCompatibilityModule, ConsentCompatibilityModule, DeletionCompatibilityModule,
            ReportCompatibilityModule, TenantGovernanceCompatibilityModule,
            TenantMembershipCompatibilityModule, TenantPreviewCompatibilityModule,
            TwoFaCompatibilityModule, UserSettingsCompatibilityModule,
        };
        use crate::integration::{IntegrationModule, store::IntegrationStore};
        use crate::repositories::{
            AuditCompatibilityRepository, AuthSecurityRepository, ConsentCompatibilityRepository,
            DeletionCompatibilityRepository, ReportCompatibilityRepository, RepositoryProvider,
            SqliteRepositoryProvider, TenantGovernanceRepository,
            TenantMembershipAuthorityRepository, TenantPreviewRepository,
            UserSettingsProfileRepository,
        };

        let backing_pool = pool();
        let repository_provider: Arc<dyn RepositoryProvider> =
            Arc::new(SqliteRepositoryProvider::new(backing_pool.clone()));

        let built = ModuleFactory::new_without_sqlite(
            Arc::new(NoopHttpClient),
            ProviderDeploymentConfig::default(),
        )
        .with_integration_module(IntegrationModule::new(IntegrationStore::new(
            backing_pool.clone(),
        )))
        .with_audit_module(AuditCompatibilityModule::new(
            AuditCompatibilityRepository::new(backing_pool.clone()),
        ))
        .with_consent_module(ConsentCompatibilityModule::new(
            ConsentCompatibilityRepository::new(backing_pool.clone()),
        ))
        .with_deletion_module(DeletionCompatibilityModule::new(
            DeletionCompatibilityRepository::new(backing_pool.clone()),
        ))
        .with_report_module(ReportCompatibilityModule::new(
            ReportCompatibilityRepository::new(backing_pool.clone()),
        ))
        .with_two_fa_module(TwoFaCompatibilityModule::new(AuthSecurityRepository::new(
            backing_pool.clone(),
        )))
        .with_staff_module(TenantMembershipCompatibilityModule::new(
            TenantMembershipAuthorityRepository::new(backing_pool.clone()),
        ))
        .with_user_settings_module(UserSettingsCompatibilityModule::new(
            UserSettingsProfileRepository::new(backing_pool.clone()),
        ))
        .with_tenant_governance_module(TenantGovernanceCompatibilityModule::new(
            TenantGovernanceRepository::new(backing_pool.clone()),
        ))
        .with_tenant_preview_module(TenantPreviewCompatibilityModule::new(
            TenantPreviewRepository::new(backing_pool),
        ))
        .with_repository_provider(repository_provider)
        .build()
        .unwrap();

        assert_eq!(built.module_names(), BASELINE_NON_PROVIDER_MODULES);
        assert_eq!(built.construction_receipts.len(), 46);
    }
}
