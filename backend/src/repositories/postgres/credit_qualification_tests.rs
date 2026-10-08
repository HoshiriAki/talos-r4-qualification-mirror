use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{CreditMutationError, PostgresRepositoryProvider, RepositoryProvider};

struct LiveCreditFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveCreditFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_credit_{}", uuid::Uuid::new_v4().simple());
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
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
             VALUES
             ('tenant-credit-a','Credit A','tenant-credit-a','active','test','now','now'),
             ('tenant-credit-b','Credit B','tenant-credit-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        for identity in [
            "credit-admin-a",
            "credit-admin-b",
            "credit-reporter-a",
            "credit-runtime-actor",
        ] {
            sqlx::query(
                "INSERT INTO identities
                 (id,username,password_hash,display_name,status,created_at,updated_at)
                 VALUES ($1,$1,'!non-interactive',$1,'active','now','now')",
            )
            .bind(identity)
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

fn context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("credit-runtime-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("credit-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_credit_authority_preserves_scope_atomic_score_identity_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveCreditFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-credit-a", "credit-a");
    let ctx_b = context("tenant-credit-b", "credit-b");
    let phone = "13800000000";

    let (black_a_id, violation_id) = {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let black_a = scoped_a.credits().blacklist_add(
            "Alice",
            phone,
            None,
            "critical risk",
            "critical",
            "credit-admin-a",
            "2026-09-30 14:10:00",
        )?;
        let black_b = scoped_b.credits().blacklist_add(
            "Bob",
            phone,
            None,
            "watch",
            "low",
            "credit-admin-b",
            "2026-09-30 14:10:01",
        )?;
        assert_ne!(black_a.id, black_b.id);

        let duplicate = scoped_a.credits().blacklist_add(
            "Alice",
            phone,
            None,
            "again",
            "high",
            "credit-admin-a",
            "2026-09-30 14:10:02",
        );
        assert!(matches!(
            duplicate,
            Err(CreditMutationError::DuplicateBlacklist)
        ));

        assert_eq!(
            scoped_a.credits().check_before_order(phone)?["allowed"],
            false
        );
        assert_eq!(
            scoped_b.credits().check_before_order(phone)?["allowed"],
            true
        );
        assert_eq!(
            scoped_a.credits().blacklist_check(phone)?["records"][0]["createdBy"],
            "credit-admin-a"
        );

        let violation = scoped_a.credits().violation_record(
            "Alice",
            phone,
            None,
            "damage",
            "major",
            "screen destroyed",
            Some("photo"),
            500.0,
            "credit-reporter-a",
            "2026-09-30 14:11:00",
        )?;
        assert_eq!(violation.penalty, 20);

        let profile_a = scoped_a
            .credits()
            .credit_get(phone, "2026-09-30 14:11:01")?;
        let profile_b = scoped_b
            .credits()
            .credit_get(phone, "2026-09-30 14:11:02")?;
        assert_eq!(profile_a["score"], 80);
        assert_eq!(profile_a["damageIncidents"], 1);
        assert_eq!(profile_b["score"], 100);
        assert_eq!(profile_b["damageIncidents"], 0);

        let cross_tenant =
            scoped_b
                .credits()
                .violation_appeal(violation.id, "foreign", "2026-09-30 14:12:00");
        assert!(matches!(
            cross_tenant,
            Err(CreditMutationError::ViolationNotFound)
        ));

        scoped_a.credits().violation_appeal(
            violation.id,
            "please review",
            "2026-09-30 14:12:01",
        )?;
        scoped_a.credits().violation_review(
            violation.id,
            "upheld",
            "confirmed",
            "credit-admin-a",
            "2026-09-30 14:13:00",
        )?;

        let history = scoped_a.credits().credit_history(phone)?;
        assert_eq!(history["history"].as_array().unwrap().len(), 1);
        assert_eq!(history["history"][0]["scoreImpact"], -20);

        let recalculated = scoped_a
            .credits()
            .credit_recalculate(phone, "2026-09-30 14:14:00")?;
        assert_eq!(recalculated.score, 90);

        scoped_a.credits().blacklist_remove(
            black_a.id,
            "resolved",
            "credit-admin-a",
            "2026-09-30 14:15:00",
        )?;
        assert_eq!(
            scoped_a.credits().blacklist_check(phone)?["blacklisted"],
            false
        );
        assert_eq!(
            scoped_b.credits().blacklist_check(phone)?["blacklisted"],
            true
        );

        (black_a.id, violation.id)
    };

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-credit-a", "credit-recomposed"))?;
    let profile = scoped_after
        .credits()
        .credit_get(phone, "2026-09-30 14:16:00")?;
    assert_eq!(profile["score"], 90);

    let violation = scoped_after
        .credits()
        .violation_get(violation_id)?
        .expect("persisted violation");
    assert_eq!(violation["status"], "upheld");
    assert_eq!(violation["reportedBy"], "credit-reporter-a");
    assert_eq!(violation["reviewedBy"], "credit-admin-a");

    let row = sqlx::query(
        "SELECT created_by,removed_by
         FROM blacklist
         WHERE tenant_id='tenant-credit-a' AND id=$1",
    )
    .bind(black_a_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(row.try_get::<String, _>("created_by")?, "credit-admin-a");
    assert_eq!(
        row.try_get::<Option<String>, _>("removed_by")?.as_deref(),
        Some("credit-admin-a")
    );

    fixture.cleanup().await
}
