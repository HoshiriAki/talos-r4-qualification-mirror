use std::future::Future;

use sqlx::Row;
use sqlx::postgres::PgPool;
use system_admin::deletion::{DeletionAdminInput, DeletionListInput, DeletionRequestInput};
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use super::RepositoryError;
use super::deletion_compatibility::{
    DeletionCompatibilityError, DeletionCompleteResult, DeletionRecord, DeletionRequestOutcome,
};

#[derive(Clone)]
pub(crate) struct PostgresDeletionCompatibilityRepository {
    pool: PgPool,
}

impl PostgresDeletionCompatibilityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn request(
        &self,
        tenant_id: &str,
        input: &DeletionRequestInput,
        now: &str,
    ) -> Result<DeletionRequestOutcome, DeletionCompatibilityError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_deletion(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            lock_tenant(&mut tx, &tenant_id).await?;
            if let Some(id) = sqlx::query_scalar::<_, String>(
                "SELECT id FROM data_deletion_requests
                 WHERE tenant_id = $1 AND user_id = $2
                   AND status IN ('pending', 'processing')
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&input.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_storage)?
            {
                tx.commit().await.map_err(pg_storage)?;
                return Ok(DeletionRequestOutcome::Existing { id });
            }

            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO data_deletion_requests
                 (id, user_id, request_type, status, reason, requested_at, created_at, tenant_id)
                 VALUES ($1, $2, $3, 'pending', $4, $5, $5, $6)",
            )
            .bind(&id)
            .bind(&input.user_id)
            .bind(&input.request_type)
            .bind(&input.reason)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(DeletionRequestOutcome::Created {
                id,
                requested_at: now,
            })
        })
    }

    pub(crate) fn list(
        &self,
        tenant_id: &str,
        input: &DeletionListInput,
    ) -> Result<Vec<DeletionRecord>, DeletionCompatibilityError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        run_pg_deletion(async move {
            let rows = match input.status.as_deref() {
                Some(status) => {
                    sqlx::query(
                        "SELECT id, user_id, request_type, status, reason, requested_at,
                                completed_at, admin_notes, created_at
                         FROM data_deletion_requests
                         WHERE tenant_id = $1 AND status = $2
                         ORDER BY created_at DESC",
                    )
                    .bind(&tenant_id)
                    .bind(status)
                    .fetch_all(&pool)
                    .await
                }
                None => {
                    sqlx::query(
                        "SELECT id, user_id, request_type, status, reason, requested_at,
                                completed_at, admin_notes, created_at
                         FROM data_deletion_requests
                         WHERE tenant_id = $1
                         ORDER BY created_at DESC",
                    )
                    .bind(&tenant_id)
                    .fetch_all(&pool)
                    .await
                }
            }
            .map_err(pg_storage)?;
            rows.into_iter()
                .map(|row| {
                    Ok(DeletionRecord {
                        id: row.try_get(0).map_err(pg_storage)?,
                        user_id: row.try_get(1).map_err(pg_storage)?,
                        request_type: row.try_get(2).map_err(pg_storage)?,
                        status: row.try_get(3).map_err(pg_storage)?,
                        reason: row.try_get(4).map_err(pg_storage)?,
                        requested_at: row.try_get(5).map_err(pg_storage)?,
                        completed_at: row.try_get(6).map_err(pg_storage)?,
                        admin_notes: row.try_get(7).map_err(pg_storage)?,
                        created_at: row.try_get(8).map_err(pg_storage)?,
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()
                .map_err(DeletionCompatibilityError::Storage)
        })
    }

    pub(crate) fn process(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
    ) -> Result<(), DeletionCompatibilityError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        run_pg_deletion(async move {
            sqlx::query(
                "UPDATE data_deletion_requests
                 SET status = 'processing', admin_notes = $1
                 WHERE tenant_id = $2 AND id = $3",
            )
            .bind(&input.admin_notes)
            .bind(&tenant_id)
            .bind(&input.request_id)
            .execute(&pool)
            .await
            .map_err(pg_storage)?;
            Ok(())
        })
    }

    pub(crate) fn reject(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
        now: &str,
    ) -> Result<(), DeletionCompatibilityError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_deletion(async move {
            sqlx::query(
                "UPDATE data_deletion_requests
                 SET status = 'rejected', completed_at = $1, admin_notes = $2
                 WHERE tenant_id = $3 AND id = $4",
            )
            .bind(&now)
            .bind(&input.admin_notes)
            .bind(&tenant_id)
            .bind(&input.request_id)
            .execute(&pool)
            .await
            .map_err(pg_storage)?;
            Ok(())
        })
    }

    pub(crate) fn complete(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
        now: &str,
    ) -> Result<DeletionCompleteResult, DeletionCompatibilityError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_deletion(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *tx)
                .await
                .map_err(pg_storage)?;
            lock_tenant(&mut tx, &tenant_id).await?;

            let identity_id = sqlx::query_scalar::<_, String>(
                "SELECT user_id FROM data_deletion_requests
                 WHERE tenant_id = $1 AND id = $2
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .bind(&input.request_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(pg_storage)?;
            let target_role = sqlx::query_scalar::<_, String>(
                "SELECT role FROM tenant_memberships
                 WHERE tenant_id = $1 AND identity_id = $2 AND status != 'revoked'
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .bind(&identity_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_storage)?
            .ok_or(DeletionCompatibilityError::MembershipNotFound)?;
            if target_role == "owner" {
                let owner_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM tenant_memberships
                     WHERE tenant_id = $1 AND role = 'owner' AND status = 'active'",
                )
                .bind(&tenant_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(pg_storage)?;
                if owner_count <= 1 {
                    return Err(DeletionCompatibilityError::LastOwner);
                }
            }

            let revoked = sqlx::query(
                "UPDATE tenant_memberships
                 SET status = 'revoked', updated_at = $1
                 WHERE tenant_id = $2 AND identity_id = $3 AND status != 'revoked'",
            )
            .bind(&now)
            .bind(&tenant_id)
            .bind(&identity_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?
            .rows_affected();
            if revoked == 0 {
                return Err(DeletionCompatibilityError::MembershipNotFound);
            }

            let anon_id = format!("ANONYMIZED-{}", Uuid::new_v4());
            let anon_display = format!("已删除用户-{}", &Uuid::new_v4().to_string()[..8]);
            let deleted_email = format!("{anon_id}@deleted.local");
            let identity_anonymized = sqlx::query(
                "UPDATE identities
                 SET username = $1, display_name = $2, email = $3, phone = NULL,
                     password_hash = 'deleted', totp_secret_ciphertext = NULL, totp_enabled = FALSE,
                     status = 'disabled', updated_at = $4
                 WHERE id = $5
                   AND NOT EXISTS (
                     SELECT 1 FROM tenant_memberships tm
                     WHERE tm.identity_id = identities.id AND tm.status != 'revoked'
                   )
                   AND NOT EXISTS (
                     SELECT 1 FROM platform_memberships pm
                     WHERE pm.identity_id = identities.id AND pm.status != 'revoked'
                   )",
            )
            .bind(&anon_id)
            .bind(&anon_display)
            .bind(&deleted_email)
            .bind(&now)
            .bind(&identity_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?
            .rows_affected()
                == 1;
            sqlx::query(
                "UPDATE data_deletion_requests
                 SET status = 'completed', completed_at = $1, admin_notes = $2
                 WHERE tenant_id = $3 AND id = $4",
            )
            .bind(&now)
            .bind(&input.admin_notes)
            .bind(&tenant_id)
            .bind(&input.request_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(DeletionCompleteResult {
                completed_at: now,
                identity_id,
                identity_anonymized,
            })
        })
    }
}

async fn lock_tenant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
) -> Result<(), DeletionCompatibilityError> {
    sqlx::query("SELECT id FROM tenants WHERE id = $1 AND status != 'deleted' FOR UPDATE")
        .bind(tenant_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(pg_storage)?;
    Ok(())
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_deletion<T, F>(future: F) -> Result<T, DeletionCompatibilityError>
where
    F: Future<Output = Result<T, DeletionCompatibilityError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("deletion compatibility runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "deletion compatibility requires the multi-thread runtime".into(),
        )
        .into());
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
