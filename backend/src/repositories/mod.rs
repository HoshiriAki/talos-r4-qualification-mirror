mod audit_compatibility;
mod audit_compatibility_dispatch;
#[cfg(feature = "postgres")]
mod audit_compatibility_postgres;
mod auth_security;
mod auth_security_dispatch;
#[cfg(feature = "postgres")]
mod auth_security_postgres;
mod barcode;
mod barcode_dispatch;
#[cfg(feature = "postgres")]
mod barcode_postgres;
#[cfg(feature = "postgres")]
mod barcode_postgres_common;
#[cfg(feature = "postgres")]
mod barcode_postgres_mutation;
#[cfg(feature = "postgres")]
mod barcode_postgres_read;
mod booking_read;
mod booking_read_dispatch;
#[cfg(feature = "postgres")]
mod booking_read_postgres;
mod bootstrap_authority;
mod bootstrap_authority_dispatch;
#[cfg(feature = "postgres")]
mod bootstrap_authority_postgres;
mod consent_compatibility;
mod consent_compatibility_dispatch;
#[cfg(feature = "postgres")]
mod consent_compatibility_postgres;
mod contract;
mod contract_dispatch;
#[cfg(feature = "postgres")]
mod contract_postgres;
#[cfg(feature = "postgres")]
mod contract_postgres_common;
#[cfg(feature = "postgres")]
mod contract_postgres_mutation;
#[cfg(feature = "postgres")]
mod contract_postgres_read;
mod contracts;
mod credit;
mod credit_dispatch;
#[cfg(feature = "postgres")]
mod credit_postgres;
#[cfg(feature = "postgres")]
mod credit_postgres_common;
#[cfg(feature = "postgres")]
mod credit_postgres_mutation;
#[cfg(feature = "postgres")]
mod credit_postgres_read;
mod customer;
mod customer_dispatch;
#[cfg(feature = "postgres")]
mod customer_postgres;
mod damage;
mod damage_dispatch;
#[cfg(feature = "postgres")]
mod damage_postgres;
#[cfg(feature = "postgres")]
mod damage_postgres_common;
#[cfg(feature = "postgres")]
mod damage_postgres_read;
#[cfg(feature = "postgres")]
mod damage_postgres_report;
#[cfg(feature = "postgres")]
mod damage_postgres_transition;
mod dashboard_read;
mod dashboard_read_dispatch;
#[cfg(feature = "postgres")]
mod dashboard_read_postgres;
mod deletion_compatibility;
mod deletion_compatibility_dispatch;
#[cfg(feature = "postgres")]
mod deletion_compatibility_postgres;
mod deposit;
mod deposit_dispatch;
#[cfg(feature = "postgres")]
mod deposit_postgres;
#[cfg(feature = "postgres")]
mod deposit_postgres_calculate;
#[cfg(feature = "postgres")]
mod deposit_postgres_collect;
#[cfg(feature = "postgres")]
mod deposit_postgres_common;
#[cfg(feature = "postgres")]
mod deposit_postgres_forfeit;
#[cfg(feature = "postgres")]
mod deposit_postgres_read;
#[cfg(feature = "postgres")]
mod deposit_postgres_release;
mod depreciation;
mod depreciation_dispatch;
#[cfg(feature = "postgres")]
mod depreciation_postgres;
mod device;
mod device_candidate;
mod device_candidate_dispatch;
#[cfg(feature = "postgres")]
mod device_candidate_postgres;
mod device_dispatch;
#[cfg(feature = "postgres")]
mod device_postgres;
mod identity_authority;
mod identity_authority_dispatch;
#[cfg(feature = "postgres")]
mod identity_authority_postgres;
mod invoice;
mod invoice_dispatch;
#[cfg(feature = "postgres")]
mod invoice_postgres;
#[cfg(feature = "postgres")]
mod invoice_postgres_common;
#[cfg(feature = "postgres")]
mod invoice_postgres_mutation;
#[cfg(feature = "postgres")]
mod invoice_postgres_read;
mod legacy_reservation;
mod legacy_reservation_dispatch;
#[cfg(feature = "postgres")]
mod legacy_reservation_postgres;
mod lifecycle;
mod lifecycle_dispatch;
#[cfg(feature = "postgres")]
mod lifecycle_postgres;
#[cfg(feature = "postgres")]
mod lifecycle_postgres_write;
mod machine_authority;
mod machine_authority_dispatch;
#[cfg(feature = "postgres")]
mod machine_authority_postgres;
mod machine_authority_sqlite;
mod maintenance_compatibility;
mod maintenance_compatibility_dispatch;
#[cfg(feature = "postgres")]
mod maintenance_compatibility_postgres;
mod model;
mod model_dispatch;
#[cfg(feature = "postgres")]
mod model_postgres;
mod notification;
mod notification_dispatch;
#[cfg(feature = "postgres")]
mod notification_postgres;
#[cfg(feature = "postgres")]
mod notification_postgres_common;
#[cfg(feature = "postgres")]
mod notification_postgres_mutation;
#[cfg(feature = "postgres")]
mod notification_postgres_read;
mod optical_sop;
mod optical_sop_dispatch;
#[cfg(feature = "postgres")]
mod optical_sop_postgres;
#[cfg(feature = "postgres")]
mod optical_sop_postgres_common;
#[cfg(feature = "postgres")]
mod optical_sop_postgres_mutation;
#[cfg(feature = "postgres")]
mod optical_sop_postgres_read;
mod order_commands;
mod order_commands_dispatch;
#[cfg(feature = "postgres")]
mod order_commands_postgres;
mod order_read;
mod order_read_dispatch;
#[cfg(feature = "postgres")]
mod order_read_postgres;
mod overdue;
mod overdue_dispatch;
#[cfg(feature = "postgres")]
mod overdue_postgres;
#[cfg(feature = "postgres")]
mod overdue_postgres_common;
#[cfg(feature = "postgres")]
mod overdue_postgres_mutation;
#[cfg(feature = "postgres")]
mod overdue_postgres_read;
mod platform_membership;
mod platform_membership_dispatch;
#[cfg(feature = "postgres")]
mod platform_membership_postgres;
mod platform_tenant;
mod platform_tenant_dispatch;
#[cfg(feature = "postgres")]
mod platform_tenant_postgres;
#[cfg(feature = "postgres")]
mod postgres;
mod pricing;
mod pricing_dispatch;
#[cfg(feature = "postgres")]
mod pricing_postgres;
#[cfg(feature = "postgres")]
mod pricing_postgres_common;
#[cfg(feature = "postgres")]
mod pricing_postgres_mutation;
#[cfg(feature = "postgres")]
mod pricing_postgres_read;
mod procurement;
mod procurement_dispatch;
#[cfg(feature = "postgres")]
mod procurement_postgres;
mod quote;
mod quote_dispatch;
#[cfg(feature = "postgres")]
mod quote_postgres;
mod r3_settlement;
mod r3_settlement_dispatch;
#[cfg(feature = "postgres")]
mod r3_settlement_postgres;
mod refund;
mod refund_dispatch;
#[cfg(feature = "postgres")]
mod refund_postgres;
#[cfg(feature = "postgres")]
mod refund_postgres_common;
#[cfg(feature = "postgres")]
mod refund_postgres_execute;
#[cfg(feature = "postgres")]
mod refund_postgres_read;
#[cfg(feature = "postgres")]
mod refund_postgres_request;
#[cfg(feature = "postgres")]
mod refund_postgres_transition;
mod rental_closure;
mod rental_closure_dispatch;
#[cfg(feature = "postgres")]
mod rental_closure_postgres;
mod repair;
mod repair_dispatch;
#[cfg(feature = "postgres")]
mod repair_postgres;
#[cfg(feature = "postgres")]
mod repair_postgres_common;
#[cfg(feature = "postgres")]
mod repair_postgres_create;
#[cfg(feature = "postgres")]
mod repair_postgres_read;
#[cfg(feature = "postgres")]
mod repair_postgres_transition;
mod report_compatibility;
mod report_compatibility_dispatch;
#[cfg(feature = "postgres")]
mod report_compatibility_postgres;
mod reservation;
mod reservation_dispatch;
#[cfg(feature = "postgres")]
mod reservation_postgres;
#[cfg(feature = "postgres")]
mod reservation_postgres_write;
mod roa;
mod roa_dispatch;
#[cfg(feature = "postgres")]
mod roa_postgres;
mod session;
mod settlement;
mod settlement_dispatch;
#[cfg(feature = "postgres")]
mod settlement_postgres;
#[cfg(feature = "postgres")]
mod settlement_postgres_common;
#[cfg(feature = "postgres")]
mod settlement_postgres_mutation;
#[cfg(feature = "postgres")]
mod settlement_postgres_read;
mod sqlite;
mod tax;
mod tax_dispatch;
#[cfg(feature = "postgres")]
mod tax_postgres;
#[cfg(feature = "postgres")]
mod tax_postgres_common;
#[cfg(feature = "postgres")]
mod tax_postgres_mutation;
#[cfg(feature = "postgres")]
mod tax_postgres_read;
mod tenant_governance;
mod tenant_governance_dispatch;
#[cfg(feature = "postgres")]
mod tenant_governance_postgres;
mod tenant_membership_authority;
mod tenant_membership_authority_dispatch;
#[cfg(feature = "postgres")]
mod tenant_membership_authority_postgres;
mod tenant_preview;
mod tenant_preview_dispatch;
#[cfg(feature = "postgres")]
mod tenant_preview_postgres;
mod tenant_resolution;
mod tenant_resolution_dispatch;
#[cfg(feature = "postgres")]
mod tenant_resolution_postgres;
mod user_settings_profile;
mod user_settings_profile_dispatch;
#[cfg(feature = "postgres")]
mod user_settings_profile_postgres;
mod warehouse;
mod warehouse_dispatch;
#[cfg(feature = "postgres")]
mod warehouse_postgres;
mod work_task;
mod work_task_dispatch;
#[cfg(feature = "postgres")]
mod work_task_postgres;
mod workflow;
mod workflow_dispatch;
#[cfg(feature = "postgres")]
mod workflow_postgres;
mod workflow_worker_tenant_source;

