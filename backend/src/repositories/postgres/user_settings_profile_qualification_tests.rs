use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::UserSettingsProfileRepository;

struct LiveUserSettingsFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveUserSettingsFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_user_settings_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(4)
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

        sqlx::query(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES
             ('identity-settings-a','identity-settings-a','hash','Settings A','active','now','now'),
             ('identity-settings-b','identity-settings-b','hash','Settings B','active','now','now')",
        )
        .execute(&pool)
        .await?;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_user_settings_profile_preserves_identity_global_upsert_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveUserSettingsFixture::create().await?;
    let repository = UserSettingsProfileRepository::postgres(fixture.pool.clone());

    assert_eq!(
        repository.get("identity-settings-a")?,
        serde_json::json!({})
    );

    repository.save(
        "identity-settings-a",
        &serde_json::json!({
            "sidebarOrder": ["orders", "devices"],
            "hiddenPaths": ["/debug"],
            "homePage": "/orders",
            "theme": "dark",
        }),
        "2026-09-25T11:00:00+08:00",
    )?;

    let (raw, legacy_tenant_marker): (String, Option<String>) = sqlx::query_as(
        "SELECT settingsJson::text, tenant_id
         FROM user_settings
         WHERE userId = 'identity-settings-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let stored: serde_json::Value = serde_json::from_str(&raw)?;
    assert_eq!(stored["theme"], "dark");
    assert!(
        legacy_tenant_marker.is_none(),
        "new identity-global settings must not manufacture tenant authority"
    );

    repository.save(
        "identity-settings-a",
        &serde_json::json!({
            "sidebarOrder": ["devices"],
            "hiddenPaths": [],
            "homePage": "/devices",
            "theme": "light",
        }),
        "2026-09-25T11:01:00+08:00",
    )?;

    let updated = repository.get("identity-settings-a")?;
    assert_eq!(updated["theme"], "light");
    assert_eq!(updated["homePage"], "/devices");
    assert_eq!(
        repository.get("identity-settings-b")?,
        serde_json::json!({}),
        "another identity must not inherit the first identity profile"
    );

    let recomposed = UserSettingsProfileRepository::postgres(fixture.pool.clone());
    assert_eq!(recomposed.get("identity-settings-a")?, updated);

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM user_settings
         WHERE userId = 'identity-settings-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(row_count, 1, "save must remain an identity-keyed upsert");

    fixture.cleanup().await
}
