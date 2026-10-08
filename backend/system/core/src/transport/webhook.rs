//! Legacy webhook-listener tombstone.
//!
//! R4-P5 retires the old provider-owned `WebhookHandler` / `path_prefix`
//! listener model. Provider plugins must not register arbitrary HTTP listeners
//! inside Core. The authoritative callback flow is now:
//!
//! Internet -> bounded `/api/integrations/webhooks/{endpoint_token}` ingress
//! -> endpoint/binding resolution -> raw-byte verifier hook -> durable inbox
//! -> normalized R4-P3 Event Lane.
//!
//! This module path remains temporarily so historical imports and repository
//! evidence do not turn into an unrelated source-layout migration. It contains
//! no executable listener, parser, signature verifier, route prefix, or network
//! authority. P6 plugin verifier/decode hooks live behind the bounded ingress.

/// Marker documenting that provider-owned listener registration is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderOwnedWebhookListenerRetired;
