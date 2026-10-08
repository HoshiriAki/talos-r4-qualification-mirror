use std::collections::{BTreeMap, BTreeSet};

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::catalog_repository::IntegrationCatalogRepository;
use super::types::{
    CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderHealth, ProviderId,
    ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest, ProviderReadiness,
};

struct LiveCatalogFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveCatalogFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!(
            "r4_p8_integration_catalog_{}",
            uuid::Uuid::new_v4().simple()
        );
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

fn manifest() -> ProviderManifest {
    let capability = CapabilityId::new("fixture.payment.charge").unwrap();
    ProviderManifest {
        provider_id: ProviderId::new("fixture-payments").unwrap(),
        version: "1.0.0".into(),
        capabilities: BTreeSet::from([capability.clone()]),
        config_schema: vec![],
        secret_schema: vec![],
        api_versions: BTreeMap::new(),
        webhook_types: BTreeSet::new(),
        simulation_capabilities: BTreeSet::from([capability]),
        readiness: ProviderReadiness::Fixture,
        compatibility: BTreeMap::new(),
    }
}

fn instance(tenant: &str, revision: &str) -> ProviderInstance {
    ProviderInstance {
        id: ProviderInstanceId::new("instance-a").unwrap(),
        tenant_id: tenant.into(),
        provider_id: ProviderId::new("fixture-payments").unwrap(),
        manifest_version: "1.0.0".into(),
        config_revision: revision.into(),
        config: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        lifecycle: ProviderLifecycle::Active,
        health: ProviderHealth::Ready,
        readiness: ProviderReadiness::Fixture,
    }
}

fn binding(tenant: &str, capability: &str, revision: &str) -> ProviderBinding {
    ProviderBinding {
        id: ProviderBindingId::new("binding-a").unwrap(),
        tenant_id: tenant.into(),
        provider_instance_id: ProviderInstanceId::new("instance-a").unwrap(),
        capability: CapabilityId::new(capability).unwrap(),
        config_revision: revision.into(),
        enabled: true,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_integration_catalog_preserves_manifest_binding_history_and_tenant_revision()
-> anyhow::Result<()> {
    let fixture = LiveCatalogFixture::create().await?;
    let repository = IntegrationCatalogRepository::postgres(fixture.pool.clone());

    let manifest = manifest();
    repository.save_manifest(&manifest)?;
    let duplicate = repository.save_manifest(&manifest).unwrap_err();
    assert!(matches!(duplicate, IntegrationError::InvalidManifest(_)));

    repository.save_instance(&instance("tenant-a", "rev-1"))?;
    repository.save_binding(
        &binding("tenant-a", "fixture.payment.charge", "rev-1"),
        "actor-a",
    )?;
    repository.save_instance(&instance("tenant-b", "rev-b1"))?;
    repository.save_binding(
        &binding("tenant-b", "fixture.payment.charge", "rev-b1"),
        "actor-b-tenant",
    )?;

    let catalog = repository.catalog()?;
    assert_eq!(catalog.manifests().len(), 1);
    assert_eq!(catalog.instances_for_tenant("tenant-a").len(), 1);
    assert_eq!(catalog.instances_for_tenant("tenant-b").len(), 1);
    assert_eq!(catalog.bindings_for_tenant("tenant-a").len(), 1);
    assert_eq!(catalog.bindings_for_tenant("tenant-b").len(), 1);

    let history: (i64, String) = sqlx::query_as(
        "SELECT COUNT(*), MIN(actor_ref)
         FROM provider_binding_history
         WHERE tenant_id='tenant-a' AND binding_id='binding-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(history.0, 1);
    assert_eq!(history.1, "actor-a");

    let stale = repository
        .save_binding(
            &binding("tenant-a", "fixture.payment.charge", "rev-stale"),
            "actor-stale",
        )
        .unwrap_err();
    assert_eq!(stale, IntegrationError::BindingRevisionStale);

    let undeclared = repository
        .save_binding(
            &binding("tenant-a", "fixture.payment.refund", "rev-1"),
            "actor-undeclared",
        )
        .unwrap_err();
    assert_eq!(undeclared, IntegrationError::CapabilityNotDeclared);

    let wrong_tenant = repository
        .save_binding(
            &binding("tenant-c", "fixture.payment.charge", "rev-1"),
            "actor-cross-tenant",
        )
        .unwrap_err();
    assert_eq!(wrong_tenant, IntegrationError::BindingUnavailable);

    let unchanged_history: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_binding_history
         WHERE tenant_id='tenant-a' AND binding_id='binding-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(unchanged_history, 1);

    repository.save_instance(&instance("tenant-a", "rev-2"))?;
    let old_revision = repository
        .save_binding(
            &binding("tenant-a", "fixture.payment.charge", "rev-1"),
            "actor-old-revision",
        )
        .unwrap_err();
    assert_eq!(old_revision, IntegrationError::BindingRevisionStale);

    repository.save_binding(
        &binding("tenant-a", "fixture.payment.charge", "rev-2"),
        "actor-b",
    )?;

    let stored_revision: String = sqlx::query_scalar(
        "SELECT config_revision FROM provider_bindings
         WHERE tenant_id='tenant-a' AND id='binding-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(stored_revision, "rev-2");

    let final_history: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_binding_history
         WHERE tenant_id='tenant-a' AND binding_id='binding-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(final_history, 2);

    let recomposed = IntegrationCatalogRepository::postgres(fixture.pool.clone());
    let recomposed_catalog = recomposed.catalog()?;
    assert_eq!(recomposed_catalog.manifests().len(), 1);
    assert_eq!(recomposed_catalog.instances_for_tenant("tenant-a").len(), 1);
    assert_eq!(recomposed_catalog.instances_for_tenant("tenant-b").len(), 1);
    assert_eq!(recomposed_catalog.bindings_for_tenant("tenant-a").len(), 1);
    assert_eq!(recomposed_catalog.bindings_for_tenant("tenant-b").len(), 1);
    assert_eq!(
        recomposed_catalog.bindings_for_tenant("tenant-a")[0].config_revision,
        "rev-2"
    );
    assert_eq!(
        recomposed_catalog.bindings_for_tenant("tenant-b")[0].config_revision,
        "rev-b1"
    );

    fixture.cleanup().await
}
