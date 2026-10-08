use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::{
    BootstrapAuthorityRepository, BootstrapPlatformOwnerCommand, BootstrapPlatformOwnerOutcome,
};

struct LiveBootstrapFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveBootstrapFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_bootstrap_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .after_connect(move |connection, _| {
                let schema = schema_for_pool.clone();
                Box::pin(async move {
                    connection
                        .execute(format!("SET search_path TO {schema}").as_str())
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await?;
        crate::db::run_all_pg_migrations(&pool).await?;

        Ok(Self {
            database_url,
            schema,
            pool,
        })
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        self.pool.close().await;
        let mut admin = PgConnection::connect(&self.database_url).await?;
        admin
            .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
            .await?;
        Ok(())
    }
}

fn command(username: &str, now: &str) -> BootstrapPlatformOwnerCommand {
    BootstrapPlatformOwnerCommand {
        username: username.into(),
        password_hash: "scrypt$".to_owned() + username,
        now: now.into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_bootstrap_authority_serializes_initial_owner_creation_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveBootstrapFixture::create().await?;
    let repository_a = BootstrapAuthorityRepository::postgres(fixture.pool.clone());
    let repository_b = repository_a.clone();

    assert!(!repository_a.has_identities()?);

    let first = tokio::spawn(async move {
        repository_a.ensure_initial_platform_owner(command("bootstrap-a", "01"))
    });
    let second = tokio::spawn(async move {
        repository_b.ensure_initial_platform_owner(command("bootstrap-b", "02"))
    });

    let first = first.await??;
    let second = second.await??;
    let created = usize::from(matches!(first, BootstrapPlatformOwnerOutcome::Created))
        + usize::from(matches!(second, BootstrapPlatformOwnerOutcome::Created));
    let skipped = usize::from(matches!(
        first,
        BootstrapPlatformOwnerOutcome::AlreadyInitialized
    )) + usize::from(matches!(
        second,
        BootstrapPlatformOwnerOutcome::AlreadyInitialized
    ));
    assert_eq!(created, 1);
    assert_eq!(skipped, 1);

    let identity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identities")
        .fetch_one(&fixture.pool)
        .await?;
    let membership_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM platform_memberships")
        .fetch_one(&fixture.pool)
        .await?;
    let owner_grant_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM platform_role_grants WHERE role = 'platform_owner'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(identity_count, 1);
    assert_eq!(membership_count, 1);
    assert_eq!(owner_grant_count, 1);

    let recomposed = BootstrapAuthorityRepository::postgres(fixture.pool.clone());
    assert!(recomposed.has_identities()?);
    assert_eq!(
        recomposed.ensure_initial_platform_owner(command("bootstrap-c", "03"))?,
        BootstrapPlatformOwnerOutcome::AlreadyInitialized
    );
    let final_identity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identities")
        .fetch_one(&fixture.pool)
        .await?;
    assert_eq!(final_identity_count, 1);

    fixture.cleanup().await
}
