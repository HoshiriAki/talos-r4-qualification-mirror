use rusqlite::params;

use crate::repositories::{RepositoryError, ScopedRepositories};

pub const DEVICE_IMPORT_MATCH_CANDIDATES_MAX: usize = 50_000;

pub(in crate::repositories) struct SqliteDeviceCandidateRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> SqliteDeviceCandidateRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub(in crate::repositories) fn list_serials(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, RepositoryError> {
        if limit == 0 || limit > DEVICE_IMPORT_MATCH_CANDIDATES_MAX + 1 {
            return Err(RepositoryError::ContractViolation(
                "device candidate limit exceeds the bounded import contract".into(),
            ));
        }
        let tenant_id = self.scoped.binding().tenant_id().as_str().to_owned();
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::ContractViolation("device candidate limit is invalid".into())
        })?;
        self.scoped.session().read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT serialNo FROM devices
                 WHERE tenant_id = ?1
                 ORDER BY serialNo
                 LIMIT ?2",
            )?;
            statement
                .query_map(params![tenant_id, limit], |row| row.get::<_, String>(0))?
                .collect()
        })
    }
}
