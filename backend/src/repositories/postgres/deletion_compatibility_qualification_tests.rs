use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::{
    DeletionCompatibilityError, DeletionCompatibilityRepository, DeletionRequestOutcome,
};
use system_admin::deletion::{DeletionAdminInput, DeletionListInput, DeletionRequestInput};

struct LiveDeletionFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDeletionFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_deletion_{}", uuid::Uuid::new_v4().simple());
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

        sqlx::query(
            "INSERT INTO tenants
             (id,name,slug,status,plan,settings,created_at,updated_at)
             VALUES
             ('tenant-a','Tenant A','tenant-a','active','free',NULL,'now','now'),
             ('tenant-b','Tenant B','tenant-b','active','free',NULL,'now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO identities
             (id,username,password_hash,display_name,email,status,created_at,updated_at)
             VALUES
             ('owner-a','owner-a','hash','Owner A','owner-a@test','active','01','01'),
             ('user-a','user-a','hash','User A','user-a@test','active','02','02'),
             ('owner-b','owner-b','hash','Owner B','owner-b@test','active','03','03')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES
             ('owner-membership-a','owner-a','tenant-a','owner','active','01','01'),
             ('user-membership-a','user-a','tenant-a','staff','active','02','02'),
             ('owner-membership-b','owner-b','tenant-b','owner','active','03','03')",
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

fn request(user_id: &str) -> DeletionRequestInput {
    DeletionRequestInput {
        user_id: user_id.into(),
        request_type: "account".into(),
        reason: "privacy".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_deletion_compatibility_preserves_scope_dedup_and_owner_safety()
-> anyhow::Result<()> {
    let fixture = LiveDeletionFixture::create().await?;
    let repository = DeletionCompatibilityRepository::postgres(fixture.pool.clone());
    let concurrent_a = repository.clone();
    let concurrent_b = repository.clone();

    let first =
        tokio::spawn(async move { concurrent_a.request("tenant-a", &request("user-a"), "10") });
    let second =
        tokio::spawn(async move { concurrent_b.request("tenant-a", &request("user-a"), "11") });
    let first = first.await??;
    let second = second.await??;
    let created = usize::from(matches!(&first, DeletionRequestOutcome::Created { .. }))
        + usize::from(matches!(&second, DeletionRequestOutcome::Created { .. }));
    let existing = usize::from(matches!(&first, DeletionRequestOutcome::Existing { .. }))
        + usize::from(matches!(&second, DeletionRequestOutcome::Existing { .. }));
    assert_eq!(created, 1);
    assert_eq!(existing, 1);

    let tenant_a = repository.list("tenant-a", &DeletionListInput { status: None })?;
    let tenant_b = repository.list("tenant-b", &DeletionListInput { status: None })?;
    assert_eq!(tenant_a.len(), 1);
    assert!(tenant_b.is_empty());
    let user_request_id = tenant_a[0].id.clone();

    repository.process(
        "tenant-a",
        &DeletionAdminInput {
            request_id: user_request_id.clone(),
            admin_notes: "processing".into(),
        },
    )?;
    let processing = repository.list(
        "tenant-a",
        &DeletionListInput {
            status: Some("processing".into()),
        },
    )?;
    assert_eq!(processing.len(), 1);

    let completed = repository.complete(
        "tenant-a",
        &DeletionAdminInput {
            request_id: user_request_id,
            admin_notes: "complete".into(),
        },
        "12",
    )?;
    assert_eq!(completed.identity_id, "user-a");
    assert!(completed.identity_anonymized);
    let user_membership_status: String =
        sqlx::query_scalar("SELECT status FROM tenant_memberships WHERE id = 'user-membership-a'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(user_membership_status, "revoked");
    let user_identity: (String, String) =
        sqlx::query_as("SELECT status, username FROM identities WHERE id = 'user-a'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(user_identity.0, "disabled");
    assert!(user_identity.1.starts_with("ANONYMIZED-"));

    let owner_request = repository.request("tenant-a", &request("owner-a"), "13")?;
    let owner_request_id = match owner_request {
        DeletionRequestOutcome::Created { id, .. } => id,
        DeletionRequestOutcome::Existing { id } => id,
    };
    let owner_result = repository.complete(
        "tenant-a",
        &DeletionAdminInput {
            request_id: owner_request_id,
            admin_notes: "must fail".into(),
        },
        "14",
    );
    assert!(matches!(
        owner_result,
        Err(DeletionCompatibilityError::LastOwner)
    ));
    let owner_status: String =
        sqlx::query_scalar("SELECT status FROM identities WHERE id = 'owner-a'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(owner_status, "active");

    fixture.cleanup().await
}
