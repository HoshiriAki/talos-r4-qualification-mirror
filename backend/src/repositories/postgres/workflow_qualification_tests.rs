use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, TenantId,
    TenantMembershipId, TenantRole, TenantScope,
};

use crate::repositories::{
    PostgresRepositoryProvider, RENTAL_DEFINITION_HASH, RENTAL_DEFINITION_ID,
    RENTAL_DEFINITION_VERSION, RENTAL_STEPS, RepositoryProvider, WorkflowStepState,
};

struct LiveWorkflowFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveWorkflowFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_workflow_{}", uuid::Uuid::new_v4().simple());
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

fn tenant_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("membership-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Admin,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("workflow-pg18").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-workflow-preview",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-workflow-preview-member")
                    .unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("workflow-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("workflow-preview").unwrap()),
        RequestId::new("workflow-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_workflow_preserves_durable_identity_claims_resolution_and_scope()
-> anyhow::Result<()> {
    let fixture = LiveWorkflowFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a = provider.bind(&tenant_context("tenant-a", "workflow-a"))?;
    let scoped_b = provider.bind(&tenant_context("tenant-b", "workflow-b"))?;
    let now = Utc.with_ymd_and_hms(2026, 9, 19, 8, 0, 0).unwrap();

    let first = scoped_a
        .workflows()
        .start_from_message("message-1", "order-1", now)?;
    let duplicate = scoped_a
        .workflows()
        .start_from_message("message-1", "order-1", now)?;
    let second_message = scoped_a
        .workflows()
        .start_from_message("message-2", "order-1", now)?;

    assert_eq!(first.id, duplicate.id);
    assert_eq!(first.id, second_message.id);
    assert_eq!(first.definition_id, RENTAL_DEFINITION_ID);
    assert_eq!(first.definition_version, RENTAL_DEFINITION_VERSION);
    assert_eq!(first.definition_hash, RENTAL_DEFINITION_HASH);
    assert_eq!(
        scoped_a.workflows().steps(&first.id)?.len(),
        RENTAL_STEPS.len()
    );
    assert_eq!(scoped_a.workflows().get(&first.id)?, Some(first.clone()));
    assert!(scoped_b.workflows().get(&first.id)?.is_none());

    let claimed = scoped_a
        .workflows()
        .claim_due("worker-a", now, 60)?
        .expect("first workflow step should be claimable");
    assert_eq!(claimed.step.state, WorkflowStepState::Running);
    assert!(
        scoped_a
            .workflows()
            .claim_due("worker-b", now, 60)?
            .is_none()
    );

    scoped_a.workflows().transition(
        &claimed.step.id,
        WorkflowStepState::RetryScheduled,
        now,
        Some(now + Duration::seconds(30)),
        Some("retry"),
    )?;
    assert!(
        scoped_a
            .workflows()
            .claim_due("worker-b", now + Duration::seconds(29), 60)?
            .is_none()
    );
    let resumed = scoped_a
        .workflows()
        .claim_due("worker-b", now + Duration::seconds(30), 60)?
        .expect("retry should become eligible");
    scoped_a.workflows().transition(
        &resumed.step.id,
        WorkflowStepState::Succeeded,
        now + Duration::seconds(31),
        None,
        None,
    )?;
    let illegal = scoped_a
        .workflows()
        .transition(
            &resumed.step.id,
            WorkflowStepState::RetryScheduled,
            now + Duration::seconds(32),
            Some(now + Duration::seconds(60)),
            None,
        )
        .unwrap_err();
    assert!(illegal.to_string().contains("illegal workflow transition"));

    let blocker_id = scoped_a.workflows().add_blocker(
        &first.id,
        "shipment_delivered",
        "R2_UNAVAILABLE",
        "provider absent",
        now,
    )?;
    let manual_id = scoped_a.workflows().add_manual_task(
        &first.id,
        "risk_cases_resolved",
        "REVIEW",
        "operator decision",
        now,
    )?;
    assert!(scoped_a.workflows().order_has_open_blockers("order-1")?);
    scoped_a.workflows().resolve_blocker(&blocker_id, now)?;
    scoped_a
        .workflows()
        .resolve_manual_task(&manual_id, "approved", now)?;
    assert!(!scoped_a.workflows().order_has_open_blockers("order-1")?);

    let resolved: i64 = sqlx::query_scalar(
        "SELECT \
           (SELECT COUNT(*) FROM workflow_blockers \
            WHERE tenant_id='tenant-a' AND status='resolved') + \
           (SELECT COUNT(*) FROM manual_decision_tasks \
            WHERE tenant_id='tenant-a' AND status='resolved' AND decision='approved')",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(resolved, 2);

    sqlx::query(
        "INSERT INTO domain_outbox \
         (id,tenant_id,source_kind,source_id,message_type,idempotency_key,payload_json, \
          payload_version,state,available_at,created_at) \
         VALUES ('event-order-2','tenant-a','order','order-2','OrderConfirmed', \
                 'event-order-2','{\"orderId\":\"order-2\"}',1,'pending',$1,$1)",
    )
    .bind(now.to_rfc3339())
    .execute(&fixture.pool)
    .await?;
    assert_eq!(scoped_a.workflows().consume_domain_events(now, 100)?, 1);

    let outbox_state: String = sqlx::query_scalar(
        "SELECT state FROM domain_outbox WHERE tenant_id='tenant-a' AND id='event-order-2'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let inbox_state: String = sqlx::query_scalar(
        "SELECT state FROM domain_inbox WHERE tenant_id='tenant-a' AND message_id='event-order-2'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let order_2_instances: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_instances \
         WHERE tenant_id='tenant-a' AND source_kind='order' AND source_id='order-2'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(
        (outbox_state.as_str(), inbox_state.as_str()),
        ("delivered", "processed")
    );
    assert_eq!(order_2_instances, 1);

    let preview = provider.bind(&preview_context())?;
    assert_eq!(
        preview
            .workflows()
            .start_from_message("preview-message", "preview-order", now)
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );

    fixture.cleanup().await?;
    Ok(())
}
