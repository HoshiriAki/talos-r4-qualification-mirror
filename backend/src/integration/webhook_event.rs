//! R4-P5 normalized webhook -> Interconnect Event Lane handoff.
//!
//! The durable webhook inbox remains the ingress source of truth. A webhook is
//! considered processed only after this port has admitted its provider-neutral
//! event into an R4-P3 Event driver. Production uses the PostgreSQL 18 durable
//! driver; the in-process driver is an explicit development/test profile.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};
use system_core::transport::interconnect::{
    ContractBinding, ContractRef, ContractVersion, CorrelationId, Extensions, IdempotencyKey,
    InterconnectError, InterconnectErrorCode, MessageEnvelope, MessageId, MessageKind, OrderingKey,
    PayloadRef, SchemaRef, Subject,
};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use super::webhook::{CanonicalWebhookEvent, WebhookApplicationPort, WebhookHandlingError};
use crate::application::interconnect::InProcessDriver;
#[cfg(feature = "postgres")]
use crate::application::interconnect::postgres::PostgresDurableDriver;

const WEBHOOK_EVENT_CONTRACT: &str = "integration.webhook.normalized";
const WEBHOOK_EVENT_SCHEMA: &str = "integration.webhook.normalized.v1";
const WEBHOOK_EVENT_VERSION: &str = "1.0.0";
const WEBHOOK_EVENT_REVISION: &str = "r4-p5-webhook-event-v1";

#[derive(Clone)]
pub enum WebhookEventLane {
    /// Explicit non-production reference profile. Admission timestamps are
    /// cached so retrying the same deterministic message within one process
    /// preserves the full P3 envelope identity.
    InProcess {
        driver: InProcessDriver,
        admission_times: Arc<Mutex<HashMap<String, u64>>>,
    },
    #[cfg(feature = "postgres")]
    Postgres(PostgresDurableDriver),
}

