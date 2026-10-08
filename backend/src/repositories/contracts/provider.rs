use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::sync::Arc;
use system_core::ExecutionContext;

#[cfg(feature = "postgres")]
use sqlx::PgPool;

use crate::observability::MetricsSink;
use crate::repositories::barcode_dispatch::ScopedBarcodeRepository;
use crate::repositories::booking_read_dispatch::ScopedBookingReadRepository;
use crate::repositories::contract_dispatch::ScopedContractRepository;
use crate::repositories::credit_dispatch::ScopedCreditRepository;
use crate::repositories::customer_dispatch::ScopedCustomerRepository;
use crate::repositories::damage_dispatch::ScopedDamageRepository;
use crate::repositories::dashboard_read_dispatch::ScopedDashboardReadRepository;
use crate::repositories::deposit_dispatch::ScopedDepositRepository;
use crate::repositories::depreciation_dispatch::ScopedDepreciationRepository;
use crate::repositories::device_candidate_dispatch::ScopedDeviceCandidateRepository;
use crate::repositories::device_dispatch::ScopedDeviceRepository;
use crate::repositories::invoice_dispatch::ScopedInvoiceRepository;
use crate::repositories::legacy_reservation_dispatch::ScopedLegacyReservationRepository;
use crate::repositories::lifecycle_dispatch::ScopedOrderLifecycleRepository;
use crate::repositories::model_dispatch::ScopedModelRepository;
use crate::repositories::notification_dispatch::ScopedNotificationRepository;
use crate::repositories::optical_sop_dispatch::ScopedOpticalSopRepository;
use crate::repositories::order_commands_dispatch::ScopedOrderCommandRepository;
use crate::repositories::order_read_dispatch::ScopedOrderReadRepository;
use crate::repositories::overdue_dispatch::ScopedOverdueRepository;
use crate::repositories::pricing_dispatch::ScopedPricingRepository;
use crate::repositories::procurement_dispatch::ScopedProcurementRepository;
use crate::repositories::quote_dispatch::ScopedQuoteRepository;
use crate::repositories::r3_settlement_dispatch::ScopedR3SettlementRepository;
use crate::repositories::refund_dispatch::ScopedRefundRepository;
use crate::repositories::rental_closure_dispatch::ScopedRentalClosureRepository;
use crate::repositories::repair_dispatch::ScopedRepairRepository;
use crate::repositories::reservation_dispatch::ScopedReservationRepository;
use crate::repositories::roa_dispatch::ScopedRoaRepository;
use crate::repositories::session::RepositorySession;
use crate::repositories::settlement_dispatch::ScopedSettlementRepository;
use crate::repositories::sqlite::SqliteSessionBackend;
use crate::repositories::tax_dispatch::ScopedTaxRepository;
use crate::repositories::warehouse_dispatch::ScopedWarehouseRepository;
use crate::repositories::work_task_dispatch::ScopedWorkTaskRepository;
use crate::repositories::workflow_dispatch::ScopedWorkflowRepository;

#[cfg(feature = "postgres")]
use crate::repositories::postgres::PostgresRepositorySession;

use super::{RepositoryBinding, RepositoryError};

pub trait RepositoryProvider: Send + Sync {
    fn bind(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, RepositoryError>;
}

pub struct ScopedRepositories {
    session: RepositorySession,
    metrics: Arc<dyn MetricsSink>,
}

impl ScopedRepositories {
    pub(in crate::repositories) fn sqlite(
        binding: RepositoryBinding,
        pool: Pool<SqliteConnectionManager>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            session: RepositorySession::sqlite(SqliteSessionBackend::new(binding, pool)),
            metrics,
        }
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn postgres(
        binding: RepositoryBinding,
        pool: PgPool,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            session: RepositorySession::postgres(PostgresRepositorySession::new(binding, pool)),
            metrics,
        }
    }

    pub fn binding(&self) -> &RepositoryBinding {
        self.session.binding()
    }

