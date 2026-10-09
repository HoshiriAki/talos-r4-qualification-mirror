pub mod baseline;
pub mod migrations;
#[cfg(feature = "postgres")]
pub mod migrations_pg;
#[cfg(all(test, feature = "postgres"))]
mod p10_restore_qualification;
#[cfg(all(test, feature = "postgres"))]
mod p8_pg18_qualification;
pub mod pool;
pub mod query;
mod r4_migrations;

#[cfg(feature = "sqlite")]
pub fn run_all_sqlite_migrations(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    let mut executed = migrations::run_migrations(conn)?;
    executed.extend(r4_migrations::run_sqlite_extension_067(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_068(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_069(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_070(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_072(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_073(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_074(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_075(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_076(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_079(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_080(conn)?);
    executed.extend(r4_migrations::run_sqlite_extension_081(conn)?);
    Ok(executed)
}

#[cfg(feature = "postgres")]
pub async fn run_all_pg_migrations(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    let mut executed = migrations_pg::run_pg_migrations(pool).await?;
    executed.extend(r4_migrations::run_pg_extension_067(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_068(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_069(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_070(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_071(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_072(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_073(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_074(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_075(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_076(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_077(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_078(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_079(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_080(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_081(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_082(pool).await?);
    executed.extend(r4_migrations::run_pg_extension_083(pool).await?);
    Ok(executed)
}

// Re-exports for Phase B+ (PostgreSQL migration) — used by services/routes
#[allow(unused_imports)]
pub use query::{PostgresDialect, QueryBuilder};
