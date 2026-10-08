use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, TenantId,
    TenantScope,
};

use crate::domain::ReservationId;
use crate::repositories::{
    DeviceAllocationRequest, PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveReservationFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveReservationFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_reservation_{}", uuid::Uuid::new_v4().simple());
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
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
                 VALUES ($1,$2,$3,'active','test','2026-09-16T00:00:00Z','2026-09-16T00:00:00Z')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }

        for (id, name, prefix) in [("model-a", "Model A", "A"), ("model-b", "Model B", "B")] {
            sqlx::query(
                "INSERT INTO device_models \
                 (id,name,category,prefix,enabled,createdat,updatedat,tenant_id) \
                 VALUES ($1,$2,'fixture',$3,TRUE,'2026-09-16T00:00:00Z','2026-09-16T00:00:00Z','tenant-a')",
            )
            .bind(id)
            .bind(name)
            .bind(prefix)
            .execute(&pool)
            .await?;
        }

        for (id, serial, model_id) in [
            ("device-a1", "SER-A1", "model-a"),
            ("device-a2", "SER-A2", "model-a"),
            ("device-b1", "SER-B1", "model-b"),
            ("device-b2", "SER-B2", "model-b"),
        ] {
            sqlx::query(
                "INSERT INTO devices (id,serialno,rentalstatus,createdat,modelid,tenant_id) \
                 VALUES ($1,$2,'available','2026-09-16T00:00:00Z',$3,'tenant-a')",
            )
            .bind(id)
            .bind(serial)
            .bind(model_id)
            .execute(&pool)
            .await?;
        }

        for (id, order_no, model_id) in [
            ("reservation-order-a", "RSV-A-001", "model-a"),
            ("reservation-order-rollback", "RSV-R-001", "model-b"),
        ] {
            sqlx::query(
                "INSERT INTO orders \
                 (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,tenant_id,totalprice,province,sendwarehouseid,returnwarehouseid,accessories,status,trackingno,devicemodels) \
                 VALUES ($1,$2,'2026-10-01','2026-10-05','2026-10-01','[]','','','', \
                         '2026-09-16T00:00:00Z','tenant-a',0,'','','','[]'::jsonb,'draft','','{}'::jsonb)",
            )
            .bind(id)
            .bind(order_no)
            .execute(&pool)
            .await?;

            sqlx::query(
                "INSERT INTO order_lines \
                 (id,tenant_id,order_id,source_quote_line_id,line_kind,reference_id,description, \
                  quantity,unit_price_minor,subtotal_minor,currency,price_snapshot_json,created_at) \
                 VALUES ($1,'tenant-a',$2,$3,'model',$4,$5,2,0,0,'CNY','{}'::jsonb,'2026-09-16T00:00:00Z')",
            )
            .bind(format!("line-{id}"))
            .bind(id)
            .bind(format!("source-{id}"))
            .bind(model_id)
            .bind(format!("Demand for {model_id}"))
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

fn normal_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("reservation-write").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-actor",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("reservation-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("reservation-preview").unwrap()),
        RequestId::new("reservation-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_reservation_write_preserves_atomic_batch_outbox_and_scope() -> anyhow::Result<()>
{
    let fixture = LiveReservationFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a = provider.bind(&normal_context("tenant-a", "reservation-write-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "reservation-write-b"))?;

    let reservation_id = ReservationId::new();
    let hold = tenant_a.reservations().create_from_order(
        reservation_id.clone(),
        "reservation-order-a",
        30,
    )?;
    assert_eq!(hold.status, "hold");
    assert_eq!(hold.version, 1);
    assert_eq!(hold.requirements.len(), 1);
    assert_eq!(hold.requirements[0].model_id, "model-a");
    assert_eq!(hold.requirements[0].quantity, 2);
    assert!(tenant_b.reservations().get(&reservation_id)?.is_none());

    let confirmed = tenant_a.reservations().confirm(&reservation_id, None)?;
    assert_eq!(confirmed.status, "confirmed");
    assert_eq!(confirmed.version, 2);

    let confirmation_events = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM domain_outbox \
         WHERE tenant_id='tenant-a' AND source_id=$1 AND message_type='ReservationConfirmed'",
    )
    .bind(reservation_id.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(confirmation_events, 1);

    let allocations = tenant_a.reservations().allocate_devices_batch(&[
        DeviceAllocationRequest {
            order_id: "reservation-order-a".into(),
            device_serial_no: "SER-A1".into(),
        },
        DeviceAllocationRequest {
            order_id: "reservation-order-a".into(),
            device_serial_no: "SER-A2".into(),
        },
    ])?;
    assert_eq!(allocations.len(), 2);
    assert!(
        tenant_a
            .reservations()
            .allocation_complete_for_order("reservation-order-a")?
    );

    let completion_events = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM domain_outbox \
         WHERE tenant_id='tenant-a' AND source_id=$1 AND message_type='AllocationComplete'",
    )
    .bind(reservation_id.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(completion_events, 1);

    let released = tenant_a
        .reservations()
        .release_allocation(&allocations[0].id)?;
    assert_eq!(released.status, "released");
    assert!(released.released_at.is_some());

    let rollback_reservation_id = ReservationId::new();
    tenant_a.reservations().create_from_order(
        rollback_reservation_id.clone(),
        "reservation-order-rollback",
        30,
    )?;
    tenant_a
        .reservations()
        .confirm(&rollback_reservation_id, None)?;

    let rollback_error = tenant_a
        .reservations()
        .allocate_devices_batch(&[
            DeviceAllocationRequest {
                order_id: "reservation-order-rollback".into(),
                device_serial_no: "SER-B1".into(),
            },
            DeviceAllocationRequest {
                order_id: "reservation-order-rollback".into(),
                device_serial_no: "SER-B-MISSING".into(),
            },
        ])
        .unwrap_err();
    assert_eq!(rollback_error.code(), "REPOSITORY_CONTRACT_VIOLATION");

    let rollback_allocations = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM allocations \
         WHERE tenant_id='tenant-a' AND reservation_id=$1",
    )
    .bind(rollback_reservation_id.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(rollback_allocations, 0);

    let rollback_completion_events = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM domain_outbox \
         WHERE tenant_id='tenant-a' AND source_id=$1 AND message_type='AllocationComplete'",
    )
    .bind(rollback_reservation_id.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(rollback_completion_events, 0);

    let preview = provider.bind(&preview_context("tenant-a"))?;
    let preview_error = preview.reservations().expire_due().unwrap_err();
    assert_eq!(preview_error.code(), "REPOSITORY_PREVIEW_WRITE_DENIED");

    fixture.cleanup().await?;
    Ok(())
}
