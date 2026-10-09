#![cfg(all(test, feature = "postgres"))]

use sqlx::postgres::PgPoolOptions;

#[tokio::test]
#[ignore = "requires TALOS_P10_RESTORE_DATABASE_URL for a restored PostgreSQL 18 database"]
async fn restored_pg18_migration_chain_is_idempotent() -> anyhow::Result<()> {
    let database_url = std::env::var("TALOS_P10_RESTORE_DATABASE_URL")?;
    let expected_latest = std::env::var("TALOS_P10_EXPECTED_MIGRATION_ID")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;

    let before_count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM schema_migrations")
        .fetch_one(&pool)
        .await?;
    let before_latest: String =
        sqlx::query_scalar("SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await?;

    anyhow::ensure!(
        before_count > 0,
        "restored migration registry must not be empty"
    );
    anyhow::ensure!(
        before_latest == expected_latest,
        "restored migration registry latest id is unexpected: expected={expected_latest} actual={before_latest}"
    );

    let first = crate::db::run_all_pg_migrations(&pool).await?;
    anyhow::ensure!(
        first.is_empty(),
        "first restored migration verification must execute zero migrations: {first:?}"
    );

    let after_first_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM schema_migrations")
            .fetch_one(&pool)
            .await?;
    let after_first_latest: String =
        sqlx::query_scalar("SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await?;
    anyhow::ensure!(
        after_first_count == before_count && after_first_latest == before_latest,
        "migration registry changed after first idempotency verification"
    );

    let second = crate::db::run_all_pg_migrations(&pool).await?;
    anyhow::ensure!(
        second.is_empty(),
        "second restored migration verification must execute zero migrations: {second:?}"
    );

    let after_second_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM schema_migrations")
            .fetch_one(&pool)
            .await?;
    let after_second_latest: String =
        sqlx::query_scalar("SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await?;

    anyhow::ensure!(
        after_second_count == before_count && after_second_latest == before_latest,
        "migration registry changed after second idempotency verification"
    );

    println!(
        "P10_RESTORE_MIGRATION_IDEMPOTENT migration_count={} latest_migration={} first_executed=0 second_executed=0",
        before_count, before_latest
    );

    pool.close().await;
    Ok(())
}
