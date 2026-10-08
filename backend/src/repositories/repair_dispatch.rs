use crate::repositories::repair::{
    RepairCreateOutcome, RepairDeviceStatsProjection, RepairListProjection, RepairMutationError,
    RepairProjection, RepairTransitionOutcome, SqliteRepairRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::repair_postgres::PostgresRepairRepository;

pub struct ScopedRepairRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedRepairRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn create(
        &self,
        damage_report_id: &str,
        repair_description: &str,
        vendor: &str,
        operator: &str,
        now: &str,
    ) -> Result<RepairCreateOutcome, RepairMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session()).create(
                damage_report_id,
                repair_description,
                vendor,
                operator,
                now,
            );
        }
        SqliteRepairRepository::new(self.scoped).create(
            damage_report_id,
            repair_description,
            vendor,
            operator,
            now,
        )
    }

    pub fn start(
        &self,
        repair_id: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session()).start(repair_id, now);
        }
        SqliteRepairRepository::new(self.scoped).start(repair_id, now)
    }

    pub fn complete(
        &self,
        repair_id: &str,
        repair_cost: f64,
        operator: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session()).complete(
                repair_id,
                repair_cost,
                operator,
                now,
            );
        }
        SqliteRepairRepository::new(self.scoped).complete(repair_id, repair_cost, operator, now)
    }

    pub fn return_to_stock(
        &self,
        repair_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session())
                .return_to_stock(repair_id, operator, now);
        }
        SqliteRepairRepository::new(self.scoped).return_to_stock(repair_id, operator, now)
    }

    pub fn get(&self, repair_id: &str) -> Result<Option<RepairProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session()).get(repair_id);
        }
        SqliteRepairRepository::new(self.scoped).get(repair_id)
    }

    pub fn list(
        &self,
        status: Option<&str>,
        device_serial_no: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<RepairListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session()).list(
                status,
                device_serial_no,
                page,
                page_size,
            );
        }
        SqliteRepairRepository::new(self.scoped).list(status, device_serial_no, page, page_size)
    }

    pub fn device_stats(
        &self,
        device_serial_no: &str,
    ) -> Result<RepairDeviceStatsProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRepairRepository::new(self.scoped.session())
                .device_stats(device_serial_no);
        }
        SqliteRepairRepository::new(self.scoped).device_stats(device_serial_no)
    }
}
