mod audit_compatibility;
mod barcode_compatibility;
mod booking_compatibility;
mod clock;
mod consent_compatibility;
mod contract_compatibility;
mod credit_compatibility;
mod customer;
mod damage_compatibility;
mod dashboard_read_authority;
mod deletion_compatibility;
mod deposit_compatibility;
mod depreciation_compatibility;
mod device_candidates;
mod device_compatibility;
mod device_mutation_authority;
mod device_read_authority;
mod excel_import_compatibility;
pub mod interconnect;
pub mod interconnect_driver;
pub mod interconnect_facets;
mod interconnect_plugin;
mod invoice_compatibility;
mod model_authority;
mod model_compatibility;
mod module_client;
mod notify_compatibility;
mod optical_sop_compatibility;
mod order_lifecycle_compatibility;
mod order_lifecycle_v2;
mod order_queries;
mod order_query_v2;
mod order_read_compatibility;
mod overdue_compatibility;
mod plugin_admission;
mod plugin_admission_isolation;
#[cfg(feature = "postgres")]
mod plugin_background_execution;
mod plugin_egress;
mod plugin_execution_admission;
mod plugin_execution_admission_service;
mod plugin_execution_services;
mod plugin_host;
mod plugin_host_operations;
mod plugin_lifecycle;
#[cfg(feature = "postgres")]
mod plugin_lifecycle_postgres;
#[cfg(feature = "sqlite")]
mod plugin_lifecycle_sqlite;
mod plugin_runtime;
mod plugin_runtime_services;
mod plugin_secret;
mod plugin_storage;
mod plugin_store;
mod plugin_verification;
mod pricing_compatibility;
mod procurement_compatibility;
mod quote;
mod r3_settlement;
mod refund_compatibility;
mod rental_close;
pub mod rental_time;
mod rental_workflow;
mod repair_compatibility;
mod report_compatibility;
mod reservation_compatibility;
mod reservation_v2;
mod roa_compatibility;
mod services;
mod settlement_compatibility;
mod tax_compatibility;
mod tenant_governance_compatibility;
mod tenant_membership_compatibility;
mod tenant_preview_compatibility;
mod tenant_simulation_compatibility;
mod two_fa_compatibility;
mod user_settings_compatibility;
mod warehouse_advanced_compatibility;
mod warehouse_authority;
mod warehouse_compatibility;
mod work_task_compatibility;
mod workers;

