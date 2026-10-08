use std::sync::Arc;

use feature_tenant_simulation::FeatureTenantSimulation;
use serde_json::json;
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, RequestId, Revision, SimulationId,
    SystemModule, TenantId, TenantScope,
};

struct LiveSimulationFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveSimulationFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_simulation_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-sim-a','Simulation A','tenant-sim-a','active','test','now','now'),
             ('tenant-sim-b','Simulation B','tenant-sim-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO pricing_configs
             (baseweekdayprice,baseweekendprice,holidayrulesjson,receiveshippingfeesjson,
              updatedby,createdat,updatedat,tenant_id)
             VALUES
             (8.5,14.0,'[]'::jsonb,'{}'::jsonb,'','now','now','tenant-sim-a'),
             (9.0,15.0,'[]'::jsonb,'{}'::jsonb,'','now','now','tenant-sim-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO simulation_tenant_revisions (tenant_id,revision,updated_at)
             VALUES
             ('tenant-sim-a',7,'2026-10-03T12:00:00+08:00'),
             ('tenant-sim-b',3,'2026-10-03T12:00:00+08:00')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO simulation_reference_configs
             (tenant_id,key,value_json,resource_version,updated_at)
             VALUES
             ('tenant-sim-a','base',
              '{\"enabled\":true,\"threshold\":5,\"label\":\"base\"}'::jsonb,
              3,'2026-10-03T12:00:00+08:00')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO simulation_reference_resource_revisions
             (tenant_id,key,resource_version,present,updated_at)
             VALUES
             ('tenant-sim-a','base',3,true,'2026-10-03T12:00:00+08:00')",
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

fn platform(actor: &str, request: &str) -> ExecutionContext {
    ExecutionContext::new(
        ActorIdentity::with_authority(
            actor,
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new(format!("membership-{actor}")).unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::platform(),
        DataScope::platform(Revision::new("platform").unwrap()),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation(actor: &str, tenant: &str, id: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let simulation_id = SimulationId::new(id).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            actor,
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new(format!("membership-{actor}")).unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(
            tenant_id,
            Namespace::Simulation(simulation_id.clone()),
            Revision::new("simulation-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation_id),
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tenant_simulation_preserves_scope_idempotency_overlay_evidence_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveSimulationFixture::create().await?;
    let module = FeatureTenantSimulation::with_postgres(fixture.pool.clone());
    let admin_a = platform("simulation-admin-a", "simulation-create");

    let create_payload = json!({
        "tenantId": "tenant-sim-a",
        "scenarioName": "postgres proof",
        "changeIntent": "verify isolated overlay",
        "ttlMinutes": 30,
        "plannedAbsentKeys": ["new-key"],
        "idempotencyKey": "simulation-create-key-123456"
    });
    let created = module
        .execute("session.create", create_payload.clone(), &admin_a)
        .map_err(anyhow::Error::msg)?;
    let simulation_id = created["id"].as_str().expect("simulation id").to_owned();
    assert_eq!(created["status"], "active");

    let replay = module
        .execute("session.create", create_payload.clone(), &admin_a)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(replay["id"].as_str(), Some(simulation_id.as_str()));

    let mut conflicting_create = create_payload;
    conflicting_create["scenarioName"] = json!("different request");
    let conflict = module
        .execute("session.create", conflicting_create, &admin_a)
        .unwrap_err();
    assert!(conflict.contains("IDEMPOTENCY_CONFLICT"));

    let foreign_actor = module
        .execute(
            "session.get",
            json!({"id": simulation_id.clone()}),
            &platform("simulation-admin-b", "simulation-foreign-actor"),
        )
        .unwrap_err();
    assert!(foreign_actor.contains("SIMULATION_NOT_FOUND"));

    let ctx = simulation(
        "simulation-admin-a",
        "tenant-sim-a",
        &simulation_id,
        "simulation-overlay",
    );
    let wrong_tenant = simulation(
        "simulation-admin-a",
        "tenant-sim-b",
        &simulation_id,
        "simulation-wrong-tenant",
    );
    let denied = module
        .execute(
            "reference_config.get",
            json!({"id": simulation_id.clone(), "key": "base"}),
            &wrong_tenant,
        )
        .unwrap_err();
    assert!(denied.contains("SESSION_NOT_EXECUTABLE"));

    let base = module
        .execute(
            "reference_config.get",
            json!({"id": simulation_id.clone(), "key": "base"}),
            &ctx,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(base["value"]["label"], "base");

    let put_payload = json!({
        "id": simulation_id.clone(),
        "key": "base",
        "value": {"enabled": false, "threshold": 9, "label": "overlay"},
        "idempotencyKey": "simulation-put-key-123456"
    });
    let overlay = module
        .execute("reference_config.put", put_payload.clone(), &ctx)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(overlay["value"]["label"], "overlay");
    let overlay_replay = module
        .execute("reference_config.put", put_payload, &ctx)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(overlay_replay, overlay);

    let production_label: String = sqlx::query_scalar(
        "SELECT value_json->>'label'
         FROM simulation_reference_configs
         WHERE tenant_id='tenant-sim-a' AND key='base'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(production_label, "base");

    let pricing = module
        .execute(
            "pricing_config.get",
            json!({"id": simulation_id.clone()}),
            &ctx,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(pricing["value"]["baseWeekdayPrice"], 8.5);

    let pricing_overlay = module
        .execute(
            "pricing_config.put",
            json!({
                "id": simulation_id.clone(),
                "value": {
                    "baseWeekdayPrice": 9.5,
                    "baseWeekendPrice": 15.5,
                    "holidayRules": [],
                    "receiveShippingFees": {},
                    "dynamicPriceMap": {}
                },
                "idempotencyKey": "simulation-pricing-key-123456"
            }),
            &ctx,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(pricing_overlay["value"]["baseWeekdayPrice"], 9.5);

    let diff = module
        .execute("diff.evaluate", json!({"id": simulation_id.clone()}), &ctx)
        .map_err(anyhow::Error::msg)?;
    let evaluation_id = diff["evaluationId"]
        .as_str()
        .expect("evaluation id")
        .to_owned();
    assert!(
        diff["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let evidence = module
        .execute(
            "diff.get",
            json!({
                "id": simulation_id.clone(),
                "evaluationId": evaluation_id.clone(),
                "limit": 50
            }),
            &admin_a,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        evidence["evaluationId"].as_str(),
        Some(evaluation_id.as_str())
    );

    let discarded = module
        .execute(
            "session.discard",
            json!({"id": simulation_id.clone()}),
            &platform("simulation-admin-a", "simulation-discard"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(discarded["status"], "discarded");

    let terminal_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM simulation_terminal_inputs
         WHERE simulation_id=$1 AND tenant_id='tenant-sim-a'",
    )
    .bind(&simulation_id)
    .fetch_one(&fixture.pool)
    .await?;
    let cleanup_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM simulation_cleanup_jobs
         WHERE simulation_id=$1 AND tenant_id='tenant-sim-a'
           AND job_kind='terminal_evidence'",
    )
    .bind(&simulation_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(terminal_count, 1);
    assert_eq!(cleanup_count, 1);

    let recomposed = FeatureTenantSimulation::with_postgres(fixture.pool.clone());
    let persisted = recomposed
        .execute(
            "session.get",
            json!({"id": simulation_id.clone()}),
            &platform("simulation-admin-a", "simulation-recomposed"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(persisted["status"], "discarded");

    let persisted_evidence = recomposed
        .execute(
            "diff.get",
            json!({
                "id": simulation_id.clone(),
                "evaluationId": evaluation_id.clone(),
                "limit": 50
            }),
            &platform("simulation-admin-a", "simulation-evidence-recomposed"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        persisted_evidence["evaluationId"].as_str(),
        Some(evaluation_id.as_str())
    );

    fixture.cleanup().await
}
