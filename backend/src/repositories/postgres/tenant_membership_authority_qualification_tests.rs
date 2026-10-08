use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_admin::staff::{CreateTenantMemberInput, ResetPasswordInput, UpdateUsernameInput};
use system_core::TenantRole;

use crate::repositories::{
    TenantMembershipActor, TenantMembershipAuthorityError, TenantMembershipAuthorityRepository,
};

struct LiveTenantMembershipFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveTenantMembershipFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_tenant_membership_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-b','Tenant B','tenant-b','active','free',NULL,'now','now'),
             ('tenant-c','Tenant C','tenant-c','active','free',NULL,'now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO identities
             (id,username,email,password_hash,display_name,phone,status,created_at,updated_at)
             VALUES
             ('owner-a','owner-a',NULL,'scrypt$owner-a','Owner A',NULL,'active','01','01'),
             ('admin-a','admin-a',NULL,'scrypt$admin-a','Admin A',NULL,'active','02','02'),
             ('candidate-a','candidate-a',NULL,'scrypt$candidate-a','Candidate A',NULL,'active','03','03'),
             ('shared','shared',NULL,'scrypt$shared','Shared',NULL,'active','04','04'),
             ('machine-a','machine-a',NULL,'!non-interactive','Machine A',NULL,'active','05','05'),
             ('owner-b','owner-b',NULL,'scrypt$owner-b','Owner B',NULL,'active','06','06')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES
             ('owner-membership-a','owner-a','tenant-a','owner','active','01','01'),
             ('admin-membership-a','admin-a','tenant-a','admin','active','02','02'),
             ('candidate-membership-a','candidate-a','tenant-a','staff','active','03','03'),
             ('shared-membership-a','shared','tenant-a','staff','active','04','04'),
             ('machine-membership-a','machine-a','tenant-a','admin','active','05','05'),
             ('owner-membership-b','owner-b','tenant-b','owner','active','06','06'),
             ('shared-membership-b','shared','tenant-b','staff','active','07','07')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO machine_identities (identity_id,created_at)
             VALUES ('machine-a','now')",
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

fn owner_a() -> TenantMembershipActor {
    TenantMembershipActor {
        tenant_id: "tenant-a".into(),
        membership_id: "owner-membership-a".into(),
        identity_id: "owner-a".into(),
        role: TenantRole::Owner,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tenant_membership_authority_preserves_scope_credentials_human_governance_and_transfer()
-> anyhow::Result<()> {
    let fixture = LiveTenantMembershipFixture::create().await?;
    let repository = TenantMembershipAuthorityRepository::postgres(fixture.pool.clone());
    let actor = owner_a();

    let tenant_a = repository.list("tenant-a")?;
    assert_eq!(tenant_a.len(), 5);
    assert!(
        tenant_a
            .iter()
            .all(|member| member.membership_id != "owner-membership-b")
    );
    let tenant_b = repository.list("tenant-b")?;
    assert_eq!(tenant_b.len(), 2);

    let created = repository.create(
        &actor,
        &CreateTenantMemberInput {
            username: "new-staff".into(),
            password_hash: "scrypt$new-staff".into(),
            role: "staff".into(),
        },
        "10",
    )?;
    assert_eq!(created.username, "new-staff");
    assert_eq!(created.role, "staff");

    let create_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE action = 'tenant_membership.created' AND resource_id = $1",
    )
    .bind(&created.membership_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(create_audits, 1);

    sqlx::query(
        "INSERT INTO auth_sessions
         (id,token_hash,identity_id,auth_strength,created_at,last_seen_at,expires_at,revoked_at)
         VALUES ('session-new-staff','token-new-staff',$1,'password','10','10','99',NULL)",
    )
    .bind(&created.identity_id)
    .execute(&fixture.pool)
    .await?;

    let reset = repository.reset_password(
        &actor,
        &ResetPasswordInput {
            id: created.membership_id.clone(),
            new_password_hash: "scrypt$rotated".into(),
        },
        "11",
    )?;
    assert_eq!(reset.revoked_session_count, 1);
    let rotated_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM identities WHERE id = $1")
            .bind(&created.identity_id)
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(rotated_hash, "scrypt$rotated");
    let remaining_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions WHERE id = 'session-new-staff'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(remaining_sessions, 0);

    let renamed = repository.update_username(
        &actor,
        &UpdateUsernameInput {
            id: created.membership_id.clone(),
            new_username: "renamed-staff".into(),
        },
        "12",
    )?;
    assert_eq!(renamed.username, "renamed-staff");

    let shared_hash_before: String =
        sqlx::query_scalar("SELECT password_hash FROM identities WHERE id = 'shared'")
            .fetch_one(&fixture.pool)
            .await?;
    let shared_reset = repository.reset_password(
        &actor,
        &ResetPasswordInput {
            id: "shared-membership-a".into(),
            new_password_hash: "scrypt$must-not-commit".into(),
        },
        "13",
    );
    assert!(matches!(
        shared_reset,
        Err(TenantMembershipAuthorityError::SharedIdentity)
    ));
    let shared_hash_after: String =
        sqlx::query_scalar("SELECT password_hash FROM identities WHERE id = 'shared'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(shared_hash_after, shared_hash_before);

    let machine_transfer = repository.transfer_ownership(&actor, "machine-membership-a", "14");
    assert!(matches!(
        machine_transfer,
        Err(TenantMembershipAuthorityError::MachineOwnerForbidden)
    ));

    let direct_machine_owner = sqlx::query(
        "INSERT INTO tenant_memberships
         (id,identity_id,tenant_id,role,status,created_at,updated_at)
         VALUES ('machine-owner-attempt','machine-a','tenant-c','owner','active','15','15')",
    )
    .execute(&fixture.pool)
    .await;
    assert!(direct_machine_owner.is_err());
    let machine_owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tenant_memberships WHERE id = 'machine-owner-attempt'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(machine_owner_count, 0);

    let transfer = repository.transfer_ownership(&actor, "admin-membership-a", "16")?;
    assert_eq!(transfer.previous_owner_membership_id, "owner-membership-a");
    assert_eq!(transfer.owner_membership_id, "admin-membership-a");
    let owner_rows: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM tenant_memberships
         WHERE tenant_id = 'tenant-a' AND role = 'owner' AND status = 'active'",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(owner_rows, vec!["admin-membership-a".to_string()]);
    let transfer_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE action = 'tenant_ownership.transferred'
           AND resource_id = 'admin-membership-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(transfer_audits, 1);

    fixture.cleanup().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tenant_ownership_transfer_serializes_competing_successors() -> anyhow::Result<()>
{
    let fixture = LiveTenantMembershipFixture::create().await?;
    let repository_a = TenantMembershipAuthorityRepository::postgres(fixture.pool.clone());
    let repository_b = repository_a.clone();
    let actor_a = owner_a();
    let actor_b = actor_a.clone();

    let transfer_a = tokio::spawn(async move {
        repository_a.transfer_ownership(&actor_a, "admin-membership-a", "20")
    });
    let transfer_b = tokio::spawn(async move {
        repository_b.transfer_ownership(&actor_b, "candidate-membership-a", "20")
    });

    let result_a = transfer_a.await?;
    let result_b = transfer_b.await?;
    let successes = usize::from(result_a.is_ok()) + usize::from(result_b.is_ok());
    assert_eq!(successes, 1);
    let loser = if result_a.is_err() {
        result_a
    } else {
        result_b
    };
    assert!(matches!(
        loser,
        Err(TenantMembershipAuthorityError::StaleOwner)
            | Err(TenantMembershipAuthorityError::RoleEscalation)
    ));

    let owner_rows: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM tenant_memberships
         WHERE tenant_id = 'tenant-a' AND role = 'owner' AND status = 'active'",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(owner_rows.len(), 1);
    assert!(matches!(
        owner_rows[0].as_str(),
        "admin-membership-a" | "candidate-membership-a"
    ));

    let transfer_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE tenant_id = 'tenant-a' AND action = 'tenant_ownership.transferred'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(transfer_audits, 1);

    fixture.cleanup().await
}
