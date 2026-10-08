#[cfg(all(test, feature = "postgres"))]
mod audit_compatibility_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod auth_security_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod barcode_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod booking_read_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod bootstrap_authority_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod consent_compatibility_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod contract_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod credit_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod damage_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod dashboard_read_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod deletion_compatibility_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod deposit_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod depreciation_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod device_candidate_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod device_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod identity_authority_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod invoice_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod legacy_reservation_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod lifecycle_write_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod machine_authority_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod maintenance_compatibility_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod model_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod notification_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod optical_sop_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod order_commands_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod overdue_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod p8f_performance_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod platform_membership_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod platform_tenant_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod pricing_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod procurement_qualification_tests;
mod provider;
#[cfg(all(test, feature = "postgres"))]
mod qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod quote_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod r3_settlement_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod refund_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod rental_closure_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod repair_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod report_compatibility_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod reservation_write_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod roa_qualification_tests;
mod session;
#[cfg(all(test, feature = "postgres"))]
mod settlement_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod tax_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod tenant_governance_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod tenant_membership_authority_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod tenant_preview_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod tenant_resolution_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod tenant_simulation_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod user_settings_profile_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod warehouse_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod work_task_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod workflow_qualification_tests;
#[cfg(all(test, feature = "postgres"))]
mod workflow_worker_tenant_source_qualification_tests;

pub use provider::PostgresRepositoryProvider;
pub(in crate::repositories) use session::PostgresRepositorySession;
