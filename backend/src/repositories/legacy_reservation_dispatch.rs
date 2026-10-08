use crate::repositories::legacy_reservation::{
    LegacyReservationConflict, LegacyReservationList, LegacyReservationProjection,
    LegacyReservationRule, LegacyReservationRulePatch, SqliteLegacyReservationRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::legacy_reservation_postgres::PostgresLegacyReservationRepository;

pub struct ScopedLegacyReservationRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedLegacyReservationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list(
        &self,
        status: Option<&str>,
        device_serial_no: Option<&str>,
        customer_phone: Option<&str>,
        warehouse_id: Option<i64>,
        page: i64,
        page_size: i64,
    ) -> Result<LegacyReservationList, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresLegacyReservationRepository::new(self.scoped.session()).list(
                status,
                device_serial_no,
                customer_phone,
                warehouse_id,
                page,
                page_size,
            );
        }
        SqliteLegacyReservationRepository::new(self.scoped).list(
            status,
            device_serial_no,
            customer_phone,
            warehouse_id,
            page,
            page_size,
        )
    }

    pub fn get(&self, id: i64) -> Result<Option<LegacyReservationProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresLegacyReservationRepository::new(self.scoped.session()).get(id);
        }
        SqliteLegacyReservationRepository::new(self.scoped).get(id)
    }

    pub fn conflicts(
        &self,
        device_serial_no: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<LegacyReservationConflict>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresLegacyReservationRepository::new(self.scoped.session()).conflicts(
                device_serial_no,
                start_date,
                end_date,
            );
        }
        SqliteLegacyReservationRepository::new(self.scoped).conflicts(
            device_serial_no,
            start_date,
            end_date,
        )
    }

    pub fn get_rule(&self) -> Result<Option<LegacyReservationRule>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresLegacyReservationRepository::new(self.scoped.session()).get_rule();
        }
        SqliteLegacyReservationRepository::new(self.scoped).get_rule()
    }

    pub fn update_rule(
        &self,
        patch: &LegacyReservationRulePatch,
        now: &str,
    ) -> Result<LegacyReservationRule, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresLegacyReservationRepository::new(self.scoped.session())
                .update_rule(patch, now);
        }
        SqliteLegacyReservationRepository::new(self.scoped).update_rule(patch, now)
    }
}