    pub fn barcodes(&self) -> ScopedBarcodeRepository<'_> {
        ScopedBarcodeRepository::new(self)
    }

    pub fn booking_reads(&self) -> ScopedBookingReadRepository<'_> {
        ScopedBookingReadRepository::new(self)
    }

    pub fn contracts(&self) -> ScopedContractRepository<'_> {
        ScopedContractRepository::new(self)
    }

    pub fn customers(&self) -> ScopedCustomerRepository<'_> {
        ScopedCustomerRepository::new(self)
    }

    pub fn credits(&self) -> ScopedCreditRepository<'_> {
        ScopedCreditRepository::new(self)
    }

    pub fn damages(&self) -> ScopedDamageRepository<'_> {
        ScopedDamageRepository::new(self)
    }

    pub fn dashboards(&self) -> ScopedDashboardReadRepository<'_> {
        ScopedDashboardReadRepository::new(self)
    }

    pub fn depreciations(&self) -> ScopedDepreciationRepository<'_> {
        ScopedDepreciationRepository::new(self)
    }

    pub fn deposits(&self) -> ScopedDepositRepository<'_> {
        ScopedDepositRepository::new(self)
    }

    pub fn refunds(&self) -> ScopedRefundRepository<'_> {
        ScopedRefundRepository::new(self)
    }

    pub fn repairs(&self) -> ScopedRepairRepository<'_> {
        ScopedRepairRepository::new(self)
    }

    pub fn settlements(&self) -> ScopedSettlementRepository<'_> {
        ScopedSettlementRepository::new(self)
    }

    pub fn taxes(&self) -> ScopedTaxRepository<'_> {
        ScopedTaxRepository::new(self)
    }

    pub fn device_candidates(&self) -> ScopedDeviceCandidateRepository<'_> {
        ScopedDeviceCandidateRepository::new(self)
    }

    pub fn devices(&self) -> ScopedDeviceRepository<'_> {
        ScopedDeviceRepository::new(self)
    }

    pub fn models(&self) -> ScopedModelRepository<'_> {
        ScopedModelRepository::new(self)
    }

    pub fn notifications(&self) -> ScopedNotificationRepository<'_> {
        ScopedNotificationRepository::new(self)
    }

    pub fn warehouses(&self) -> ScopedWarehouseRepository<'_> {
        ScopedWarehouseRepository::new(self)
    }

    pub fn orders(&self) -> ScopedOrderReadRepository<'_> {
        ScopedOrderReadRepository::new(self)
    }

    pub fn order_commands(&self) -> ScopedOrderCommandRepository<'_> {
        ScopedOrderCommandRepository::new(self)
    }

    pub fn overdues(&self) -> ScopedOverdueRepository<'_> {
        ScopedOverdueRepository::new(self)
    }

    pub fn optical_sops(&self) -> ScopedOpticalSopRepository<'_> {
        ScopedOpticalSopRepository::new(self)
    }

    pub fn invoices(&self) -> ScopedInvoiceRepository<'_> {
        ScopedInvoiceRepository::new(self)
    }

    pub fn legacy_reservations(&self) -> ScopedLegacyReservationRepository<'_> {
        ScopedLegacyReservationRepository::new(self)
    }

    pub fn lifecycles(&self) -> ScopedOrderLifecycleRepository<'_> {
        ScopedOrderLifecycleRepository::new(self)
    }

    pub fn pricing(&self) -> ScopedPricingRepository<'_> {
        ScopedPricingRepository::new(self)
    }

    pub fn procurements(&self) -> ScopedProcurementRepository<'_> {
        ScopedProcurementRepository::new(self)
    }

    pub fn quotes(&self) -> ScopedQuoteRepository<'_> {
        ScopedQuoteRepository::new(self)
    }

    pub fn reservations(&self) -> ScopedReservationRepository<'_> {
        ScopedReservationRepository::new(self)
    }

    pub fn rental_closure(&self) -> ScopedRentalClosureRepository<'_> {
        ScopedRentalClosureRepository::new(self)
    }

    pub fn roa(&self) -> ScopedRoaRepository<'_> {
        ScopedRoaRepository::new(self)
    }

    pub fn r3_settlements(&self) -> ScopedR3SettlementRepository<'_> {
        ScopedR3SettlementRepository::new(self)
    }

    pub fn work_tasks(&self) -> ScopedWorkTaskRepository<'_> {
        ScopedWorkTaskRepository::new(self)
    }

    pub fn workflows(&self) -> ScopedWorkflowRepository<'_> {
        ScopedWorkflowRepository::new(self)
    }

    pub(in crate::repositories) fn session(&self) -> &RepositorySession {
        &self.session
    }

    pub(in crate::repositories) fn metrics(&self) -> Arc<dyn MetricsSink> {
        self.metrics.clone()
    }
}
