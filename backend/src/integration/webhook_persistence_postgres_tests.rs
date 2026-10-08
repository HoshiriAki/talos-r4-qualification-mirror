#![cfg(feature = "postgres")]

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::catalog_repository::IntegrationCatalogRepository;
use super::types::{
    CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderHealth, ProviderId,
    ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest, ProviderReadiness,
};
use super::webhook_persistence_contract::{
    WebhookAdminPersistence, WebhookReplayPersistence, WebhookRuntimePersistence,
};
use super::webhook_persistence_postgres::PostgresWebhookPersistence;

struct LiveWebhookFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveWebhookFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!(
            "r4_p8_webhook_persistence_{}",
            uuid::Uuid::new_v4().simple()
        );
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

fn manifest() -> ProviderManifest {
    let capability = CapabilityId::new("fixture.webhook").unwrap();
    ProviderManifest {
        provider_id: ProviderId::new("fixture-webhooks").unwrap(),
        version: "1.0.0".into(),
        capabilities: BTreeSet::from([capability.clone()]),
        config_schema: vec![],
        secret_schema: vec![],
        api_versions: BTreeMap::new(),
        webhook_types: BTreeSet::from(["fixture.webhook.updated".into()]),
        simulation_capabilities: BTreeSet::from([capability]),
        readiness: ProviderReadiness::Fixture,
        compatibility: BTreeMap::new(),
    }
}

fn instance() -> ProviderInstance {
    ProviderInstance {
        id: ProviderInstanceId::new("instance-a").unwrap(),
        tenant_id: "tenant-a".into(),
        provider_id: ProviderId::new("fixture-webhooks").unwrap(),
        manifest_version: "1.0.0".into(),
        config_revision: "rev-1".into(),
        config: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        lifecycle: ProviderLifecycle::Active,
        health: ProviderHealth::Ready,
        readiness: ProviderReadiness::Fixture,
    }
}

fn binding() -> ProviderBinding {
    ProviderBinding {
        id: ProviderBindingId::new("binding-a").unwrap(),
        tenant_id: "tenant-a".into(),
        provider_instance_id: ProviderInstanceId::new("instance-a").unwrap(),
        capability: CapabilityId::new("fixture.webhook").unwrap(),
        config_revision: "rev-1".into(),
        enabled: true,
    }
}

