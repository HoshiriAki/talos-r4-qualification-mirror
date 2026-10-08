#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::repair::{
    RepairCreateOutcome, RepairDeviceStatsProjection, RepairListProjection, RepairMutationError,
    RepairProjection, RepairTransitionOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresRepairRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresRepairRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn create(
        &self,
        damage_report_id: &str,
        repair_description: &str,
        vendor: &str,
        operator: &str,
        now: &str,
    ) -> Result<RepairCreateOutcome, RepairMutationError> {
        crate::repositories::repair_postgres_create::create(
            self.session,
            damage_report_id,
            repair_description,
            vendor,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn start(
        &self,
        repair_id: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        crate::repositories::repair_postgres_transition::start(self.session, repair_id, now)
    }

    pub(in crate::repositories) fn complete(
        &self,
        repair_id: &str,
        repair_cost: f64,
        operator: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        crate::repositories::repair_postgres_transition::complete(
            self.session,
            repair_id,
            repair_cost,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn return_to_stock(
        &self,
        repair_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        crate::repositories::repair_postgres_transition::return_to_stock(
            self.session,
            repair_id,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn get(
        &self,
        repair_id: &str,
    ) -> Result<Option<RepairProjection>, RepositoryError> {
        crate::repositories::repair_postgres_read::get(self.session, repair_id)
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        device_serial_no: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<RepairListProjection, RepositoryError> {
        crate::repositories::repair_postgres_read::list(
            self.session,
            status,
            device_serial_no,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn device_stats(
        &self,
        device_serial_no: &str,
    ) -> Result<RepairDeviceStatsProjection, RepositoryError> {
        crate::repositories::repair_postgres_read::device_stats(self.session, device_serial_no)
    }
}
