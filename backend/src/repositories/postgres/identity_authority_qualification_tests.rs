use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{DataScope, Revision, TenantId};

use crate::repositories::IdentityAuthorityRepository;

struct LiveIdentityAuthorityFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveIdentityAuthorityFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_identity_authority_{}", uuid::Uuid::new_v4().simple());
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

        for (id, name, slug) in [
            ("tenant-a", "Tenant A", "tenant-a"),
            ("tenant-b", "Tenant B", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
                 VALUES ($1,$2,$3,'active','test','now','now')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }

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

fn scope(tenant: &str) -> DataScope {
    DataScope::production(
        TenantId::new(tenant).unwrap(),
        Revision::new("identity-authority-live").unwrap(),
    )
    .unwrap()
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_identity_authority_preserves_scope_roles_sessions_and_expiry()
-> anyhow::Result<()> {
    let fixture = LiveIdentityAuthorityFixture::create().await?;

    sqlx::query(
        "INSERT INTO identities
         (id,username,password_hash,display_name,status,created_at,updated_at)
         VALUES
         ('tenant-identity','alice','tenant-hash','Alice','active','now','now'),
         ('platform-identity','root','platform-hash','Root','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO tenant_memberships
         (id,identity_id,tenant_id,role,status,created_at,updated_at)
         VALUES
         ('tenant-member-a','tenant-identity','tenant-a','admin','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO platform_memberships
         (id,identity_id,status,created_at,updated_at)
         VALUES ('platform-member','platform-identity','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO platform_role_grants
         (id,platform_membership_id,role,granted_by_identity_id,granted_at)
         VALUES
         ('platform-role-owner','platform-member','platform_owner','platform-identity','now'),
         ('platform-role-auditor','platform-member','security_auditor','platform-identity','now')",
    )
    .execute(&fixture.pool)
    .await?;

    let tenant_token = "tenant-token";
    let platform_token = "platform-token";
    let expired_token = "expired-token";
    sqlx::query(
        "INSERT INTO auth_sessions
         (id,token_hash,identity_id,auth_strength,created_at,last_seen_at,expires_at)
         VALUES
         ('tenant-session',$1,'tenant-identity','mfa','2026-09-19T00:00:00Z','2000-01-01T00:00:00Z','2099-01-01T00:00:00Z'),
         ('platform-session',$2,'platform-identity','mfa','2026-09-19T00:00:00Z','2000-01-01T00:00:00Z','2099-01-01T00:00:00Z'),
         ('expired-session',$3,'tenant-identity','password','2000-01-01T00:00:00Z','2000-01-01T00:00:00Z','2000-01-02T00:00:00Z')",
    )
    .bind(token_hash(tenant_token))
    .bind(token_hash(platform_token))
    .bind(token_hash(expired_token))
    .execute(&fixture.pool)
    .await?;

    let repository = IdentityAuthorityRepository::postgres(fixture.pool.clone());

    let tenant_account = repository
        .find_user_by_username(&scope("tenant-a"), "alice")?
        .expect("tenant identity must resolve in its own scope");
    assert_eq!(tenant_account.id, "tenant-identity");
    assert_eq!(tenant_account.authority_role, "admin");
    assert_eq!(tenant_account.tenant_scope_id.as_deref(), Some("tenant-a"));
    assert!(
        repository
            .find_user_by_username(&scope("tenant-b"), "alice")?
            .is_none()
    );

    let platform_account = repository
        .find_platform_identity_by_username("root")?
        .expect("platform identity must resolve");
    assert_eq!(platform_account.id, "platform-identity");
    assert_eq!(platform_account.authority_role, "platform_owner");

    repository.record_successful_login("tenant-identity", "2026-09-20T05:55:00Z")?;
    let recorded_last_login: Option<String> =
        sqlx::query_scalar("SELECT last_login_at FROM identities WHERE id='tenant-identity'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(recorded_last_login.as_deref(), Some("2026-09-20T05:55:00Z"));

    let updated_profile = repository.update_identity_profile(
        "tenant-identity",
        Some("Alice Updated"),
        Some("alice.updated@example.test"),
        Some("+1-555-0100"),
        "2026-09-20T05:56:00Z",
    )?;
    assert_eq!(updated_profile.display_name, "Alice Updated");
    assert_eq!(updated_profile.email, "alice.updated@example.test");
    assert_eq!(updated_profile.phone, "+1-555-0100");
    let persisted_profile: (String, String, String, String) = sqlx::query_as(
        "SELECT display_name, COALESCE(email, ''), COALESCE(phone, ''), updated_at
         FROM identities WHERE id='tenant-identity'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(
        persisted_profile,
        (
            "Alice Updated".to_string(),
            "alice.updated@example.test".to_string(),
            "+1-555-0100".to_string(),
            "2026-09-20T05:56:00Z".to_string(),
        )
    );

    let tenant_auth = repository
        .get_auth_user(tenant_token, Some("tenant-a"))?
        .expect("tenant bearer must resolve");
    assert_eq!(tenant_auth.tenant_id(), Some("tenant-a"));
    assert_eq!(tenant_auth.authority_label(), "admin");
    assert!(
        repository
            .get_auth_user(tenant_token, Some("tenant-b"))?
            .is_none()
    );

    let platform_auth = repository
        .get_auth_user(platform_token, None)?
        .expect("platform bearer must resolve");
    assert!(platform_auth.tenant_id().is_none());
    assert!(
        platform_auth
            .platform_roles()
            .iter()
            .any(|role| format!("{role:?}") == "Owner")
    );

    let tenant_last_seen: String =
        sqlx::query_scalar("SELECT last_seen_at FROM auth_sessions WHERE id='tenant-session'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_ne!(tenant_last_seen, "2000-01-01T00:00:00Z");

    assert!(
        repository
            .get_auth_user(expired_token, Some("tenant-a"))?
            .is_none()
    );
    let expired_remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions WHERE id='expired-session'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(expired_remaining, 0);

    fixture.cleanup().await
}
