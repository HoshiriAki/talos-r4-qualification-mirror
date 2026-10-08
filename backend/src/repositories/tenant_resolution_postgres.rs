#[cfg(feature = "postgres")]
use std::future::Future;

use sqlx::Row;
use sqlx::postgres::PgPool;
use tokio::runtime::{Handle, RuntimeFlavor};

use super::RepositoryError;
use super::tenant_resolution::TenantResolutionRecord;

#[derive(Clone)]
pub(crate) struct PostgresTenantResolutionRepository {
    pool: PgPool,
}

impl PostgresTenantResolutionRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn resolve_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<TenantResolutionRecord>, RepositoryError> {
        let pool = self.pool.clone();
        let slug = slug.to_owned();
        run_pg_tenant_resolution(async move {
            let row = sqlx::query(
                "SELECT id,name,slug,status,plan,settings,created_at,updated_at
                 FROM tenants WHERE slug=$1",
            )
            .bind(slug)
            .fetch_optional(&pool)
            .await
            .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            row.map(|row| {
                Ok(TenantResolutionRecord {
                    id: row.try_get(0).map_err(pg_decode)?,
                    name: row.try_get(1).map_err(pg_decode)?,
                    slug: row.try_get(2).map_err(pg_decode)?,
                    status: row.try_get(3).map_err(pg_decode)?,
                    plan: row.try_get(4).map_err(pg_decode)?,
                    settings: row.try_get(5).map_err(pg_decode)?,
                    created_at: row.try_get(6).map_err(pg_decode)?,
                    updated_at: row.try_get(7).map_err(pg_decode)?,
                })
            })
            .transpose()
        })
    }
}

fn run_pg_tenant_resolution<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("tenant resolution runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "tenant resolution requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn pg_decode<T>(error: T) -> RepositoryError
where
    T: std::fmt::Display,
{
    RepositoryError::Postgres(error.to_string())
}
