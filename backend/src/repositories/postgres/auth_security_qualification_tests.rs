use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::{
    AuthSecurityRepository, StoredRateState, TotpCredentialRewrap, TotpDisableOutcome,
    TotpEnrollmentOutcome, TotpStatus,
};

struct LiveAuthSecurityFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveAuthSecurityFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_auth_security_{}", uuid::Uuid::new_v4().simple());
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
async fn live_pg18_auth_security_preserves_rate_session_revocation_and_totp_cas()
-> anyhow::Result<()> {
    let fixture = LiveAuthSecurityFixture::create().await?;
    let repository = AuthSecurityRepository::postgres(fixture.pool.clone());

    let rate_key = "a".repeat(64);
    repository.mutate_rate_state(&rate_key, 1_000, 0, |_| {
        (
            StoredRateState {
                failure_count: 3,
                window_started_at_ms: 900,
                blocked_until_ms: 2_000,
            },
            (),
        )
    })?;
    assert_eq!(repository.blocked_until_ms(&rate_key)?, Some(2_000));

    let account_key = "b".repeat(64);
    let source_key = "c".repeat(64);
    repository.mutate_rate_states(
        &[account_key.as_str(), source_key.as_str()],
        1_100,
        0,
        |index, _| StoredRateState {
            failure_count: (index + 1) as i64,
            window_started_at_ms: 1_000,
            blocked_until_ms: 0,
        },
    )?;
    let durable_rate_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_rate_limit_state")
        .fetch_one(&fixture.pool)
        .await?;
    assert_eq!(durable_rate_rows, 3);

    let recomposed = AuthSecurityRepository::postgres(fixture.pool.clone());
    assert_eq!(recomposed.blocked_until_ms(&rate_key)?, Some(2_000));
    recomposed.clear_rate_state(&rate_key)?;
    assert_eq!(recomposed.blocked_until_ms(&rate_key)?, None);

    sqlx::query(
        "INSERT INTO identities
         (id,username,password_hash,display_name,totp_secret_ciphertext,totp_enabled,status,created_at,updated_at)
         VALUES ('identity-auth-pg','auth-pg','old-hash','Auth PG','v1:old',TRUE,'active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;

    recomposed.insert_session(
        "session-a",
        &"1".repeat(64),
        "identity-auth-pg",
        "password",
        "2026-09-19T00:00:00Z",
        "2026-09-26T00:00:00Z",
    )?;
    recomposed.insert_session(
        "session-b",
        &"2".repeat(64),
        "identity-auth-pg",
        "mfa",
        "2026-09-19T00:00:01Z",
        "2026-09-26T00:00:01Z",
    )?;

    let revoked = recomposed.rotate_password_and_revoke_sessions(
        "identity-auth-pg",
        "new-hash",
        "2026-09-19T00:01:00Z",
    )?;
    assert_eq!(revoked, 2);
    let remaining_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM auth_sessions WHERE identity_id='identity-auth-pg'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(remaining_sessions, 0);
    let stored_password: String =
        sqlx::query_scalar("SELECT password_hash FROM identities WHERE id='identity-auth-pg'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(stored_password, "new-hash");

    recomposed.insert_session(
        "session-delete",
        &"3".repeat(64),
        "identity-auth-pg",
        "password",
        "2026-09-19T00:03:00Z",
        "2026-09-26T00:03:00Z",
    )?;
    recomposed.delete_session_by_token_hash(&"3".repeat(64))?;
    let deleted_session_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions WHERE id='session-delete'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(deleted_session_rows, 0);

    for (id, token_hash) in [("session-c", "4".repeat(64)), ("session-d", "5".repeat(64))] {
        recomposed.insert_session(
            id,
            &token_hash,
            "identity-auth-pg",
            "password",
            "2026-09-19T00:04:00Z",
            "2026-09-26T00:04:00Z",
        )?;
    }
    assert_eq!(
        recomposed.delete_sessions_by_identity("identity-auth-pg")?,
        2
    );

    recomposed.insert_session(
        "session-expired",
        &"6".repeat(64),
        "identity-auth-pg",
        "password",
        "2026-09-18T00:00:00Z",
        "2026-09-19T00:00:00Z",
    )?;
    recomposed.insert_session(
        "session-future",
        &"7".repeat(64),
        "identity-auth-pg",
        "password",
        "2026-09-19T00:00:00Z",
        "2026-09-26T00:00:00Z",
    )?;
    assert_eq!(
        recomposed.cleanup_expired_sessions("2026-09-20T00:00:00Z")?,
        1
    );
    let future_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions WHERE id='session-future'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(future_sessions, 1);

    let verified = recomposed.with_totp_credential("identity-auth-pg", |enabled, envelope| {
        assert!(enabled);
        assert_eq!(envelope.as_deref(), Some("v1:old"));
        Ok((
            "verified",
            Some(TotpCredentialRewrap {
                expected_envelope: "v1:old".into(),
                replacement_envelope: "v1:new".into(),
                updated_at: "2026-09-19T00:02:00Z".into(),
            }),
        ))
    })?;
    assert_eq!(verified, "verified");
    let rewrapped: String = sqlx::query_scalar(
        "SELECT totp_secret_ciphertext FROM identities WHERE id='identity-auth-pg'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(rewrapped, "v1:new");

    sqlx::query(
        "INSERT INTO tenants
         (id,name,slug,status,plan,settings,created_at,updated_at)
         VALUES ('tenant-mfa','Tenant MFA','tenant-mfa','active','free',NULL,'now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO identities
         (id,username,password_hash,display_name,totp_enabled,status,created_at,updated_at)
         VALUES ('identity-mfa','identity-mfa','hash','Identity MFA',FALSE,'active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO tenant_memberships
         (id,identity_id,tenant_id,role,status,created_at,updated_at)
         VALUES ('membership-mfa','identity-mfa','tenant-mfa','staff','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;

    assert_eq!(
        recomposed.scoped_totp_status("tenant-mfa", "identity-mfa")?,
        TotpStatus {
            enabled: false,
            has_secret: false,
        }
    );
    assert_eq!(
        recomposed.install_pending_totp_secret(
            "tenant-mfa",
            "identity-mfa",
            "v1:pending",
            "2026-09-19T01:00:00Z",
        )?,
        TotpEnrollmentOutcome::Installed
    );
    assert_eq!(
        recomposed.scoped_totp_status("tenant-mfa", "identity-mfa")?,
        TotpStatus {
            enabled: false,
            has_secret: true,
        }
    );

    let enabled = recomposed.verify_and_enable_scoped_totp(
        "tenant-mfa",
        "identity-mfa",
        "2026-09-19T01:01:00Z",
        |envelope| {
            assert_eq!(envelope, "v1:pending");
            Ok(("enabled", Some("v1:enabled".into())))
        },
    )?;
    assert_eq!(enabled, "enabled");
    assert_eq!(
        recomposed.scoped_totp_status("tenant-mfa", "identity-mfa")?,
        TotpStatus {
            enabled: true,
            has_secret: true,
        }
    );
    assert_eq!(
        recomposed.install_pending_totp_secret(
            "tenant-mfa",
            "identity-mfa",
            "v1:replacement",
            "2026-09-19T01:02:00Z",
        )?,
        TotpEnrollmentOutcome::AlreadyEnabled
    );

    sqlx::query(
        "INSERT INTO platform_memberships
         (id,identity_id,status,created_at,updated_at)
         VALUES ('platform-mfa','identity-mfa','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    assert_eq!(
        recomposed.disable_scoped_totp(
            "tenant-mfa",
            Some("tenant-admin"),
            "identity-mfa",
            "2026-09-19T01:03:00Z",
        )?,
        TotpDisableOutcome::SharedIdentityForbidden
    );
    assert_eq!(
        recomposed.scoped_totp_status("tenant-mfa", "identity-mfa")?,
        TotpStatus {
            enabled: true,
            has_secret: true,
        }
    );
    assert_eq!(
        recomposed.disable_scoped_totp(
            "tenant-mfa",
            Some("identity-mfa"),
            "identity-mfa",
            "2026-09-19T01:04:00Z",
        )?,
        TotpDisableOutcome::Disabled
    );
    assert_eq!(
        recomposed.scoped_totp_status("tenant-mfa", "identity-mfa")?,
        TotpStatus {
            enabled: false,
            has_secret: false,
        }
    );

    fixture.cleanup().await
}
