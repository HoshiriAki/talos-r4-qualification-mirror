use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, TenantId, TenantScope,
};

use crate::domain::{ContactKind, CustomerId, CustomerRiskStatus, CustomerStatus, ReservationId};
use crate::repositories::{
    NewCustomerContact, NewCustomerRecord, OrderListRequest, PostgresRepositoryProvider,
    RepositoryProvider,
};

const RESERVATION_A: &str = "11111111-1111-4111-8111-111111111111";
const REQUIREMENT_A: &str = "22222222-2222-4222-8222-222222222222";
const ALLOCATION_A: &str = "33333333-3333-4333-8333-333333333333";

struct LiveFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_repo_{}", uuid::Uuid::new_v4().simple());
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

    async fn seed(&self) -> anyhow::Result<()> {
        for (id, name, slug) in [
            ("tenant-a", "Tenant A", "tenant-a"),
            ("tenant-b", "Tenant B", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
                 VALUES ($1,$2,$3,'active','test','2026-09-15T00:00:00+08:00','2026-09-15T00:00:00+08:00')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&self.pool)
            .await?;
        }

        for (id, order_no, tenant_id, address) in [
            ("order-a", "A-001", "tenant-a", "Alpha Road"),
            ("order-b", "B-001", "tenant-b", "Beta Road"),
        ] {
            sqlx::query(
                "INSERT INTO orders \
                 (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,tenant_id,totalprice,province,sendwarehouseid,returnwarehouseid,accessories,status,trackingno,devicemodels) \
                 VALUES ($1,$2,'2026-09-16','2026-09-20','2026-09-15','[]',$4,'','', \
                         '2026-09-15T00:00:00+08:00',$3,100,'Test','','','[]'::jsonb,'draft','','{}'::jsonb)",
            )
            .bind(id)
            .bind(order_no)
            .bind(tenant_id)
            .bind(address)
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO device_models \
             (id,name,category,prefix,enabled,createdat,updatedat,tenant_id) \
             VALUES ('model-a','Model A','fixture','A',TRUE, \
                     '2026-09-15T00:00:00+08:00','2026-09-15T00:00:00+08:00','tenant-a')",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,modelid,tenant_id) \
             VALUES ('device-a','SER-A','available','2026-09-15T00:00:00+08:00','model-a','tenant-a')",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO rental_reservations \
             (id,tenant_id,order_id,source_kind,status,start_date,end_date,expires_at,version,created_at,updated_at) \
             VALUES ($1,'tenant-a','order-a','order','confirmed','2026-09-16','2026-09-20', \
                     '2026-09-21T00:00:00Z',1,'2026-09-15T00:00:00Z','2026-09-15T00:00:00Z')",
        )
        .bind(RESERVATION_A)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO reservation_requirements \
             (id,tenant_id,reservation_id,model_id,quantity,created_at) \
             VALUES ($1,'tenant-a',$2,'model-a',1,'2026-09-15T00:00:00Z')",
        )
        .bind(REQUIREMENT_A)
        .bind(RESERVATION_A)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO allocations \
             (id,tenant_id,reservation_id,order_id,device_serial_no,model_id,start_date,end_date,status,allocated_at) \
             VALUES ($1,'tenant-a',$2,'order-a','SER-A','model-a','2026-09-16','2026-09-20','allocated','2026-09-15T00:00:00Z')",
        )
        .bind(ALLOCATION_A)
        .bind(RESERVATION_A)
        .execute(&self.pool)
        .await?;
        Ok(())
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
        DataScope::production(tenant_id, Revision::new("pg-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn platform_actor() -> ActorIdentity {
    ActorIdentity::with_authority(
        "platform-actor",
        AuthorityContext::Platform {
            membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
            roles: vec![PlatformRole::Owner],
        },
    )
    .unwrap()
}

fn preview_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        platform_actor(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("preview-revision").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-session").unwrap()),
        RequestId::new("preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let simulation_id = SimulationId::new("simulation-session").unwrap();
    ExecutionContext::new(
        platform_actor(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(
            tenant_id,
            Namespace::Simulation(simulation_id.clone()),
            Revision::new("simulation-base").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation_id),
        RequestId::new("simulation-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_scoped_order_read_preserves_tenant_preview_and_mode_boundaries()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    fixture.seed().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());

    let tenant_a = provider.bind(&normal_context("tenant-a", "request-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "request-b"))?;
    let page_a = tenant_a.orders().list(&OrderListRequest::default())?;
    let page_b = tenant_b.orders().list(&OrderListRequest::default())?;
    assert_eq!(page_a.total, 1);
    assert_eq!(page_a.orders[0].id, "order-a");
    assert_eq!(page_b.total, 1);
    assert_eq!(page_b.orders[0].id, "order-b");
    assert!(tenant_a.orders().get_by_id("order-b")?.is_none());
    assert!(tenant_b.orders().get_by_id("order-a")?.is_none());

    let customer_id = CustomerId::new();
    let customer = tenant_a.customers().create(&NewCustomerRecord {
        id: customer_id.clone(),
        legal_name: "Alice Example".into(),
        display_name: "Alice".into(),
        status: CustomerStatus::Active,
        risk_status: CustomerRiskStatus::Clear,
        contacts: vec![NewCustomerContact {
            kind: ContactKind::Email,
            raw_value: "Alice@Example.com".into(),
            normalized_value: "alice@example.com".into(),
            is_primary: true,
        }],
        actor_identity_id: None,
    })?;
    assert_eq!(customer.id, customer_id);
    assert_eq!(customer.contacts.len(), 1);
    assert_eq!(customer.contacts[0].masked_value, "a***@example.com");
    assert_eq!(tenant_a.customers().list(100)?.len(), 1);
    assert!(tenant_b.customers().get(&customer_id)?.is_none());
    let duplicate_candidates = tenant_a
        .customers()
        .duplicate_candidates(ContactKind::Email, "alice@example.com")?;
    assert_eq!(duplicate_candidates.len(), 1);
    assert_eq!(duplicate_candidates[0].customer_id, customer_id);
    assert!(
        tenant_b
            .customers()
            .duplicate_candidates(ContactKind::Email, "alice@example.com")?
            .is_empty()
    );

    let lifecycle_a = tenant_a.lifecycles().operational_view("order-a")?;
    assert_eq!(lifecycle_a.lifecycle.commercial_status, "draft");
    assert_eq!(lifecycle_a.lifecycle.version, 1);
    assert!(
        lifecycle_a
            .allowed_actions
            .iter()
            .any(|action| action.action == "submit_order")
    );
    assert!(
        lifecycle_a
            .allowed_actions
            .iter()
            .any(|action| action.action == "confirm_order")
    );
    assert!(
        lifecycle_a
            .blockers
            .iter()
            .any(|blocker| blocker == "financial_obligation_unsatisfied")
    );
    assert!(
        !lifecycle_a
            .blockers
            .iter()
            .any(|blocker| blocker == "reservation_not_confirmed")
    );
    assert!(
        !lifecycle_a
            .blockers
            .iter()
            .any(|blocker| blocker == "allocation_incomplete")
    );
    assert!(tenant_a.lifecycles().get("order-b")?.is_none());

    let reservation_id = ReservationId::parse(RESERVATION_A)?;
    let reservation = tenant_a
        .reservations()
        .find_by_order("order-a")?
        .expect("tenant-a reservation");
    assert_eq!(reservation.id, reservation_id);
    assert_eq!(reservation.requirements.len(), 1);
    assert_eq!(reservation.requirements[0].model_id, "model-a");
    assert_eq!(reservation.allocations.len(), 1);
    assert_eq!(reservation.allocations[0].device_serial_no, "SER-A");
    assert!(
        tenant_a
            .reservations()
            .allocation_complete_for_order("order-a")?
    );
    assert!(tenant_b.reservations().find_by_order("order-a")?.is_none());
    assert!(tenant_b.reservations().get(&reservation_id)?.is_none());
    let capacity = tenant_a.reservations().capacity(
        "model-a",
        "2026-09-16",
        "2026-09-20",
        1,
        Some("SER-A"),
    )?;
    assert_eq!(capacity.total_capacity, 1);
    assert_eq!(capacity.reserved_capacity, 1);
    assert_eq!(capacity.available_capacity, 0);
    assert_eq!(capacity.device_available, Some(false));
    assert!(!capacity.can_reserve);
    assert_eq!(tenant_a.reservations().expire_due()?, 0);

    let preview = provider.bind(&preview_context("tenant-a"))?;
    assert_eq!(
        preview.orders().list(&OrderListRequest::default())?.total,
        1
    );
    assert!(preview.customers().get(&customer_id)?.is_some());
    assert_eq!(
        preview
            .customers()
            .anonymize(&customer_id, None)
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );
    assert_eq!(
        preview
            .lifecycles()
            .operational_view("order-a")?
            .lifecycle
            .order_id,
        "order-a"
    );
    assert_eq!(
        preview
            .lifecycles()
            .apply_action("order-a", "submit_order", 1, "platform-actor", "forbidden")
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );
    assert!(preview.reservations().get(&reservation_id)?.is_some());
    assert_eq!(
        preview.reservations().expire_due().unwrap_err().code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );
    let preview_write = preview.session().pg_write(|connection| {
        Box::pin(async move {
            sqlx::query("UPDATE orders SET notes='forbidden' WHERE tenant_id='tenant-a'")
                .execute(connection)
                .await
                .map(|_| ())
        })
    });
    assert_eq!(
        preview_write.unwrap_err().code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );

    let read_write_bypass = tenant_a.session().pg_read(|connection| {
        Box::pin(async move {
            sqlx::query("UPDATE orders SET notes='forbidden' WHERE tenant_id='tenant-a'")
                .execute(connection)
                .await
                .map(|_| ())
        })
    });
    assert_eq!(
        read_write_bypass.unwrap_err().code(),
        "REPOSITORY_CONTRACT_VIOLATION"
    );

    assert_eq!(
        provider
            .bind(&simulation_context("tenant-a"))
            .err()
            .unwrap()
            .code(),
        "REPOSITORY_SIMULATION_UNSUPPORTED"
    );

    fixture.cleanup().await?;
    Ok(())
}
