use crate::repositories::optical_sop::{
    OpticalCompleteOutcome, OpticalCreateOutcome, OpticalInspectionProjection,
    OpticalListProjection, OpticalSopMutationError, OpticalStatsProjection, OpticalUpdateOutcome,
    SqliteOpticalSopRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::optical_sop_postgres::PostgresOpticalSopRepository;

pub struct ScopedOpticalSopRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedOpticalSopRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn create(
        &self,
        order_id: &str,
        device_serial_no: &str,
        inspector_id: &str,
        now: &str,
    ) -> Result<OpticalCreateOutcome, OpticalSopMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOpticalSopRepository::new(self.scoped.session()).create(
                order_id,
                device_serial_no,
                inspector_id,
                now,
            );
        }
        SqliteOpticalSopRepository::new(self.scoped).create(
            order_id,
            device_serial_no,
            inspector_id,
            now,
        )
    }

    pub fn update_step(
        &self,
        id: i64,
        step: &str,
        ok: bool,
        note: Option<&str>,
        now: &str,
    ) -> Result<OpticalUpdateOutcome, OpticalSopMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOpticalSopRepository::new(self.scoped.session())
                .update_step(id, step, ok, note, now);
        }
        SqliteOpticalSopRepository::new(self.scoped).update_step(id, step, ok, note, now)
    }

    pub fn complete(
        &self,
        id: i64,
        operator: &str,
        now: &str,
    ) -> Result<OpticalCompleteOutcome, OpticalSopMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOpticalSopRepository::new(self.scoped.session())
                .complete(id, operator, now);
        }
        SqliteOpticalSopRepository::new(self.scoped).complete(id, operator, now)
    }

    pub fn get(&self, id: i64) -> Result<Option<OpticalInspectionProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOpticalSopRepository::new(self.scoped.session()).get(id);
        }
        SqliteOpticalSopRepository::new(self.scoped).get(id)
    }

    pub fn list(
        &self,
        order_id: Option<&str>,
        device_serial_no: Option<&str>,
        overall_grade: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<OpticalListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOpticalSopRepository::new(self.scoped.session()).list(
                order_id,
                device_serial_no,
                overall_grade,
                page,
                page_size,
            );
        }
        SqliteOpticalSopRepository::new(self.scoped).list(
            order_id,
            device_serial_no,
            overall_grade,
            page,
            page_size,
        )
    }

    pub fn stats(&self) -> Result<OpticalStatsProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOpticalSopRepository::new(self.scoped.session()).stats();
        }
        SqliteOpticalSopRepository::new(self.scoped).stats()
    }
}
