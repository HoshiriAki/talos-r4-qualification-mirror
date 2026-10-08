use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantResolutionRecord {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub plan: String,
    pub settings: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone)]
pub(crate) struct SqliteTenantResolutionRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteTenantResolutionRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn resolve_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<TenantResolutionRecord>, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.query_row(
            "SELECT id,name,slug,status,plan,settings,created_at,updated_at
             FROM tenants WHERE slug=?1",
            [slug],
            |row| {
                Ok(TenantResolutionRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    slug: row.get(2)?,
                    status: row.get(3)?,
                    plan: row.get(4)?,
                    settings: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|error| RepositoryError::Sqlite(error.to_string()))
    }
}
