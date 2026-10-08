use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::ConsentCompatibilityRepository;
use system_admin::consent::{ConsentCheckInput, ConsentRecordInput, ConsentRevokeInput};

struct LiveConsentFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveConsentFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_consent_{}", uuid::Uuid::new_v4().simple());
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
             VALUES ('consent-user','consent-user','!test','Consent User','active','01','01')",
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

fn record_input(version: &str) -> ConsentRecordInput {
    ConsentRecordInput {
        user_id: "consent-user".into(),
        consent_type: "tos".into(),
        version: version.into(),
        ip_address: "127.0.0.1".into(),
        user_agent: "qualification".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_consent_compatibility_preserves_scope_record_check_revoke_and_audit()
-> anyhow::Result<()> {
    let fixture = LiveConsentFixture::create().await?;
    let repository = ConsentCompatibilityRepository::postgres(fixture.pool.clone());

    let tenant_a = repository.record("tenant-a", &record_input("v1"), "01")?;
    let tenant_b = repository.record("tenant-b", &record_input("v1"), "02")?;
    assert_ne!(tenant_a.id, tenant_b.id);

    let check = ConsentCheckInput {
        user_id: "consent-user".into(),
        consent_type: "tos".into(),
        version: "v1".into(),
    };
    assert!(repository.check("tenant-a", &check)?);
    assert!(repository.check("tenant-b", &check)?);
    assert!(!repository.check("tenant-c", &check)?);

    let revoke = ConsentRevokeInput {
        user_id: "consent-user".into(),
        consent_type: "tos".into(),
    };
    let revoked = repository.revoke("tenant-a", &revoke, "03")?;
    assert_eq!(revoked.affected, 1);
    assert_eq!(revoked.revoked_at, "03");
    assert!(!repository.check("tenant-a", &check)?);
    assert!(repository.check("tenant-b", &check)?);

    let audit_a = repository.audit("tenant-a", "consent-user")?;
    let audit_b = repository.audit("tenant-b", "consent-user")?;
    assert_eq!(audit_a.len(), 1);
    assert_eq!(audit_b.len(), 1);
    assert!(!audit_a[0].consented);
    assert_eq!(audit_a[0].revoked_at.as_deref(), Some("03"));
    assert!(audit_b[0].consented);
    assert!(audit_b[0].revoked_at.is_none());

    fixture.cleanup().await
}