#[cfg(test)]
mod customer_tests;
#[cfg(all(test, feature = "postgres"))]
mod interconnect_plugin_pg_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod order_read_parity_tests;
#[cfg(all(test, feature = "postgres"))]
mod plugin_background_execution_pg_tests;
#[cfg(test)]
mod plugin_conformance_tests;
#[cfg(test)]
mod plugin_execution_services_tests;
#[cfg(all(test, feature = "postgres"))]
mod plugin_lifecycle_pg_tests;
#[cfg(all(test, feature = "postgres"))]
mod plugin_runtime_services_pg_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod plugin_storage_pg_tests;
#[cfg(all(test, feature = "sqlite"))]
mod plugin_storage_tests;
#[cfg(all(test, feature = "postgres"))]
mod plugin_store_pg_tests;
#[cfg(all(test, feature = "sqlite"))]
mod plugin_store_tests;
#[cfg(test)]
mod quote_tests;
#[cfg(test)]
mod r3_settlement_tests;
#[cfg(test)]
mod rental_closure_tests;
#[cfg(test)]
mod reservation_tests;
#[cfg(test)]
mod workflow_tests;
pub(crate) use audit_compatibility::AuditCompatibilityModule;
pub(crate) use barcode_compatibility::BarcodeCompatibilityModule;
pub(crate) use booking_compatibility::BookingCompatibilityModule;
#[cfg(test)]
pub use clock::FixedClock;
pub use clock::{Clock, SystemClock};
pub(crate) use consent_compatibility::ConsentCompatibilityModule;
pub(crate) use contract_compatibility::ContractCompatibilityModule;
pub(crate) use credit_compatibility::CreditCompatibilityModule;
pub use customer::CustomerModule;
pub(crate) use damage_compatibility::DamageCompatibilityModule;
pub use dashboard_read_authority::{DashboardReadAuthorityError, DashboardReadAuthorityService};
pub(crate) use deletion_compatibility::DeletionCompatibilityModule;
pub(crate) use deposit_compatibility::DepositCompatibilityModule;
pub(crate) use depreciation_compatibility::DepreciationCompatibilityModule;
pub(crate) use device_candidates::is_valid_serial_no;
pub use device_candidates::{
    DeviceCandidateQueryError, DeviceCandidateQueryService, DeviceMatchCandidate,
    DeviceResolveResult,
};
pub(crate) use device_compatibility::DeviceCompatibilityModule;
pub use device_mutation_authority::{
    DeviceBulkUpdateView, DeviceImportCandidate, DeviceImportView, DeviceMutationAuthorityError,
    DeviceMutationAuthorityService,
};
pub use device_read_authority::{
    DeviceReadAuthorityError, DeviceReadAuthorityService, DeviceReadPageView,
    DeviceReadPaginationView, DeviceReadView,
};
pub(crate) use excel_import_compatibility::ExcelImportCompatibilityModule;
pub(crate) use invoice_compatibility::InvoiceCompatibilityModule;
pub use model_authority::{ModelAuthorityError, ModelAuthorityService, ModelAuthorityView};
pub(crate) use model_compatibility::ModelCompatibilityModule;
pub use module_client::{ModuleClient, RegistryModuleClient};
pub(crate) use notify_compatibility::NotifyCompatibilityModule;
pub(crate) use optical_sop_compatibility::OpticalSopCompatibilityModule;
pub use order_lifecycle_compatibility::OrderLifecycleCompatibilityModule;
pub use order_lifecycle_v2::OrderLifecycleV2Module;
pub use order_queries::OrderQueryService;
pub use order_query_v2::OrderQueryV2Module;
pub use order_read_compatibility::OrderReadCompatibilityModule;
pub(crate) use overdue_compatibility::OverdueCompatibilityModule;
pub use plugin_admission::{
    PluginAdmissionPolicies, PluginAdmissionServiceError, PluginPrincipalAuthorityResolver,
};
#[cfg(feature = "postgres")]
pub use plugin_background_execution::{
    PluginBackgroundExecutionError, PostgresPluginBackgroundExecutionGate,
};
pub use plugin_egress::{
    PluginEgressBudgetRegistry, PluginEgressError, PluginEgressGrantPolicy,
    PluginSecretNetworkPolicy,
};
pub use plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionAdmissionError};
#[cfg(feature = "postgres")]
pub use plugin_execution_services::PostgresPluginExecutionStorageService;
#[cfg(feature = "sqlite")]
pub use plugin_execution_services::SqlitePluginExecutionStorageService;
pub use plugin_execution_services::{
    PluginExecutionEgressError, PluginExecutionEgressService, PluginExecutionSecretError,
    PluginExecutionSecretService, PluginExecutionStorageError,
};
pub use plugin_host::{
    PluginHostError, PluginInstallationLifecycle, PluginInstallationRecord,
    TrustedPluginRuntimePolicy,
};
pub use plugin_host_operations::PluginHostOperations;
#[cfg(feature = "postgres")]
pub use plugin_lifecycle::PostgresPluginLifecycleService;
#[cfg(feature = "sqlite")]
pub use plugin_lifecycle::SqlitePluginLifecycleService;
pub use plugin_lifecycle::{
    PluginInstallationActivation, PluginLifecycleMutationError, PluginUpgradeAuthorization,
};
pub use plugin_runtime::{
    DangerousPluginCombinationApproval, ProductionPluginHostError,
    TrustedDangerousPluginCombinationPolicy, TrustedNativePublisherPolicy,
};
pub use plugin_runtime_services::PluginRuntimeServices;
pub use plugin_secret::{
    PluginSecretBackend, PluginSecretBinding, PluginSecretError, PluginSecretPolicy,
};
pub use plugin_storage::{
    PluginStorageEntry, PluginStorageError, PluginStoragePolicy, PluginStorageQuota,
};
pub use plugin_store::PluginStoreError;
pub use plugin_verification::{
    PluginPackageVerificationError, PluginPublisherSignatureVerifier, VerifiedPluginPackage,
    verify_publisher_package,
};
pub(crate) use pricing_compatibility::PricingCompatibilityModule;
pub(crate) use procurement_compatibility::ProcurementCompatibilityModule;
pub use quote::QuoteModule;
pub use r3_settlement::R3SettlementModule;
pub(crate) use refund_compatibility::RefundCompatibilityModule;
pub use rental_close::{
    CloseTerminalAuthority, CloseTerminalSnapshot, RentalCloseCoordinator,
    UnavailableProductionCloseAuthority,
};
pub use rental_workflow::{
    DurableRentalProcessManager, RentalEffectOutcome, RentalWorkflowEffects, RentalWorkflowRun,
    UnavailableProductionRentalEffects,
};
pub(crate) use repair_compatibility::RepairCompatibilityModule;
pub(crate) use report_compatibility::ReportCompatibilityModule;
pub(crate) use reservation_compatibility::ReservationCompatibilityModule;
pub use reservation_v2::ReservationV2Module;
pub(crate) use roa_compatibility::RoaCompatibilityModule;
pub use services::ApplicationServices;
pub(crate) use settlement_compatibility::SettlementCompatibilityModule;
pub(crate) use tax_compatibility::TaxCompatibilityModule;
pub(crate) use tenant_governance_compatibility::TenantGovernanceCompatibilityModule;
pub(crate) use tenant_membership_compatibility::TenantMembershipCompatibilityModule;
pub(crate) use tenant_preview_compatibility::TenantPreviewCompatibilityModule;
pub(crate) use tenant_simulation_compatibility::TenantSimulationCompatibilityModule;
pub(crate) use two_fa_compatibility::TwoFaCompatibilityModule;
pub(crate) use user_settings_compatibility::UserSettingsCompatibilityModule;
pub(crate) use warehouse_advanced_compatibility::WarehouseAdvancedCompatibilityModule;
pub use warehouse_authority::{
    WarehouseAuthorityError, WarehouseAuthorityService, WarehouseDeviceDetailView,
    WarehouseDevicePageView, WarehouseRegionRuleView, WarehouseRouteView,
};
pub(crate) use warehouse_compatibility::WarehouseCompatibilityModule;
pub(crate) use work_task_compatibility::WorkTaskCompatibilityModule;
pub use workers::{
    DurableRentalWorkflowWorker, LegacyMaintenanceAdapter, MaintenanceWorkerRunner,
    RentalWorkflowWorkerPort, WorkerContextFactory, WorkerRunner,
};
