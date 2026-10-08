use crate::repositories::procurement::{
    NewProcurementRecord, ProcurementMutationError, ProcurementProjection,
    SqliteProcurementRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::procurement_postgres::PostgresProcurementRepository;

pub struct ScopedProcurementRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedProcurementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn get(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<ProcurementProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresProcurementRepository::new(self.scoped.session()).get(device_serial_no);
        }
        SqliteProcurementRepository::new(self.scoped).get(device_serial_no)
    }

    pub fn create(
        &self,
        input: &NewProcurementRecord,
    ) -> Result<ProcurementProjection, ProcurementMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresProcurementRepository::new(self.scoped.session()).create(input);
        }
        SqliteProcurementRepository::new(self.scoped).create(input)
    }

    pub fn set_replacement_value(
        &self,
        device_serial_no: &str,
        replacement_value: f64,
    ) -> Result<ProcurementProjection, ProcurementMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresProcurementRepository::new(self.scoped.session())
                .set_replacement_value(device_serial_no, replacement_value);
        }
        SqliteProcurementRepository::new(self.scoped)
            .set_replacement_value(device_serial_no, replacement_value)
    }
}