async fn inbox_status(pool: &PgPool, event_id: &str) -> anyhow::Result<String> {
    Ok(sqlx::query_scalar(
        "SELECT status FROM webhook_inbox WHERE tenant_id='tenant-a' AND provider_event_id=$1",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_webhook_persistence_preserves_identity_claim_retry_dead_letter_and_replay()
-> anyhow::Result<()> {
    let fixture = LiveWebhookFixture::create().await?;
    let catalog = IntegrationCatalogRepository::postgres(fixture.pool.clone());
    catalog.save_manifest(&manifest())?;
    catalog.save_instance(&instance())?;
    catalog.save_binding(&binding(), "actor-a")?;

    let persistence = PostgresWebhookPersistence::new(fixture.pool.clone());
    let endpoint_token = b"webhook-token-a";
    let endpoint_id = persistence.register_webhook_endpoint(
        "tenant-a",
        &ProviderBindingId::new("binding-a")?,
        endpoint_token,
    )?;

    let endpoints = persistence.list_webhook_endpoints("tenant-a")?;
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].endpoint_id, endpoint_id);

    let endpoint = persistence.resolve_webhook_endpoint(endpoint_token)?;
    assert_eq!(endpoint.tenant_id, "tenant-a");
    assert_eq!(
        persistence
            .resolve_webhook_endpoint(b"unknown-token")
            .unwrap_err(),
        IntegrationError::BindingUnavailable
    );

    let promoted_payload = br#"{"eventType":"fixture.webhook.updated","value":1}"#;
    persistence.record_rejected_webhook(
        &endpoint,
        "provider-event-promote",
        "{}",
        promoted_payload,
        "verification_failed",
    )?;
    let (promoted_id, promoted_duplicate) = persistence.record_verified_webhook(
        &endpoint,
        "provider-event-promote",
        "{}",
        promoted_payload,
    )?;
    assert!(promoted_duplicate);
    assert_eq!(
        inbox_status(&fixture.pool, "provider-event-promote").await?,
        "verified"
    );

    let conflict_payload = br#"{"value":2}"#;
    let (conflict_id, conflict_duplicate) = persistence.record_verified_webhook(
        &endpoint,
        "provider-event-conflict",
        "{}",
        conflict_payload,
    )?;
    assert!(!conflict_duplicate);
    assert_eq!(
        persistence
            .record_verified_webhook(
                &endpoint,
                "provider-event-conflict",
                "{}",
                br#"{"value":3}"#,
            )
            .unwrap_err(),
        IntegrationError::WebhookUnverifiable
    );
    let expected_hash = hex::encode(Sha256::digest(conflict_payload));
    let stored_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM webhook_inbox
         WHERE tenant_id='tenant-a' AND provider_event_id='provider-event-conflict'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(stored_hash, expected_hash);

    sqlx::query(
        "UPDATE webhook_inbox SET status='processed'
         WHERE id=$1 OR id=$2",
    )
    .bind(promoted_id.as_str())
    .bind(conflict_id.as_str())
    .execute(&fixture.pool)
    .await?;

    let (_work_id, duplicate) = persistence.record_verified_webhook(
        &endpoint,
        "provider-event-work",
        "{}",
        br#"{"work":1}"#,
    )?;
    assert!(!duplicate);
    assert!(persistence.claim_next_webhook("tenant-b")?.is_none());

    let first_claim = persistence
        .claim_next_webhook("tenant-a")?
        .expect("tenant-a verified webhook must be claimable");
    assert_eq!(first_claim.provider_event_id, "provider-event-work");
    assert_eq!(first_claim.attempt_number, 1);
    persistence.retry_webhook(&first_claim, "application_retry")?;
    assert_eq!(
        inbox_status(&fixture.pool, "provider-event-work").await?,
        "verified"
    );

    let second_claim = persistence
        .claim_next_webhook("tenant-a")?
        .expect("retried webhook must be claimable again");
    assert_eq!(second_claim.attempt_number, 2);
    persistence.complete_webhook(&second_claim, "fixture.webhook.updated")?;
    assert_eq!(
        inbox_status(&fixture.pool, "provider-event-work").await?,
        "processed"
    );

    persistence.record_verified_webhook(
        &endpoint,
        "provider-event-dead",
        "{}",
        br#"{"dead":1}"#,
    )?;
    let dead_claim = persistence
        .claim_next_webhook("tenant-a")?
        .expect("dead-letter fixture must be claimable");
    persistence.dead_letter_webhook(&dead_claim, "permanent_mapping_failure")?;
    assert_eq!(
        inbox_status(&fixture.pool, "provider-event-dead").await?,
        "dead_letter"
    );

    let dead_letters = persistence.list_webhook_dead_letters("tenant-a")?;
    assert_eq!(dead_letters.len(), 1);
    assert_eq!(dead_letters[0].inbox_id, dead_claim.inbox_id);

    persistence.replay_webhook(
        "tenant-a",
        &dead_claim.inbox_id,
        "operator-a",
        "manual replay after mapper repair",
    )?;
    assert_eq!(
        inbox_status(&fixture.pool, "provider-event-dead").await?,
        "verified"
    );
    let replay_audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhook_replay_audit
         WHERE tenant_id='tenant-a' AND inbox_id=$1",
    )
    .bind(dead_claim.inbox_id.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(replay_audit_count, 1);

    persistence.set_webhook_endpoint_enabled("tenant-a", &endpoint_id, false)?;
    assert_eq!(
        persistence
            .resolve_webhook_endpoint(endpoint_token)
            .unwrap_err(),
        IntegrationError::BindingUnavailable
    );
    persistence.set_webhook_endpoint_enabled("tenant-a", &endpoint_id, true)?;
    assert_eq!(
        persistence
            .resolve_webhook_endpoint(endpoint_token)?
            .endpoint_id,
        endpoint_id
    );

    fixture.cleanup().await
}
