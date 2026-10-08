use crate::repositories::roa::{RoaBasisProjection, SqliteRoaRepository};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::roa_postgres::PostgresRoaRepository;

pub struct ScopedRoaRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedRoaRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn get(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<RoaBasisProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRoaRepository::new(self.scoped.session()).get(device_serial_no);
        }
        SqliteRoaRepository::new(self.scoped).get(device_serial_no)
    }

    pub fn list(&self) -> Result<Vec<RoaBasisProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRoaRepository::new(self.scoped.session()).list();
        }
        SqliteRoaRepository::new(self.scoped).list()
    }
}