pub(crate) use audit_compatibility::{AuditAuthorityWrite, AuditCompatibilityWrite};
pub(crate) use audit_compatibility_dispatch::AuditCompatibilityRepository;
pub(crate) use auth_security::{
    StoredRateState, TotpCredentialRewrap, TotpDisableOutcome, TotpEnrollmentOutcome, TotpStatus,
};
pub(crate) use auth_security_dispatch::AuthSecurityRepository;
pub(crate) use barcode::BarcodeMutationError;
pub use barcode::{
    BarcodeDeviceInfoProjection, BarcodeLabelProjection, BarcodeLookupProjection,
    ScanEventProjection, ScanHistoryProjection, ScanStatsByType, ScanStatsProjection,
};
pub use barcode_dispatch::ScopedBarcodeRepository;
pub use booking_read::{
    BookingAvailabilityDay, BookingAvailabilityProjection, BookingDeviceSummary,
    BookingPriceProjection, BookingSearchItem,
};
pub use booking_read_dispatch::ScopedBookingReadRepository;
pub(crate) use bootstrap_authority::{
    BootstrapPlatformOwnerCommand, BootstrapPlatformOwnerOutcome,
};
pub(crate) use bootstrap_authority_dispatch::BootstrapAuthorityRepository;
pub(crate) use consent_compatibility::{
    ConsentAuditRecord, ConsentRecordResult, ConsentRevokeResult,
};
pub(crate) use consent_compatibility_dispatch::ConsentCompatibilityRepository;
pub(crate) use contract::ContractMutationError;
pub use contract::{
    ContractGenerateOutcome, ContractListItem, ContractListProjection, ContractProjection,
    ContractSignOutcome, ContractSignatureProjection, ContractTemplateCreateOutcome,
    ContractTemplateListProjection, ContractTemplateProjection, ContractTemplateUpdateOutcome,
    ContractVerifyOutcome,
};
pub use contract_dispatch::ScopedContractRepository;
pub use contracts::{
    RepositoryAccess, RepositoryBinding, RepositoryError, RepositoryProvider, ScopedRepositories,
};
pub(crate) use credit::CreditMutationError;
pub use credit::{
    BlacklistAddOutcome, BlacklistRemoveOutcome, CreditRecalculateOutcome, ViolationRecordOutcome,
    ViolationTransitionOutcome, score_label as credit_score_label,
};
pub use credit_dispatch::ScopedCreditRepository;
pub use customer::{
    CustomerContactProjection, CustomerMigrationExceptionProjection, CustomerProjection,
    DuplicateCustomerCandidate, NewCustomerContact, NewCustomerRecord,
};
pub use customer_dispatch::ScopedCustomerRepository;
pub(crate) use damage::DamageMutationError;
pub use damage::{
    DamageListItem, DamageListProjection, DamageProjection, DamageReportOutcome,
    DamageTransitionOutcome,
};
pub use damage_dispatch::ScopedDamageRepository;
pub use dashboard_read::{
    DashboardCountPoint, DashboardModelRankingPoint, DashboardOrderBucket, DashboardOrderWindow,
    DashboardOverdueRow, DashboardOverviewProjection, DashboardProvincePiePoint,
    DashboardProvinceTrendPoint, DashboardRecentOrder, DashboardRevenuePoint,
    DashboardWarehousePoint,
};
pub use dashboard_read_dispatch::ScopedDashboardReadRepository;
pub(crate) use deletion_compatibility::{
    DeletionCompatibilityError, DeletionCompleteResult, DeletionRecord, DeletionRequestOutcome,
};
pub(crate) use deletion_compatibility_dispatch::DeletionCompatibilityRepository;
pub(crate) use deposit::DepositMutationError;
pub use deposit::{
    DepositCalculateOutcome, DepositCollectOutcome, DepositDetailProjection, DepositForfeitOutcome,
    DepositLedgerProjection, DepositProjection, DepositReleaseOutcome,
};
pub use deposit_dispatch::ScopedDepositRepository;
pub(crate) use depreciation::DepreciationMutationError;
pub use depreciation::{DepreciationLogProjection, DepreciationSnapshot, MonthlyDepreciationRun};
pub use depreciation_dispatch::ScopedDepreciationRepository;
pub(crate) use device::{
    DeviceCheckinStatusMutation, DeviceCompatibilityReadRequest, DeviceCompatibilityReadRow,
    ImportedDeviceDraft,
};
pub use device::{
    DeviceListRequest, DevicePage, DevicePagedRequest, DevicePagination, DevicePatch,
    DeviceProjection, DeviceSortDirection, DeviceSortField, NewDevice,
};
pub(crate) use device_candidate::DEVICE_IMPORT_MATCH_CANDIDATES_MAX;
pub use device_candidate_dispatch::ScopedDeviceCandidateRepository;
pub use device_dispatch::ScopedDeviceRepository;
pub(crate) use identity_authority_dispatch::IdentityAuthorityRepository;
pub(crate) use invoice::InvoiceMutationError;
pub use invoice::{
    InvoiceIssueOutcome, InvoiceListItem, InvoiceListProjection, InvoiceProjection,
    InvoiceRedFlushOutcome, InvoiceVoidOutcome,
};
pub use invoice_dispatch::ScopedInvoiceRepository;
pub use legacy_reservation::{
    LegacyReservationConflict, LegacyReservationList, LegacyReservationProjection,
    LegacyReservationRule, LegacyReservationRulePatch,
};
pub use legacy_reservation_dispatch::ScopedLegacyReservationRepository;
pub use lifecycle::{
    LifecycleAllowedAction, LifecycleHistoryProjection, LifecycleMigrationExceptionProjection,
    LifecycleOperationalView, OrderLifecycleProjection,
};
pub use lifecycle_dispatch::ScopedOrderLifecycleRepository;
pub(crate) use machine_authority::{
    MachineAdminContext, MachineAuthorization, MachineClientProjection, MachineIssuedCredential,
    MachineProvisionRecord, MachineScopeRecord, machine_scope_allowed,
};
pub(crate) use machine_authority_dispatch::MachineAuthorityRepository;
#[cfg(feature = "postgres")]
pub(crate) use machine_authority_postgres::PostgresMachineAuthorityRepository;
pub(crate) use maintenance_compatibility::MaintenanceStatusSync;
pub(crate) use maintenance_compatibility_dispatch::MaintenanceCompatibilityRepository;
pub use model::ModelProjection;
pub(crate) use model::{ModelMutationError, ModelPatch, ModelPricingPatch, NewModel};
pub use model_dispatch::ScopedModelRepository;
pub(crate) use notification::NotificationMutationError;
pub use notification::{
    NotificationListProjection, NotificationLogOutcome, NotificationMessageProjection,
    NotificationOrderContext, NotificationTemplateProjection, NotificationTemplateUpsertOutcome,
};
pub use notification_dispatch::ScopedNotificationRepository;
pub(crate) use optical_sop::OpticalSopMutationError;
pub use optical_sop::{
    OpticalCompleteOutcome, OpticalCreateOutcome, OpticalInspectionProjection,
    OpticalListProjection, OpticalStatsProjection, OpticalUpdateOutcome,
};
pub use optical_sop_dispatch::ScopedOpticalSopRepository;
pub use order_commands::{DraftOrderPatch, ImportedOrderDraft, ImportedOrderProjection};
pub use order_commands_dispatch::ScopedOrderCommandRepository;
pub use order_read::{
    OrderListRequest, OrderReadPage, OrderReadProjection, OrderSortDirection, OrderSortField,
};
pub use order_read_dispatch::ScopedOrderReadRepository;
pub(crate) use overdue::OverdueMutationError;
pub use overdue::{OverdueFeeConfig, OverdueFeeConfigPatch};
pub use overdue_dispatch::ScopedOverdueRepository;
pub(crate) use platform_membership::{
    CreatePlatformMembership, CreatedPlatformMembership, NewPlatformIdentity,
    PlatformMembershipAuditActor, PlatformMembershipMutationError, PlatformMembershipProjection,
    RevokePlatformMembership, UpdatePlatformMembership,
};
pub(crate) use platform_membership_dispatch::PlatformMembershipRepository;
pub(crate) use platform_tenant::{
    ClosePlatformTenant, CreatePlatformTenant, PlatformTenantAuditActor,
    PlatformTenantMutationError, PlatformTenantProjection, UpdatePlatformTenant,
    UpdatePlatformTenantStatus,
};
pub(crate) use platform_tenant_dispatch::PlatformTenantRepository;
#[cfg(feature = "postgres")]
pub use postgres::PostgresRepositoryProvider;
pub use pricing::{DynamicPriceRecord, ModelBasePriceRecord, PricingConfigRecord};
pub use pricing_dispatch::ScopedPricingRepository;
pub use procurement::ProcurementProjection;
pub(crate) use procurement::{NewProcurementRecord, ProcurementMutationError};
pub use procurement_dispatch::ScopedProcurementRepository;
pub use quote::{
    AccessoryProjection, NewQuote, NewQuoteLine, OrderFromQuoteProjection, QuoteLineProjection,
    QuoteProjection,
};
pub use quote_dispatch::ScopedQuoteRepository;
pub use r3_settlement::{
    AdditionalChargeInput, DamageFindingInput, DamageFindingProjection, DepositDeductionInput,
    DisputeProjection, InspectionCompletionInput, InspectionCompletionProjection,
    LiabilityDecisionInput, LiabilityDecisionProjection, OpenDisputeInput, R3ClosureFacts,
    RepairCaseProjection, RepairDecisionInput, RepairTransitionInput, ResolveDisputeInput,
    SettlementCaseProjection, SettlementEffectAdmissionProjection, SettlementIdInput,
    SettlementLineInput, SettlementProposalInput,
};
pub use r3_settlement_dispatch::ScopedR3SettlementRepository;
pub(crate) use refund::RefundMutationError;
pub use refund::{
    RefundExecuteOutcome, RefundListItem, RefundListProjection, RefundRequestOutcome,
    RefundStatusOutcome,
};
pub use refund_dispatch::ScopedRefundRepository;
pub use rental_closure::{
    InspectionProjection, RentalClosureFacts, ReturnProjection, SettlementProjection,
};
pub use rental_closure_dispatch::ScopedRentalClosureRepository;
pub(crate) use repair::RepairMutationError;
pub use repair::{
    RepairCreateOutcome, RepairDeviceStatsProjection, RepairHistoryItem, RepairListItem,
    RepairListProjection, RepairProjection, RepairTransitionOutcome,
};
pub use repair_dispatch::ScopedRepairRepository;
pub(crate) use report_compatibility_dispatch::ReportCompatibilityRepository;
pub use reservation::{
    AllocationProjection, CapacityProjection, DeviceAllocationRequest,
    ReservationMigrationExceptionProjection, ReservationProjection,
    ReservationRequirementProjection,
};
pub use reservation_dispatch::ScopedReservationRepository;
pub use roa::RoaBasisProjection;
pub use roa_dispatch::ScopedRoaRepository;
pub(in crate::repositories) use session::RepositorySession;
pub(crate) use settlement::SettlementMutationError;
pub use settlement::{
    SettlementConfirmOutcome, SettlementGenerateOutcome, SettlementListProjection,
    SettlementProjection as FinanceSettlementProjection, SettlementRevenueDetail,
};
pub use settlement_dispatch::ScopedSettlementRepository;
pub use sqlite::SqliteRepositoryProvider;
pub use tax::{
    TaxConfigExportRow, TaxConfigProjection, TaxExportSnapshot, TaxInvoiceExportRow,
    TaxUpsertOutcome,
};
pub use tax_dispatch::ScopedTaxRepository;
pub(crate) use tenant_governance::{
    GovernanceAuditQuery, GovernanceAuditRecord, GovernanceChangeIntent,
    GovernanceHealthProjection, GovernanceMutationError, GovernanceTenantListQuery,
    GovernanceTenantProjection,
};
pub(crate) use tenant_governance_dispatch::TenantGovernanceRepository;
pub(crate) use tenant_membership_authority::{
    TenantMembershipActor, TenantMembershipAuthorityError, TenantOwnershipTransfer,
};
pub(crate) use tenant_membership_authority_dispatch::TenantMembershipAuthorityRepository;
pub(crate) use tenant_preview::{
    PreviewDashboardProjection, PreviewMutationError, PreviewSessionCreate,
    PreviewSessionProjection,
};
pub(crate) use tenant_preview_dispatch::TenantPreviewRepository;
pub(crate) use tenant_resolution::TenantResolutionRecord;
pub(crate) use tenant_resolution_dispatch::TenantResolutionRepository;
pub(crate) use user_settings_profile_dispatch::UserSettingsProfileRepository;
pub(crate) use warehouse::{
    NewWarehouse, UpsertWarehouseRegionRule, WarehouseMutationError, WarehousePatch,
};
pub use warehouse::{
    WarehouseAdvancedCapacityProjection, WarehouseDeviceDetailProjection,
    WarehouseDeviceMoveOutcome, WarehouseDevicePage, WarehouseDeviceProjection,
    WarehouseLowStockProjection, WarehouseProjection, WarehouseRegionRuleProjection,
    WarehouseRoutingRuleProjection, WarehouseStatsProjection,
};
pub use warehouse_dispatch::ScopedWarehouseRepository;
pub use work_task::{WorkTaskListRequest, WorkTaskMutation, WorkTaskProjection, WorkTaskState};
pub use work_task_dispatch::ScopedWorkTaskRepository;
pub use workflow::{
    ClaimedWorkflowStep, RENTAL_DEFINITION_HASH, RENTAL_DEFINITION_ID, RENTAL_DEFINITION_VERSION,
    RENTAL_STEPS, WorkflowInstance, WorkflowStep, WorkflowStepState,
};
pub use workflow_dispatch::ScopedWorkflowRepository;
pub(crate) use workflow_worker_tenant_source::WorkflowWorkerTenantSource;
