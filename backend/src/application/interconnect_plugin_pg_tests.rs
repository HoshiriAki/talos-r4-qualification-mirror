#![cfg(feature = "postgres")]

use std::sync::Arc;

use serde_json::Value;
use sqlx::postgres::{PgConnection, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::transport::interconnect::{
    BindingRevisionRef, ContractBinding, ContractRef, ContractVersion, CorrelationId, Extensions,
    MessageKind, PayloadRef, PluginId, ProviderInstanceRef, SchemaRef, Subject,
};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use super::interconnect::postgres::PostgresDurableDriver;
use super::interconnect_plugin::{PluginWorkAdmission, enqueue_postgres_plugin_work};

struct LiveFixture {
    database_url: String,
    schema: String,
    driver: PostgresDurableDriver,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_plugin_admission_{}", uuid::Uuid::new_v4().simple());
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
        sqlx::query(
            "CREATE TABLE tenants (id TEXT PRIMARY KEY,name TEXT NOT NULL,slug TEXT UNIQUE NOT NULL,status TEXT NOT NULL,plan TEXT NOT NULL,settings TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) VALUES ('tenant-a','A','a','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        let driver = PostgresDurableDriver::new(pool);
        driver.ensure_schema().await?;
        Ok(Self {
            database_url,
            schema,
            driver,
        })
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        self.driver.pool().close().await;
        let mut admin = PgConnection::connect(&self.database_url).await?;
        admin
            .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
            .await?;
        Ok(())
    }
}

fn ctx() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "staff-a",
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("membership-a").unwrap(),
                tenant_id: tenant.clone(),
                role: TenantRole::Staff,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("corr-a").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn admission(work_id: &str, package: char) -> PluginWorkAdmission {
    PluginWorkAdmission {
        envelope: system_core::transport::interconnect::MessageEnvelope {
            id: system_core::transport::interconnect::MessageId::new(work_id).unwrap(),
            kind: MessageKind::Work,
            subject: Subject::new("logistics.shipment.query").unwrap(),
            contract: ContractBinding {
                contract: ContractRef::new("logistics.shipment.query").unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                schema: SchemaRef::new("logistics.shipment.query.v1").unwrap(),
            },
            correlation_id: CorrelationId::new("corr-a").unwrap(),
            causation_id: None,
            created_at_ms: 10,
            deadline_ms: None,
            ordering_key: None,
            idempotency_key: None,
            payload: PayloadRef::Inline(Value::Null),
            extensions: Extensions::empty(),
        },
        executable: system_core::transport::interconnect::PluginExecutableRef {
            plugin_id: PluginId::new("official.fixture").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: package.to_string().repeat(64),
            manifest_digest_sha256: "b".repeat(64),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            provider_instance_id: Some(ProviderInstanceRef::new("provider-a").unwrap()),
            binding_revision: Some(BindingRevisionRef::new("binding-rev-1").unwrap()),
        },
        retry_budget: 1,
        available_at_ms: 10,
    }
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_plugin_work_admission_is_atomic_and_cannot_retrofit_generic_work()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let trusted = ctx();

    let plugin = admission("plugin-work-1", 'a');
    let work_id = enqueue_postgres_plugin_work(&fixture.driver, &trusted, plugin.clone()).await?;
    assert_eq!(work_id.as_str(), "plugin-work-1");
    assert_eq!(
        enqueue_postgres_plugin_work(&fixture.driver, &trusted, plugin.clone())
            .await?
            .as_str(),
        "plugin-work-1"
    );

    let work_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM interconnect_work WHERE work_id='plugin-work-1'")
            .fetch_one(fixture.driver.pool())
            .await?;
    let pin_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM interconnect_plugin_work_pins WHERE work_id='plugin-work-1'",
    )
    .fetch_one(fixture.driver.pool())
    .await?;
    assert_eq!((work_count, pin_count), (1, 1));

    let changed = admission("plugin-work-1", 'c');
    assert_eq!(
        enqueue_postgres_plugin_work(&fixture.driver, &trusted, changed)
            .await
            .unwrap_err()
            .code,
        system_core::transport::interconnect::InterconnectErrorCode::ContractIncompatible
    );

    assert_eq!(
        fixture
            .driver
            .enqueue_work(&trusted, plugin.envelope.clone(), 1, 10)
            .await
            .unwrap_err()
            .code,
        system_core::transport::interconnect::InterconnectErrorCode::ContractIncompatible
    );

    let generic = admission("generic-work-1", 'a');
    fixture
        .driver
        .enqueue_work(&trusted, generic.envelope.clone(), 1, 10)
        .await?;
    assert_eq!(
        enqueue_postgres_plugin_work(&fixture.driver, &trusted, generic)
            .await
            .unwrap_err()
            .code,
        system_core::transport::interconnect::InterconnectErrorCode::ContractIncompatible
    );
    let generic_pin_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM interconnect_plugin_work_pins WHERE work_id='generic-work-1'",
    )
    .fetch_one(fixture.driver.pool())
    .await?;
    assert_eq!(generic_pin_count, 0);

    let orphan_insert = sqlx::query(
        "INSERT INTO interconnect_plugin_work_pins \
         (work_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,manifest_digest_sha256,capability_contract_version,provider_instance_id,binding_revision) \
         VALUES ('missing-work','tenant-a','official.fixture','1.0.0',$1,$2,'1.0.0','provider-a','binding-rev-1')",
    )
    .bind("a".repeat(64))
    .bind("b".repeat(64))
    .execute(fixture.driver.pool())
    .await;
    assert!(orphan_insert.is_err());

    // Force the pin insert to fail after enqueue_work_in_transaction has inserted
    // the Work row. Because both writes share one transaction, the admission
    // error must leave neither durable Work nor durable executable pin behind.
    sqlx::raw_sql(
        "CREATE FUNCTION reject_plugin_pin() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'forced plugin pin failure'; END $$; \
         CREATE TRIGGER reject_plugin_pin_before_insert \
         BEFORE INSERT ON interconnect_plugin_work_pins \
         FOR EACH ROW EXECUTE FUNCTION reject_plugin_pin();",
    )
    .execute(fixture.driver.pool())
    .await?;
    let rollback_probe = admission("rollback-work-1", 'd');
    assert_eq!(
        enqueue_postgres_plugin_work(&fixture.driver, &trusted, rollback_probe)
            .await
            .unwrap_err()
            .code,
        system_core::transport::interconnect::InterconnectErrorCode::DriverFailure
    );
    let rollback_work_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM interconnect_work WHERE work_id='rollback-work-1'",
    )
    .fetch_one(fixture.driver.pool())
    .await?;
    let rollback_pin_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM interconnect_plugin_work_pins WHERE work_id='rollback-work-1'",
    )
    .fetch_one(fixture.driver.pool())
    .await?;
    assert_eq!((rollback_work_count, rollback_pin_count), (0, 0));

    fixture.cleanup().await?;
    Ok(())
}