impl WebhookEventLane {
    pub fn development() -> Self {
        Self::InProcess {
            driver: InProcessDriver::default(),
            admission_times: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn in_process_for_test(driver: InProcessDriver) -> Self {
        Self::InProcess {
            driver,
            admission_times: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(feature = "postgres")]
    pub fn postgres(pool: sqlx::PgPool) -> Self {
        Self::Postgres(PostgresDurableDriver::new(pool))
    }

    async fn stable_created_at_ms(
        &self,
        tenant_id: &str,
        message_id: &str,
    ) -> Result<u64, WebhookHandlingError> {
        match self {
            Self::InProcess {
                admission_times, ..
            } => {
                let mut times = admission_times
                    .lock()
                    .map_err(|_| WebhookHandlingError::Retryable("event_lane:time_cache".into()))?;
                Ok(*times
                    .entry(message_id.to_owned())
                    .or_insert_with(now_epoch_ms))
            }
            #[cfg(feature = "postgres")]
            Self::Postgres(driver) => {
                // If a process crashed after durable Event append but before
                // inbox completion, replay must reconstruct the exact same
                // envelope. Reuse the persisted P3 admission timestamp rather
                // than generating a materially different message with the same
                // deterministic MessageId.
                let existing: Option<i64> = sqlx::query_scalar(
                    "SELECT created_at_ms FROM interconnect_events WHERE message_id=$1 AND tenant_id=$2",
                )
                .bind(message_id)
                .bind(tenant_id)
                .fetch_optional(driver.pool())
                .await
                .map_err(|_| WebhookHandlingError::Retryable("event_lane:timestamp_lookup".into()))?;
                match existing {
                    Some(value) => u64::try_from(value).map_err(|_| {
                        WebhookHandlingError::Permanent(
                            "event_lane:invalid_persisted_timestamp".into(),
                        )
                    }),
                    None => Ok(now_epoch_ms()),
                }
            }
        }
    }

    async fn append(
        &self,
        ctx: &ExecutionContext,
        envelope: MessageEnvelope,
    ) -> Result<(), InterconnectError> {
        match self {
            Self::InProcess { driver, .. } => driver.append_event(ctx, envelope).map(|_| ()),
            #[cfg(feature = "postgres")]
            Self::Postgres(driver) => driver.append_event(ctx, envelope).await.map(|_| ()),
        }
    }
}

#[derive(Clone)]
pub struct InterconnectWebhookEventPort {
    lane: WebhookEventLane,
}

impl InterconnectWebhookEventPort {
    pub fn new(lane: WebhookEventLane) -> Self {
        Self { lane }
    }

    async fn build_admission(
        &self,
        event: &CanonicalWebhookEvent,
    ) -> Result<(ExecutionContext, MessageEnvelope, String, u64), WebhookHandlingError> {
        let identity = event_identity(event)?;
        let message_id = identity.message_id.clone();
        let created_at_ms = self
            .lane
            .stable_created_at_ms(&event.tenant_id, &message_id)
            .await?;
        let (ctx, envelope) = build_event_admission(event, identity, created_at_ms)?;
        Ok((ctx, envelope, message_id, created_at_ms))
    }
}

#[async_trait]
impl WebhookApplicationPort for InterconnectWebhookEventPort {
    async fn dispatch(&self, event: CanonicalWebhookEvent) -> Result<(), WebhookHandlingError> {
        let (ctx, envelope, message_id, first_created_at_ms) = self.build_admission(&event).await?;
        match self.lane.append(&ctx, envelope).await {
            Ok(()) => Ok(()),
            Err(error) if error.code == InterconnectErrorCode::ContractIncompatible => {
                // Two independent workers can both observe "no existing event"
                // before one wins the PostgreSQL insert. The loser then has a
                // different wall-clock created_at_ms and correctly receives a
                // P3 identity conflict. Re-read the winner's durable timestamp
                // and retry exactly once. A genuine material conflict remains a
                // conflict on the second append and is never silently accepted.
                let replay_created_at_ms = self
                    .lane
                    .stable_created_at_ms(&event.tenant_id, &message_id)
                    .await?;
                if replay_created_at_ms == first_created_at_ms {
                    return Err(map_interconnect_error(error));
                }
                let identity = event_identity(&event)?;
                let (replay_ctx, replay_envelope) =
                    build_event_admission(&event, identity, replay_created_at_ms)?;
                self.lane
                    .append(&replay_ctx, replay_envelope)
                    .await
                    .map_err(map_interconnect_error)
            }
            Err(error) => Err(map_interconnect_error(error)),
        }
    }
}

struct EventIdentity {
    subject: Subject,
    message_id: String,
    correlation_id: String,
    ordering_key: String,
}

fn event_identity(event: &CanonicalWebhookEvent) -> Result<EventIdentity, WebhookHandlingError> {
    // The plugin/mapper must emit a canonical P3 subject. Do not silently
    // rewrite arbitrary provider event names because that would create a second
    // subject-normalization truth outside Interconnect Core.
    let subject = Subject::new(&event.event_type)
        .map_err(|_| permanent("invalid_normalized_event_subject"))?;
    if event.payload_hash.len() != 64
        || !event
            .payload_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(permanent("invalid_normalized_payload_hash"));
    }

    let identity_material = format!(
        "{}\n{}\n{}\n{}\n{}",
        event.tenant_id,
        event.provider_id.as_str(),
        event.binding_id.as_str(),
        event.provider_event_id,
        event.payload_hash,
    );
    let identity_digest = hex::encode(Sha256::digest(identity_material.as_bytes()));
    let ordering_digest = hex::encode(Sha256::digest(event.binding_id.as_str().as_bytes()));
    Ok(EventIdentity {
        subject,
        message_id: format!("webhook:{identity_digest}"),
        correlation_id: format!("webhook:{identity_digest}"),
        ordering_key: format!("webhook-binding:{ordering_digest}"),
    })
}

fn build_event_admission(
    event: &CanonicalWebhookEvent,
    identity: EventIdentity,
    created_at_ms: u64,
) -> Result<(ExecutionContext, MessageEnvelope), WebhookHandlingError> {
    let tenant =
        TenantId::new(event.tenant_id.clone()).map_err(|_| permanent("invalid_webhook_tenant"))?;
    let context = ExecutionContext::new(
        ActorIdentity::system(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(
            tenant,
            Revision::new(WEBHOOK_EVENT_REVISION)
                .map_err(|_| permanent("invalid_webhook_revision"))?,
        )
        .map_err(|_| permanent("invalid_webhook_data_scope"))?,
        ExecutionMode::Normal,
        RequestId::new(identity.correlation_id.clone())
            .map_err(|_| permanent("invalid_webhook_correlation"))?,
        Some(identity.message_id.clone()),
        Arc::new(NoopHttpClient),
    )
    .map_err(|_| permanent("invalid_webhook_execution_context"))?;

    let envelope = MessageEnvelope {
        id: MessageId::new(identity.message_id.clone())
            .map_err(|_| permanent("invalid_webhook_message_id"))?,
        kind: MessageKind::Event,
        subject: identity.subject,
        contract: ContractBinding {
            contract: ContractRef::new(WEBHOOK_EVENT_CONTRACT)
                .map_err(|_| permanent("invalid_webhook_contract"))?,
            version: ContractVersion::new(WEBHOOK_EVENT_VERSION)
                .map_err(|_| permanent("invalid_webhook_contract_version"))?,
            schema: SchemaRef::new(WEBHOOK_EVENT_SCHEMA)
                .map_err(|_| permanent("invalid_webhook_schema"))?,
        },
        correlation_id: CorrelationId::new(identity.correlation_id)
            .map_err(|_| permanent("invalid_webhook_correlation"))?,
        causation_id: None,
        created_at_ms,
        deadline_ms: None,
        ordering_key: Some(
            OrderingKey::new(identity.ordering_key)
                .map_err(|_| permanent("invalid_webhook_ordering_key"))?,
        ),
        idempotency_key: Some(
            IdempotencyKey::new(identity.message_id)
                .map_err(|_| permanent("invalid_webhook_idempotency_key"))?,
        ),
        payload: PayloadRef::Inline(json!({
            "providerId": event.provider_id.as_str(),
            "bindingId": event.binding_id.as_str(),
            "providerEventId": event.provider_event_id.as_str(),
            "eventType": event.event_type.as_str(),
            "payloadHash": event.payload_hash.as_str(),
        })),
        extensions: Extensions::empty(),
    };
    envelope
        .validate()
        .map_err(|_| permanent("invalid_webhook_event_envelope"))?;
    Ok((context, envelope))
}

fn map_interconnect_error(error: InterconnectError) -> WebhookHandlingError {
    match error.code {
        InterconnectErrorCode::DriverFailure | InterconnectErrorCode::ResourceBudgetExceeded => {
            WebhookHandlingError::Retryable(format!("event_lane:{:?}", error.code))
        }
        _ => WebhookHandlingError::Permanent(format!("event_lane:{:?}", error.code)),
    }
}

fn permanent(classification: &str) -> WebhookHandlingError {
    WebhookHandlingError::Permanent(classification.to_owned())
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::integration::types::{ProviderBindingId, ProviderId};

    fn event() -> CanonicalWebhookEvent {
        CanonicalWebhookEvent {
            tenant_id: "tenant-a".into(),
            provider_id: ProviderId::new("fixture-webhooks").unwrap(),
            binding_id: ProviderBindingId::new("binding-a").unwrap(),
            provider_event_id: "provider-event-1".into(),
            event_type: "fixture.webhook.updated".into(),
            payload_hash: hex::encode(Sha256::digest(b"fixture")),
        }
    }

    #[tokio::test]
    async fn replay_is_idempotent_on_the_normalized_event_lane() {
        let driver = InProcessDriver::default();
        let port = InterconnectWebhookEventPort::new(WebhookEventLane::in_process_for_test(
            driver.clone(),
        ));
        let event = event();
        port.dispatch(event.clone()).await.unwrap();
        port.dispatch(event.clone()).await.unwrap();

        let identity = event_identity(&event).unwrap();
        let created_at = port
            .lane
            .stable_created_at_ms(&event.tenant_id, &identity.message_id)
            .await
            .unwrap();
        let (ctx, _) = build_event_admission(&event, identity, created_at).unwrap();
        let records = driver
            .replay_events(
                &ctx,
                system_core::transport::interconnect::ConsumerId::new("fixture-consumer").unwrap(),
                Subject::new("fixture.webhook.updated").unwrap(),
                0,
                10,
            )
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].envelope.kind, MessageKind::Event);
        assert_eq!(
            records[0].envelope.contract.contract.as_str(),
            WEBHOOK_EVENT_CONTRACT
        );
    }

    #[tokio::test]
    async fn invalid_provider_event_subject_fails_before_event_admission() {
        let driver = InProcessDriver::default();
        let port = InterconnectWebhookEventPort::new(WebhookEventLane::in_process_for_test(driver));
        let mut invalid = event();
        invalid.event_type = "provider/event/*".into();
        assert!(matches!(
            port.dispatch(invalid).await,
            Err(WebhookHandlingError::Permanent(ref class))
                if class == "invalid_normalized_event_subject"
        ));
    }

    #[test]
    fn envelope_contains_no_raw_webhook_or_signature_material() {
        let event = event();
        let identity = event_identity(&event).unwrap();
        let (_, envelope) = build_event_admission(&event, identity, 123).unwrap();
        let encoded = serde_json::to_string(&envelope).unwrap();
        assert!(!encoded.contains("fixture-signed"));
        assert!(!encoded.contains("verification_headers"));
        assert!(encoded.contains("payloadHash"));
        assert_eq!(envelope.created_at_ms, 123);
    }
}
