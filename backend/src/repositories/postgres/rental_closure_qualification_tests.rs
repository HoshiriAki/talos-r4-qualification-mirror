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

struct LiveRentalClosureFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveRentalClosureFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_rental_closure_{}", uuid::Uuid::new_v4().simple());
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
                 VALUES ($1,$2,$3,'active','test','2026-09-19T00:00:00Z','2026-09-19T00:00:00Z')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO device_models \
             (id,name,category,prefix,enabled,createdat,updatedat,tenant_id) \
             VALUES ('model-a','Model A','fixture','A',TRUE, \
                     '2026-09-19T00:00:00Z','2026-09-19T00:00:00Z','tenant-a')",
        )
        .execute(&pool)
        .await?;

        for (id, serial) in [("device-a1", "SER-A1"), ("device-a2", "SER-A2")] {
            sqlx::query(
                "INSERT INTO devices \
                 (id,serialno,rentalstatus,createdat,modelid,tenant_id) \
                 VALUES ($1,$2,'available','2026-09-19T00:00:00Z','model-a','tenant-a')",
            )
            .bind(id)
            .bind(serial)
            .execute(&pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO orders \
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno, \
              createdat,tenant_id,totalprice,province,sendwarehouseid,returnwarehouseid,accessories, \
              status,trackingno,devicemodels) \
             VALUES ('closure-order-a','CLS-A-001','2026-10-01','2026-10-05','2026-10-01', \
                     '[]','','','', '2026-09-19T00:00:00Z','tenant-a',0,'','','','[]'::jsonb, \
                     'draft','','{}'::jsonb)",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_lines \
             (id,tenant_id,order_id,source_quote_line_id,line_kind,reference_id,description, \
              quantity,unit_price_minor,subtotal_minor,currency,price_snapshot_json,created_at) \
             VALUES ('closure-line-a','tenant-a','closure-order-a','closure-source-a','model', \
                     'model-a','Closure demand',2,0,0,'CNY','{}'::jsonb,'2026-09-19T00:00:00Z')",
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

fn normal_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("rental-closure-pg18").unwrap()).unwrap(),
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
            "platform-rental-closure-preview",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-rental-closure-preview-member")
                    .unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("rental-closure-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("rental-closure-preview").unwrap()),
        RequestId::new("rental-closure-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_rental_closure_preserves_return_inspection_settlement_and_scope()
-> anyhow::Result<()> {
    let fixture = LiveRentalClosureFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a = provider.bind(&normal_context("tenant-a", "closure-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "closure-b"))?;

    let reservation_id = ReservationId::new();
    tenant_a
        .reservations()
        .create_from_order(reservation_id.clone(), "closure-order-a", 30)?;
    tenant_a.reservations().confirm(&reservation_id, None)?;
    let allocations = tenant_a.reservations().allocate_devices_batch(&[
        DeviceAllocationRequest {
            order_id: "closure-order-a".into(),
            device_serial_no: "SER-A1".into(),
        },
        DeviceAllocationRequest {
            order_id: "closure-order-a".into(),
            device_serial_no: "SER-A2".into(),
        },
    ])?;
    assert_eq!(allocations.len(), 2);

    let partial = tenant_a.rental_closure().receive_allocation(
        "closure-order-a",
        allocations[0].id.as_str(),
        "scan-a1",
    )?;
    assert_eq!(partial.status, "receiving");
    assert_eq!(partial.item_count, 1);
    assert_eq!(partial.required_count, 2);

    let duplicate = tenant_a.rental_closure().receive_allocation(
        "closure-order-a",
        allocations[0].id.as_str(),
        "scan-a1",
    )?;
    assert_eq!(duplicate.item_count, 1);

    assert!(
        tenant_b
            .rental_closure()
            .receive_allocation(
                "closure-order-a",
                allocations[0].id.as_str(),
                "foreign-scan",
            )
            .is_err()
    );

    let complete = tenant_a.rental_closure().receive_allocation(
        "closure-order-a",
        allocations[1].id.as_str(),
        "scan-a2",
    )?;
    assert_eq!(complete.status, "received");
    assert_eq!(complete.item_count, 2);
    assert!(
        tenant_a
            .rental_closure()
            .facts("closure-order-a")?
            .return_received
    );

    let inspection_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM rental_inspections \
         WHERE tenant_id='tenant-a' ORDER BY allocation_id",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(inspection_ids.len(), 2);
    assert_eq!(
        tenant_a
            .rental_closure()
            .inspection(&inspection_ids[0])?
            .expect("inspection must exist")
            .status,
        "pending"
    );
    assert!(
        tenant_b
            .rental_closure()
            .inspection(&inspection_ids[0])?
            .is_none()
    );

    assert!(
        tenant_a
            .rental_closure()
            .transition_inspection(&inspection_ids[0], "passed", 1)
            .is_err()
    );
    tenant_a
        .rental_closure()
        .transition_inspection(&inspection_ids[0], "in_progress", 1)?;
    tenant_a
        .rental_closure()
        .transition_inspection(&inspection_ids[0], "failed", 2)?;
    assert!(
        tenant_a
            .rental_closure()
            .transition_inspection(&inspection_ids[0], "passed", 2)
            .is_err()
    );

    tenant_a
        .rental_closure()
        .transition_inspection(&inspection_ids[1], "in_progress", 1)?;
    tenant_a
        .rental_closure()
        .transition_inspection(&inspection_ids[1], "passed", 2)?;

    let damaged_facts = tenant_a.rental_closure().facts("closure-order-a")?;
    assert!(damaged_facts.inspection_complete);
    assert_eq!(damaged_facts.open_damage_reviews, 1);

    let damage_blocked =
        tenant_a
            .rental_closure()
            .calculate_settlement("closure-order-a", "CNY", 12345, true)?;
    assert_eq!(damage_blocked.status, "blocked");
    assert_eq!(
        damage_blocked.blocker_code.as_deref(),
        Some("DAMAGE_REVIEW_OPEN")
    );

    tenant_a.rental_closure().resolve_damage_review(
        &inspection_ids[0],
        "reviewed without assigning liability or fee",
    )?;
    assert_eq!(
        tenant_a
            .rental_closure()
            .facts("closure-order-a")?
            .open_damage_reviews,
        0
    );

    let authority_blocked =
        tenant_a
            .rental_closure()
            .calculate_settlement("closure-order-a", "cny", 12345, false)?;
    assert_eq!(authority_blocked.amount_minor, 12345);
    assert_eq!(authority_blocked.status, "blocked");
    assert_eq!(
        authority_blocked.blocker_code.as_deref(),
        Some("FINANCIAL_AUTHORITY_NOT_AVAILABLE")
    );

    let calculated =
        tenant_a
            .rental_closure()
            .calculate_settlement("closure-order-a", "CNY", 12345, true)?;
    let repeat =
        tenant_a
            .rental_closure()
            .calculate_settlement("closure-order-a", "CNY", 12345, true)?;
    assert_eq!(calculated.status, "calculated");
    assert_eq!(calculated.facts_hash, repeat.facts_hash);

    tenant_a
        .rental_closure()
        .mark_fixture_settlement_terminal("closure-order-a")?;
    assert!(
        tenant_a
            .rental_closure()
            .facts("closure-order-a")?
            .settlement_terminal
    );

    for (message_type, expected) in [
        ("ReturnReceived", 1_i64),
        ("InspectionComplete", 1),
        ("RiskCasesResolved", 1),
        ("SettlementComplete", 1),
    ] {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM domain_outbox \
             WHERE tenant_id='tenant-a' AND message_type=$1",
        )
        .bind(message_type)
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!(
            count, expected,
            "unexpected outbox count for {message_type}"
        );
    }

    let preview = provider.bind(&preview_context("tenant-a"))?;
    assert_eq!(
        preview
            .rental_closure()
            .receive_allocation(
                "closure-order-a",
                allocations[0].id.as_str(),
                "preview-scan",
            )
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );

    fixture.cleanup().await?;
    Ok(())
}
