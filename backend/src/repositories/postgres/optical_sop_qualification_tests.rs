use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    OpticalSopMutationError, PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveOpticalSopFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveOpticalSopFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_optical_sop_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-optical-a','Optical A','tenant-optical-a','active','test','now','now'),
             ('tenant-optical-b','Optical B','tenant-optical-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        for actor in ["optical-actor-a", "optical-actor-b"] {
            sqlx::query(
                "INSERT INTO identities
                 (id,username,password_hash,display_name,status,created_at,updated_at)
                 VALUES ($1,$1,'!non-interactive',$1,'active','now','now')",
            )
            .bind(actor)
            .execute(&pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              deviceserialno,createdat,status,tenant_id)
             VALUES
             ('optical-order-a','OPT-A','2026-10-01','2026-10-10','2026-10-01',
              'pickup','','','','now','in_use','tenant-optical-a'),
             ('optical-order-b','OPT-B','2026-10-01','2026-10-10','2026-10-01',
              'pickup','','','','now','shipped','tenant-optical-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('optical-device-a','OPT-A-1','in_use','now','tenant-optical-a'),
             ('optical-device-b','OPT-B-1','in_use','now','tenant-optical-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_devices (id,orderid,serialno,createdat,tenant_id)
             VALUES
             ('optical-od-a','optical-order-a','OPT-A-1','now','tenant-optical-a'),
             ('optical-od-b','optical-order-b','OPT-B-1','now','tenant-optical-b')",
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

fn context(tenant: &str, actor: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated(actor, "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("optical-sop-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_optical_sop_authority_preserves_scope_completion_damage_link_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveOpticalSopFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-optical-a", "optical-actor-a", "optical-a");
    let ctx_b = context("tenant-optical-b", "optical-actor-b", "optical-b");

    let (inspection_id, damage_id, completed_at) = {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let created = scoped_a.optical_sops().create(
            "optical-order-a",
            "OPT-A-1",
            "optical-actor-a",
            "2026-10-02T10:10:00+08:00",
        )?;
        assert_eq!(created.overall_grade, "pass");
        assert_eq!(
            scoped_a.optical_sops().list(None, None, None, 1, 20)?.total,
            1
        );
        assert_eq!(
            scoped_b.optical_sops().list(None, None, None, 1, 20)?.total,
            0
        );
        assert!(scoped_b.optical_sops().get(created.id)?.is_none());

        let duplicate = scoped_a.optical_sops().create(
            "optical-order-a",
            "OPT-A-1",
            "optical-actor-a",
            "2026-10-02T10:10:01+08:00",
        );
        assert!(matches!(duplicate, Err(OpticalSopMutationError::Duplicate)));

        let cross_tenant_device = scoped_a.optical_sops().create(
            "optical-order-a",
            "OPT-B-1",
            "optical-actor-a",
            "2026-10-02T10:10:02+08:00",
        );
        assert!(matches!(
            cross_tenant_device,
            Err(OpticalSopMutationError::DeviceNotInOrder)
        ));

        let updated = scoped_a.optical_sops().update_step(
            created.id,
            "body",
            false,
            Some("scratch"),
            "2026-10-02T10:11:00+08:00",
        )?;
        assert_eq!(updated.overall_grade, "minor_damage");

        let completed = scoped_a.optical_sops().complete(
            created.id,
            "optical-actor-a",
            "2026-10-02T10:12:00+08:00",
        )?;
        assert_eq!(completed.overall_grade, "minor_damage");
        let damage_id = completed.damage_report_id.clone().expect("damage id");

        let repeated = scoped_a.optical_sops().complete(
            created.id,
            "optical-actor-a",
            "2026-10-02T10:12:30+08:00",
        )?;
        assert_eq!(
            repeated.damage_report_id.as_deref(),
            Some(damage_id.as_str())
        );
        assert_eq!(repeated.completed_at, completed.completed_at);

        let immutable = scoped_a.optical_sops().update_step(
            created.id,
            "lens",
            false,
            Some("late mutation"),
            "2026-10-02T10:13:00+08:00",
        );
        assert!(matches!(immutable, Err(OpticalSopMutationError::Completed)));

        let reports = scoped_a.damages().get_by_order("optical-order-a")?;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].id, damage_id);
        assert_eq!(reports[0].status, "reported");
        assert_eq!(reports[0].reported_by, "optical-actor-a");
        assert!(!reports[0].appearance_ok);
        assert!(reports[0].damage_description.contains("外观异常: scratch"));

        let damage_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM damage_reports
             WHERE tenant_id=$1 AND order_id=$2",
        )
        .bind("tenant-optical-a")
        .bind("optical-order-a")
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!(damage_count, 1);

        let stats = scoped_a.optical_sops().stats()?;
        assert_eq!(stats.total, 1);
        assert_eq!(stats.pass_count, 0);
        assert_eq!(stats.damage_count, 1);
        assert_eq!(stats.by_grade.get("minor_damage"), Some(&1));

        (created.id, damage_id, completed.completed_at)
    };

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context(
        "tenant-optical-a",
        "optical-actor-a",
        "optical-recomposed",
    ))?;
    let persisted = scoped_after
        .optical_sops()
        .get(inspection_id)?
        .expect("inspection after recomposition");
    assert_eq!(
        persisted.damage_report_id.as_deref(),
        Some(damage_id.as_str())
    );
    assert_eq!(
        persisted.completed_at.as_deref(),
        Some(completed_at.as_str())
    );
    assert_eq!(persisted.overall_grade, "minor_damage");

    fixture.cleanup().await
}
