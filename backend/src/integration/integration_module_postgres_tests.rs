#![cfg(feature = "postgres")]

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    SystemModule, TenantId, TenantScope,
};

use super::keystore::InMemoryKeyStore;
use super::module::IntegrationModule;
use crate::observability::RuntimeMetrics;

struct LiveIntegrationModuleFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveIntegrationModuleFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_integration_module_{}", uuid::Uuid::new_v4().simple());
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

fn platform_context() -> ExecutionContext {
    ExecutionContext::new(
        ActorIdentity::system(),
        TenantScope::platform(),
        DataScope::platform(Revision::new("integration-module-platform").unwrap()),
        ExecutionMode::Normal,
        RequestId::new("integration-module-platform-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn tenant_context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("integration-module-test", "admin").unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("integration-module-tenant").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("integration-module-tenant-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_integration_module_composition_routes_management_authorities_to_postgres()
-> anyhow::Result<()> {
    let fixture = LiveIntegrationModuleFixture::create().await?;
    let metrics = Arc::new(RuntimeMetrics::default());
    let module = IntegrationModule::new_with_postgres_persistence(
        fixture.pool.clone(),
        Arc::new(InMemoryKeyStore::default()),
        metrics.clone(),
    );

    module
        .execute(
            "register_manifest",
            serde_json::json!({
                "providerId": "fixture-module",
                "version": "1.0.0",
                "capabilities": ["fixture.payment.charge"],
                "configSchema": [],
                "secretSchema": [],
                "apiVersions": {},
                "webhookTypes": ["payment.updated"],
                "simulationCapabilities": ["fixture.payment.charge"],
                "readiness": "fixture",
                "compatibility": {}
            }),
            &platform_context(),
        )
        .map_err(anyhow::Error::msg)?;

    let tenant = tenant_context();
    module
        .execute(
            "upsert_instance",
            serde_json::json!({
                "id": "instance-a",
                "providerId": "fixture-module",
                "manifestVersion": "1.0.0",
                "configRevision": "rev-1",
                "config": {},
                "secretRefs": {},
                "lifecycle": "active",
                "health": "ready",
                "readiness": "fixture"
            }),
            &tenant,
        )
        .map_err(anyhow::Error::msg)?;
    module
        .execute(
            "upsert_binding",
            serde_json::json!({
                "id": "binding-a",
                "providerInstanceId": "instance-a",
                "capability": "fixture.payment.charge",
                "configRevision": "rev-1",
                "enabled": true
            }),
            &tenant,
        )
        .map_err(anyhow::Error::msg)?;

    let operation_payload = serde_json::json!({
        "capability": "fixture.payment.charge",
        "operationType": "charge",
        "idempotencyKey": "module-operation-a",
        "requestHash": "module-request-hash-a"
    });
    let first_operation = module
        .execute("plan_operation", operation_payload.clone(), &tenant)
        .map_err(anyhow::Error::msg)?;
    let duplicate_operation = module
        .execute("plan_operation", operation_payload, &tenant)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        first_operation["externalOperationId"],
        duplicate_operation["externalOperationId"]
    );

    let webhook = module
        .execute(
            "create_webhook_endpoint",
            serde_json::json!({"bindingId": "binding-a"}),
            &tenant,
        )
        .map_err(anyhow::Error::msg)?;
    assert!(
        webhook["endpointToken"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    );

    let deposit = module
        .execute(
            "create_deposit",
            serde_json::json!({
                "authorityKind": "order",
                "authorityId": "order-a",
                "amountMinor": 2500,
                "currency": "USD"
            }),
            &tenant,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(deposit["state"], "expected");

    let manifest_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_manifests
         WHERE provider_id='fixture-module'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let instance_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_instances
         WHERE tenant_id='tenant-a' AND id='instance-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let binding_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_bindings
         WHERE tenant_id='tenant-a' AND id='binding-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let operation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_operations
         WHERE tenant_id='tenant-a' AND idempotency_key='module-operation-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let webhook_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhook_endpoints
         WHERE tenant_id='tenant-a' AND binding_id='binding-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let deposit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM integration_deposits
         WHERE tenant_id='tenant-a' AND authority_kind='order' AND authority_id='order-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;

    assert_eq!(
        (
            manifest_count,
            instance_count,
            binding_count,
            operation_count,
            webhook_count,
            deposit_count,
        ),
        (1, 1, 1, 1, 1, 1)
    );

    let output = metrics.render();
    assert!(output.contains(
        "talos_integration_operation_events_total{event=\"admitted\",state=\"ready\"} 1"
    ));
    assert!(
        output.contains("talos_financial_foundation_events_total{event=\"deposit_recorded\"} 1")
    );

    fixture.cleanup().await
}
