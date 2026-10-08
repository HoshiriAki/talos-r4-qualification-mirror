use super::types::{
    IntegrationError, ProviderBindingId, ProviderId, WebhookEndpointId, WebhookInboxId,
};
use super::webhook::{ClaimedWebhook, WebhookEndpointContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookEndpointSummary {
    pub endpoint_id: WebhookEndpointId,
    pub binding_id: ProviderBindingId,
    pub provider_id: ProviderId,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookDeadLetterSummary {
    pub inbox_id: WebhookInboxId,
    pub endpoint_id: WebhookEndpointId,
    pub provider_event_id: String,
    pub reason: String,
    pub replay_count: u64,
    pub created_at: String,
    pub replayed_at: Option<String>,
}

pub(crate) trait WebhookReplayPersistence: Send + Sync {
    fn replay_webhook(
        &self,
        tenant_id: &str,
        inbox_id: &WebhookInboxId,
        actor_ref: &str,
        reason: &str,
    ) -> Result<(), IntegrationError>;
}

pub(crate) trait WebhookRuntimePersistence: WebhookReplayPersistence {
    fn resolve_webhook_endpoint(
        &self,
        token: &[u8],
    ) -> Result<WebhookEndpointContext, IntegrationError>;

    fn record_verified_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
    ) -> Result<(WebhookInboxId, bool), IntegrationError>;

    fn record_rejected_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
        classification: &str,
    ) -> Result<(), IntegrationError>;

    fn claim_next_webhook(
        &self,
        tenant_id: &str,
    ) -> Result<Option<ClaimedWebhook>, IntegrationError>;

    fn complete_webhook(
        &self,
        webhook: &ClaimedWebhook,
        canonical_event_type: &str,
    ) -> Result<(), IntegrationError>;

    fn retry_webhook(
        &self,
        webhook: &ClaimedWebhook,
        classification: &str,
    ) -> Result<(), IntegrationError>;

    fn dead_letter_webhook(
        &self,
        webhook: &ClaimedWebhook,
        reason: &str,
    ) -> Result<(), IntegrationError>;
}

pub(crate) trait WebhookAdminPersistence: WebhookReplayPersistence {
    fn register_webhook_endpoint(
        &self,
        tenant_id: &str,
        binding_id: &ProviderBindingId,
        endpoint_token: &[u8],
    ) -> Result<WebhookEndpointId, IntegrationError>;

    fn list_webhook_endpoints(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookEndpointSummary>, IntegrationError>;

    fn set_webhook_endpoint_enabled(
        &self,
        tenant_id: &str,
        endpoint_id: &WebhookEndpointId,
        enabled: bool,
    ) -> Result<(), IntegrationError>;

    fn list_webhook_dead_letters(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookDeadLetterSummary>, IntegrationError>;
}
