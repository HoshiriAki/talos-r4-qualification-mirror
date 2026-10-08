use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::TenantRole;

use crate::error::AppError;
use crate::repositories::{
    MachineAdminContext, MachineProvisionRecord, MachineScopeRecord,
    PostgresMachineAuthorityRepository,
};

struct LiveMachineAuthorityFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveMachineAuthorityFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_machine_authority_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(6)
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
             (id,name,slug,status,plan,created_at,updated_at)
             VALUES ('tenant-a','Tenant A','tenant-a','active','test','now','now'),
                    ('tenant-b','Tenant B','tenant-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES ('admin-a','admin-a','hash','Admin A','active','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES ('admin-membership-a','admin-a','tenant-a','owner','active','now','now')",
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

fn admin() -> MachineAdminContext {
    MachineAdminContext {
        identity_id: "admin-a".into(),
        membership_id: "admin-membership-a".into(),
        tenant_id: "tenant-a".into(),
        role: TenantRole::Owner,
    }
}

fn scope() -> MachineScopeRecord {
    MachineScopeRecord {
        module: "device".into(),
        command: "list".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_machine_authority_preserves_fresh_scope_rotation_rate_and_audit()
-> anyhow::Result<()> {
    let fixture = LiveMachineAuthorityFixture::create().await?;
    let repository = PostgresMachineAuthorityRepository::new(fixture.pool.clone());

    let issued = repository.provision(
        &admin(),
        MachineProvisionRecord {
            name: "Inventory Robot".into(),
            scopes: vec![scope()],
            rate_limit_rpm: 2,
            role: TenantRole::Staff,
        },
    )?;
    assert!(issued.secret.starts_with("mapi_"));

    let machine: (String, String, String, String) = sqlx::query_as(
        "SELECT c.identity_id,c.membership_id,c.tenant_id,m.role
         FROM api_clients c
         JOIN tenant_memberships m ON m.id=c.membership_id
         WHERE c.id=$1",
    )
    .bind(&issued.client_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(machine.2, "tenant-a");
    assert_eq!(machine.3, "staff");

    let marker_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM machine_identities WHERE identity_id=$1")
            .bind(&machine.0)
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(marker_count, 1);

    let forbidden_session = sqlx::query(
        "INSERT INTO auth_sessions
         (id,token_hash,identity_id,auth_strength,created_at,last_seen_at,expires_at)
         VALUES ('machine-session','hash',$1,'password','now','now','never')",
    )
    .bind(&machine.0)
    .execute(&fixture.pool)
    .await;
    assert!(forbidden_session.is_err());

    let forbidden_owner = sqlx::query("UPDATE tenant_memberships SET role='owner' WHERE id=$1")
        .bind(&machine.1)
        .execute(&fixture.pool)
        .await;
    assert!(forbidden_owner.is_err());

    let authorized =
        repository.authorize(&issued.secret, "tenant-a", "v1", &scope(), "machine-auth-1")?;
    assert_eq!(authorized.identity_id, machine.0);
    assert_eq!(authorized.membership_id, machine.1);
    assert_eq!(authorized.tenant_id, "tenant-a");
    assert_eq!(authorized.role, TenantRole::Staff);

    let wrong_tenant = repository
        .authorize(
            &issued.secret,
            "tenant-b",
            "v1",
            &scope(),
            "machine-auth-wrong-tenant",
        )
        .unwrap_err();
    assert!(matches!(wrong_tenant, AppError::Forbidden));

    let usage_after_denial: i32 =
        sqlx::query_scalar("SELECT used FROM api_usage_windows WHERE client_id=$1")
            .bind(&issued.client_id)
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(usage_after_denial, 1);

    let rotated = repository
        .lifecycle(&admin(), &issued.client_id, "rotate")?
        .expect("rotate must issue a replacement credential");
    assert_ne!(rotated.credential_id, issued.credential_id);
    assert_ne!(rotated.secret, issued.secret);

    let old_secret = repository
        .authorize(
            &issued.secret,
            "tenant-a",
            "v1",
            &scope(),
            "machine-auth-old-secret",
        )
        .unwrap_err();
    assert!(matches!(old_secret, AppError::Unauthorized));

    repository.authorize(
        &rotated.secret,
        "tenant-a",
        "v1",
        &scope(),
        "machine-auth-2",
    )?;

    let recomposed = PostgresMachineAuthorityRepository::new(fixture.pool.clone());
    let rate_limited = recomposed
        .authorize(
            &rotated.secret,
            "tenant-a",
            "v1",
            &scope(),
            "machine-auth-rate-limited",
        )
        .unwrap_err();
    assert!(matches!(rate_limited, AppError::RateLimited { .. }));

    let usage: i32 = sqlx::query_scalar("SELECT used FROM api_usage_windows WHERE client_id=$1")
        .bind(&issued.client_id)
        .fetch_one(&fixture.pool)
        .await?;
    assert_eq!(usage, 2);

    let clients = recomposed.list(&admin())?;
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].client_id, issued.client_id);
    assert_eq!(clients[0].window_usage, 2);

    let admitted_roles: String = sqlx::query_scalar(
        "SELECT roles_snapshot::text
         FROM audit_events
         WHERE resource_id=$1
           AND action='machine.access'
           AND detail_json->>'outcome'='admitted'
         ORDER BY occurred_at
         LIMIT 1",
    )
    .bind(&issued.client_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert!(admitted_roles.contains("staff"));
    assert!(!admitted_roles.contains("admin"));

    recomposed.credential_lifecycle(
        &admin(),
        &issued.client_id,
        &rotated.credential_id,
        "revoke",
    )?;
    let revoked_secret = recomposed
        .authorize(
            &rotated.secret,
            "tenant-a",
            "v1",
            &scope(),
            "machine-auth-revoked",
        )
        .unwrap_err();
    assert!(matches!(revoked_secret, AppError::Unauthorized));

    fixture.cleanup().await
}
