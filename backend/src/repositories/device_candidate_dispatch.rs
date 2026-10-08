use crate::repositories::device_candidate::{
    DEVICE_IMPORT_MATCH_CANDIDATES_MAX, SqliteDeviceCandidateRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::device_candidate_postgres::PostgresDeviceCandidateRepository;

pub struct ScopedDeviceCandidateRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedDeviceCandidateRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list_for_import(&self) -> Result<Vec<String>, RepositoryError> {
        let limit = DEVICE_IMPORT_MATCH_CANDIDATES_MAX + 1;
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceCandidateRepository::new(self.scoped.session())
                .list_serials(limit);
        }
        SqliteDeviceCandidateRepository::new(self.scoped).list_serials(limit)
    }
}
