#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::device_candidate::DEVICE_IMPORT_MATCH_CANDIDATES_MAX;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresDeviceCandidateRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresDeviceCandidateRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
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
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::ContractViolation("device candidate limit is invalid".into())
        })?;
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT serialNo AS serial_no FROM devices
                     WHERE tenant_id = $1
                     ORDER BY serialNo
                     LIMIT $2",
                )
                .bind(tenant_id)
                .bind(limit)
                .fetch_all(&mut *connection)
                .await?;
                rows.iter()
                    .map(|row| row.try_get::<String, _>("serial_no"))
                    .collect()
            })
        })
    }
}
