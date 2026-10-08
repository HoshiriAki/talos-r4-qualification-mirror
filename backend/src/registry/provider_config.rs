//! Legacy provider-module activation is deliberately disabled.
//!
//! Provider configuration used to be read from process environment variables
//! and copied into `SystemModule::init` JSON. That process-wide configuration
//! had no tenant binding and could carry plaintext secrets. Stage 2 moves
//! operational provider selection to the Integration Fabric's persisted
//! ProviderInstance/ProviderBinding and KeyStore boundaries. The old provider
//! crates remain compiled workspace artifacts for deferred SF/WeChat work, but
//! are not eligible for Registry activation.

use super::descriptors::ProviderId;

#[derive(Clone, Default)]
pub struct ProviderDeploymentConfig;

impl ProviderDeploymentConfig {
    /// A manifest is descriptive; it cannot make a connector operational.
    pub fn is_configured(&self, _provider: ProviderId) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderDeploymentConfig, ProviderId};

    #[test]
    fn legacy_provider_modules_cannot_be_activated_by_process_configuration() {
        for provider in [
            ProviderId::SfExpress,
            ProviderId::WechatPay,
            ProviderId::Alipay,
            ProviderId::Miniapp,
        ] {
            assert!(!ProviderDeploymentConfig.is_configured(provider));
        }
    }
}
