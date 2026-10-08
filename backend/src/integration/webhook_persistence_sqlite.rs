use super::store::IntegrationStore;
use super::types::{IntegrationError, ProviderBindingId, WebhookEndpointId, WebhookInboxId};
use super::webhook::{ClaimedWebhook, WebhookEndpointContext};
use super::webhook_persistence_contract::{
    WebhookAdminPersistence, WebhookDeadLetterSummary, WebhookEndpointSummary,
    WebhookReplayPersistence, WebhookRuntimePersistence,
};

impl WebhookReplayPersistence for IntegrationStore {
    fn replay_webhook(
        &self,
        tenant_id: &str,
        inbox_id: &WebhookInboxId,
        actor_ref: &str,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::replay_webhook(self, tenant_id, inbox_id, actor_ref, reason)
    }
}

impl WebhookRuntimePersistence for IntegrationStore {
    fn resolve_webhook_endpoint(
        &self,
        token: &[u8],
    ) -> Result<WebhookEndpointContext, IntegrationError> {
        IntegrationStore::resolve_webhook_endpoint(self, token)
    }

    fn record_verified_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
    ) -> Result<(WebhookInboxId, bool), IntegrationError> {
        IntegrationStore::record_verified_webhook(
            self,
            endpoint,
            provider_event_id,
            headers_json,
            raw_payload,
        )
    }

    fn record_rejected_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
        classification: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::record_rejected_webhook(
            self,
            endpoint,
            provider_event_id,
            headers_json,
            raw_payload,
            classification,
        )
    }

    fn claim_next_webhook(
        &self,
        tenant_id: &str,
    ) -> Result<Option<ClaimedWebhook>, IntegrationError> {
        IntegrationStore::claim_next_webhook(self, tenant_id)
    }

    fn complete_webhook(
        &self,
        webhook: &ClaimedWebhook,
        canonical_event_type: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::complete_webhook(self, webhook, canonical_event_type)
    }

    fn retry_webhook(
        &self,
        webhook: &ClaimedWebhook,
        classification: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::retry_webhook(self, webhook, classification)
    }

    fn dead_letter_webhook(
        &self,
        webhook: &ClaimedWebhook,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::dead_letter_webhook(self, webhook, reason)
    }
}

impl WebhookAdminPersistence for IntegrationStore {
    fn register_webhook_endpoint(
        &self,
        tenant_id: &str,
        binding_id: &ProviderBindingId,
        endpoint_token: &[u8],
    ) -> Result<WebhookEndpointId, IntegrationError> {
        IntegrationStore::register_webhook_endpoint(self, tenant_id, binding_id, endpoint_token)
    }

    fn list_webhook_endpoints(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookEndpointSummary>, IntegrationError> {
        IntegrationStore::list_webhook_endpoints(self, tenant_id)
    }

    fn set_webhook_endpoint_enabled(
        &self,
        tenant_id: &str,
        endpoint_id: &WebhookEndpointId,
        enabled: bool,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::set_webhook_endpoint_enabled(self, tenant_id, endpoint_id, enabled)
    }

    fn list_webhook_dead_letters(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookDeadLetterSummary>, IntegrationError> {
        IntegrationStore::list_webhook_dead_letters(self, tenant_id)
    }
}
