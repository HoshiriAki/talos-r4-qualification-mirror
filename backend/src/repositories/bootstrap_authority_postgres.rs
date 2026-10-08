use std::future::Future;

use sqlx::postgres::PgPool;
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use super::RepositoryError;
use super::bootstrap_authority::{BootstrapPlatformOwnerCommand, BootstrapPlatformOwnerOutcome};

#[derive(Clone)]
pub(crate) struct PostgresBootstrapAuthorityRepository {
    pool: PgPool,
}

impl PostgresBootstrapAuthorityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn has_identities(&self) -> Result<bool, RepositoryError> {
        let pool = self.pool.clone();
        run_pg_bootstrap(async move {
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM identities)")
                .fetch_one(&pool)
                .await
                .map_err(pg_storage)
        })
    }

    pub(crate) fn ensure_initial_platform_owner(
        &self,
        command: BootstrapPlatformOwnerCommand,
    ) -> Result<BootstrapPlatformOwnerOutcome, RepositoryError> {
        let pool = self.pool.clone();
        run_pg_bootstrap(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *tx)
                .await
                .map_err(pg_storage)?;
            sqlx::query(
                "LOCK TABLE identities, platform_memberships, platform_role_grants
                 IN SHARE ROW EXCLUSIVE MODE",
            )
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;

            let initialized: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identities)")
                .fetch_one(&mut *tx)
                .await
                .map_err(pg_storage)?;
            if initialized {
                tx.commit().await.map_err(pg_storage)?;
                return Ok(BootstrapPlatformOwnerOutcome::AlreadyInitialized);
            }

            let identity_id = Uuid::new_v4().to_string();
            let membership_id = Uuid::new_v4().to_string();
            let grant_id = Uuid::new_v4().to_string();

            sqlx::query(
                "INSERT INTO identities
                 (id, username, password_hash, display_name, status, created_at, updated_at)
                 VALUES ($1, $2, $3, $2, 'active', $4, $4)",
            )
            .bind(&identity_id)
            .bind(&command.username)
            .bind(&command.password_hash)
            .bind(&command.now)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            sqlx::query(
                "INSERT INTO platform_memberships
                 (id, identity_id, status, created_at, updated_at)
                 VALUES ($1, $2, 'active', $3, $3)",
            )
            .bind(&membership_id)
            .bind(&identity_id)
            .bind(&command.now)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            sqlx::query(
                "INSERT INTO platform_role_grants
                 (id, platform_membership_id, role, granted_by_identity_id, granted_at)
                 VALUES ($1, $2, 'platform_owner', $3, $4)",
            )
            .bind(&grant_id)
            .bind(&membership_id)
            .bind(&identity_id)
            .bind(&command.now)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;

            tx.commit().await.map_err(pg_storage)?;
            Ok(BootstrapPlatformOwnerOutcome::Created)
        })
    }
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_bootstrap<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("bootstrap authority runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "bootstrap authority requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
