#![cfg(feature = "postgres")]

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::types::{ProviderBindingId, ProviderId};
use super::webhook::{CanonicalWebhookEvent, WebhookApplicationPort};
use super::webhook_event::{InterconnectWebhookEventPort, WebhookEventLane};
use crate::application::interconnect::postgres::PostgresDurableDriver;

struct LiveFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_webhook_event_{}", uuid::Uuid::new_v4().simple());
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

        PostgresDurableDriver::new(pool.clone())
            .ensure_schema()
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

fn event() -> CanonicalWebhookEvent {
    CanonicalWebhookEvent {
        tenant_id: "tenant-a".into(),
        provider_id: ProviderId::new("fixture-webhooks").unwrap(),
        binding_id: ProviderBindingId::new("binding-a").unwrap(),
        provider_event_id: "provider-event-live-1".into(),
        event_type: "fixture.webhook.updated".into(),
        payload_hash: hex::encode(Sha256::digest(b"fixture-live-payload")),
    }
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_webhook_event_replay_reuses_persisted_envelope_identity() -> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let event = event();

    let first = InterconnectWebhookEventPort::new(WebhookEventLane::postgres(fixture.pool.clone()));
    first.dispatch(event.clone()).await.map_err(|error| {
        anyhow::anyhow!("first normalized webhook Event admission failed: {error:?}")
    })?;

    let first_row: (i64, i64, String) = sqlx::query_as(
        "SELECT COUNT(*) OVER () AS event_count, created_at_ms, envelope::text AS envelope_text \
         FROM interconnect_events WHERE tenant_id='tenant-a' AND subject='fixture.webhook.updated'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(first_row.0, 1);
    assert!(!first_row.2.contains("fixture-live-payload"));
    assert!(!first_row.2.contains("authorization"));

    // Recreate the port as a process-restart analogue. The second admission
    // must reconstruct the persisted timestamp and deduplicate to the exact P3
    // envelope identity rather than adding a second Event or conflicting.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let restarted =
        InterconnectWebhookEventPort::new(WebhookEventLane::postgres(fixture.pool.clone()));
    restarted.dispatch(event).await.map_err(|error| {
        anyhow::anyhow!("replayed normalized webhook Event admission failed: {error:?}")
    })?;

    let second_row: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) AS event_count, MIN(created_at_ms) AS created_at_ms \
         FROM interconnect_events WHERE tenant_id='tenant-a' AND subject='fixture.webhook.updated'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(second_row.0, 1);
    assert_eq!(second_row.1, first_row.1);

    fixture.cleanup().await?;
    Ok(())
}
