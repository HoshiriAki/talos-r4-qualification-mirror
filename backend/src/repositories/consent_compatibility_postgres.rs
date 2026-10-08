use std::future::Future;

use sqlx::Row;
use sqlx::postgres::PgPool;
use system_admin::consent::{ConsentCheckInput, ConsentRecordInput, ConsentRevokeInput};
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use super::RepositoryError;
use super::consent_compatibility::{ConsentAuditRecord, ConsentRecordResult, ConsentRevokeResult};

#[derive(Clone)]
pub(crate) struct PostgresConsentCompatibilityRepository {
    pool: PgPool,
}

impl PostgresConsentCompatibilityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn record(
        &self,
        tenant_id: &str,
        input: &ConsentRecordInput,
        now: &str,
    ) -> Result<ConsentRecordResult, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_consent(async move {
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO privacy_consents
                 (id, user_id, consent_type, version, consented, ip_address, user_agent,
                  created_at, tenant_id)
                 VALUES ($1, $2, $3, $4, 1, $5, $6, $7, $8)",
            )
            .bind(&id)
            .bind(&input.user_id)
            .bind(&input.consent_type)
            .bind(&input.version)
            .bind(&input.ip_address)
            .bind(&input.user_agent)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&pool)
            .await
            .map_err(pg_storage)?;
            Ok(ConsentRecordResult {
                id,
                created_at: now,
            })
        })
    }

    pub(crate) fn check(
        &self,
        tenant_id: &str,
        input: &ConsentCheckInput,
    ) -> Result<bool, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        run_pg_consent(async move {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(
                    SELECT 1 FROM privacy_consents
                    WHERE tenant_id = $1 AND user_id = $2 AND consent_type = $3
                      AND version = $4 AND consented = 1 AND revoked_at IS NULL
                    ORDER BY created_at DESC LIMIT 1
                )",
            )
            .bind(&tenant_id)
            .bind(&input.user_id)
            .bind(&input.consent_type)
            .bind(&input.version)
            .fetch_one(&pool)
            .await
            .map_err(pg_storage)
        })
    }

    pub(crate) fn revoke(
        &self,
        tenant_id: &str,
        input: &ConsentRevokeInput,
        now: &str,
    ) -> Result<ConsentRevokeResult, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_consent(async move {
            let affected = sqlx::query(
                "UPDATE privacy_consents
                 SET consented = 0, revoked_at = $1
                 WHERE tenant_id = $2 AND user_id = $3 AND consent_type = $4
                   AND revoked_at IS NULL",
            )
            .bind(&now)
            .bind(&tenant_id)
            .bind(&input.user_id)
            .bind(&input.consent_type)
            .execute(&pool)
            .await
            .map_err(pg_storage)?
            .rows_affected();
            let affected = usize::try_from(affected).map_err(|_| {
                RepositoryError::ContractViolation(
                    "consent revoke affected-row count overflow".into(),
                )
            })?;
            Ok(ConsentRevokeResult {
                affected,
                revoked_at: now,
            })
        })
    }

    pub(crate) fn audit(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<Vec<ConsentAuditRecord>, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let user_id = user_id.to_owned();
        run_pg_consent(async move {
            let rows = sqlx::query(
                "SELECT id, user_id, consent_type, version, consented, ip_address,
                        user_agent, revoked_at, created_at
                 FROM privacy_consents
                 WHERE tenant_id = $1 AND user_id = $2
                 ORDER BY created_at DESC",
            )
            .bind(&tenant_id)
            .bind(&user_id)
            .fetch_all(&pool)
            .await
            .map_err(pg_storage)?;
            rows.into_iter()
                .map(|row| {
                    Ok(ConsentAuditRecord {
                        id: row.try_get(0).map_err(pg_storage)?,
                        user_id: row.try_get(1).map_err(pg_storage)?,
                        consent_type: row.try_get(2).map_err(pg_storage)?,
                        version: row.try_get(3).map_err(pg_storage)?,
                        consented: row.try_get::<i32, _>(4).map_err(pg_storage)? == 1,
                        ip_address: row.try_get(5).map_err(pg_storage)?,
                        user_agent: row.try_get(6).map_err(pg_storage)?,
                        revoked_at: row.try_get(7).map_err(pg_storage)?,
                        created_at: row.try_get(8).map_err(pg_storage)?,
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()
        })
    }
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_consent<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("consent compatibility runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "consent compatibility requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
