use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IntegrationError {
    #[error("integration value {kind} must not be blank")]
    BlankValue { kind: &'static str },
    #[error("integration capability is not declared by the provider manifest")]
    CapabilityNotDeclared,
    #[error("provider manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("required secret reference is missing")]
    RequiredSecretMissing,
    #[error("secret reference must be a KeyStore URI")]
    InvalidSecretReference,
    #[error("provider binding is unavailable")]
    BindingUnavailable,
    #[error("provider instance is not active and ready")]
    InstanceNotReady,
    #[error("integration execution mode is blocked")]
    ModeBlocked,
    #[error("integration operation transition is invalid")]
    InvalidOperationTransition,
    #[error("provider binding revision changed before dispatch")]
    BindingRevisionStale,
    #[error("external operation is not due for retry")]
    OperationNotDue,
    #[error("external operation circuit is open")]
    CircuitOpen,
    #[error("external operation concurrency limit reached")]
    ConcurrencyLimited,
    #[error("external operation rate limit reached")]
    RateLimited,
    #[error("deposit or refund currency does not match")]
    CurrencyMismatch,
    #[error("refund amount exceeds the available deposit balance")]
    RefundAmountExceedsAvailable,
    #[error("refund operation is unavailable")]
    RefundOperationUnavailable,
    #[error("secret access is denied")]
    SecretAccessDenied,
    #[error("secret reference is unavailable")]
    SecretUnavailable,
    #[error("integration transport policy denied the request")]
    EgressDenied,
    #[error("integration transport request is invalid")]
    InvalidTransportRequest,
    #[error("integration response exceeded the configured limit")]
    ResponseTooLarge,
    #[error("integration transport timed out")]
    Timeout,
    #[error("integration transport failed")]
    TransportFailure,
    #[error("integration persistence failed")]
    Persistence,
    #[error("webhook verification failed")]
    WebhookUnverifiable,
}

macro_rules! stable_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IntegrationError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IntegrationError::BlankValue { kind: $kind });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

stable_id!(ProviderId, "provider id");
stable_id!(ProviderInstanceId, "provider instance id");
stable_id!(ProviderBindingId, "provider binding id");
stable_id!(CapabilityId, "capability id");
stable_id!(ExternalOperationId, "external operation id");
stable_id!(ExternalAttemptId, "external attempt id");
stable_id!(WebhookEndpointId, "webhook endpoint id");
stable_id!(WebhookInboxId, "webhook inbox id");
stable_id!(DepositId, "deposit id");
stable_id!(RefundId, "refund id");

/// Opaque KeyStore identifier persisted with an integration instance.
///
/// A provider credential is never accepted as a reference.  The only wire
/// form is `keystore://fixture/<opaque-id>` or
/// `keystore://deployment/<opaque-id>`; persistence additionally verifies
/// that the configured KeyStore has registered that reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn new(value: impl Into<String>) -> Result<Self, IntegrationError> {
        let value = value.into();
        let Some(path) = value.strip_prefix("keystore://") else {
            return Err(IntegrationError::InvalidSecretReference);
        };
        let Some((environment, opaque_id)) = path.split_once('/') else {
            return Err(IntegrationError::InvalidSecretReference);
        };
        if !matches!(environment, "fixture" | "deployment")
            || opaque_id.is_empty()
            || opaque_id.starts_with('/')
            || opaque_id.ends_with('/')
            || opaque_id.contains("//")
            || !opaque_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/')
            })
        {
            return Err(IntegrationError::InvalidSecretReference);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReadiness {
    Stub,
    Fixture,
    Sandbox,
    Production,
}

impl ProviderReadiness {
    pub fn permits_dispatch(&self) -> bool {
        // Stage 2 has no real sandbox or production connector.  Configuration
        // records may carry those readiness declarations, but only an explicit
        // fixture instance can enter the dispatch boundary.
        self.is_fixture()
    }

    pub fn is_fixture(&self) -> bool {
        matches!(self, Self::Fixture)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLifecycle {
    Draft,
    Active,
    Suspended,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealth {
    Unknown,
    Ready,
    Degraded,
    Unhealthy,
}

/// The only values permitted in normal Provider configuration storage. There
/// is deliberately no arbitrary text or bytes variant: credentials belong in
/// `SecretRef` and are resolved only by KeyStore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigValueType {
    HttpsOrigin,
    Boolean,
    PositiveInteger,
}

impl ConfigValueType {
    pub fn accepts(&self, value: &NonSecretConfigValue) -> bool {
        matches!(
            (self, value),
            (Self::HttpsOrigin, NonSecretConfigValue::HttpsOrigin(_))
                | (Self::Boolean, NonSecretConfigValue::Boolean(_))
                | (
                    Self::PositiveInteger,
                    NonSecretConfigValue::PositiveInteger(_)
                )
        )
    }
}

/// A canonical HTTPS origin. Query parameters, credentials, fragments, and
/// non-root paths are excluded so configuration cannot smuggle credential
/// material through an endpoint string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HttpsOrigin(String);

impl HttpsOrigin {
    pub fn new(value: impl Into<String>) -> Result<Self, IntegrationError> {
        let value = value.into();
        let parsed = Url::parse(&value)
            .map_err(|_| IntegrationError::InvalidManifest("invalid HTTPS origin".into()))?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !matches!(parsed.path(), "" | "/")
        {
            return Err(IntegrationError::InvalidManifest(
                "configuration HTTPS origin must not contain credentials, query, fragment, or path"
                    .into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HttpsOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct PositiveInteger(u64);

impl PositiveInteger {
    pub fn new(value: u64) -> Result<Self, IntegrationError> {
        if value == 0 {
            return Err(IntegrationError::InvalidManifest(
                "positive integer configuration must be greater than zero".into(),
            ));
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for PositiveInteger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum NonSecretConfigValue {
    HttpsOrigin(HttpsOrigin),
    Boolean(bool),
    PositiveInteger(PositiveInteger),
}

impl<'de> Deserialize<'de> for NonSecretConfigValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(value) => HttpsOrigin::new(value)
                .map(Self::HttpsOrigin)
                .map_err(serde::de::Error::custom),
            serde_json::Value::Bool(value) => Ok(Self::Boolean(value)),
            serde_json::Value::Number(value) => value
                .as_u64()
                .ok_or_else(|| serde::de::Error::custom("configuration integer must be positive"))
                .and_then(|value| {
                    PositiveInteger::new(value)
                        .map(Self::PositiveInteger)
                        .map_err(serde::de::Error::custom)
                }),
            _ => Err(serde::de::Error::custom(
                "configuration must be an HTTPS origin, boolean, or positive integer",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigField {
    pub name: String,
    pub required: bool,
    pub value_type: ConfigValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretValueType {
    OpaqueCredential,
    SigningKey,
}

/// The bounded reason that a Provider needs a KeyStore reference.  This is
/// deliberately an enum rather than a free-form description so a manifest
/// declaration can never become another persisted credential channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretPurpose {
    ProviderAuthentication,
    WebhookVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretRequirement {
    pub name: String,
    pub required: bool,
    pub value_type: SecretValueType,
    pub purpose: SecretPurpose,
}

/// An API family version uses a deliberately narrow public form (`v1`,
/// `v1.2`, or `v1.2.3`).  It cannot carry an opaque Provider value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ApiVersion(String);

impl ApiVersion {
    pub fn new(value: impl Into<String>) -> Result<Self, IntegrationError> {
        let value = value.into();
        let numeric = value.strip_prefix('v').ok_or_else(|| {
            IntegrationError::InvalidManifest("API version must start with v".into())
        })?;
        if numeric.is_empty()
            || numeric.split('.').any(|part| {
                part.is_empty()
                    || !part.chars().all(|character| character.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
            })
        {
            return Err(IntegrationError::InvalidManifest(
                "API version must use v followed by numeric segments".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ApiVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Compatibility state is a declaration, not a provider-authored text blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityRule {
    BackwardCompatible,
    RequiresMigration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderManifest {
    pub provider_id: ProviderId,
    pub version: String,
    pub capabilities: BTreeSet<CapabilityId>,
    pub config_schema: Vec<ConfigField>,
    pub secret_schema: Vec<SecretRequirement>,
    pub api_versions: BTreeMap<CapabilityId, ApiVersion>,
    pub webhook_types: BTreeSet<String>,
    /// Simulation-capable operations are declared explicitly. This is only a
    /// fixture/stub declaration; it never authorizes a real external call.
    #[serde(default)]
    pub simulation_capabilities: BTreeSet<CapabilityId>,
    pub readiness: ProviderReadiness,
    pub compatibility: BTreeMap<CapabilityId, CompatibilityRule>,
}

impl ProviderManifest {
    pub fn validate(&self) -> Result<(), IntegrationError> {
        if self.version.trim().is_empty() {
            return Err(IntegrationError::InvalidManifest(
                "version must not be blank".into(),
            ));
        }
        if self.capabilities.is_empty() {
            return Err(IntegrationError::InvalidManifest(
                "at least one capability is required".into(),
            ));
        }
        ensure_unique_nonblank(
            self.config_schema.iter().map(|field| field.name.as_str()),
            "config field",
        )?;
        ensure_unique_nonblank(
            self.secret_schema.iter().map(|field| field.name.as_str()),
            "secret requirement",
        )?;
        let config_names = self
            .config_schema
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        if self
            .secret_schema
            .iter()
            .any(|requirement| config_names.contains(requirement.name.as_str()))
        {
            return Err(IntegrationError::InvalidManifest(
                "configuration and secret requirement names must not overlap".into(),
            ));
        }
        if self
            .api_versions
            .keys()
            .any(|capability| !self.capabilities.contains(capability))
        {
            return Err(IntegrationError::InvalidManifest(
                "API versions must refer to declared capabilities".into(),
            ));
        }
        if self
            .compatibility
            .keys()
            .any(|capability| !self.capabilities.contains(capability))
        {
            return Err(IntegrationError::InvalidManifest(
                "compatibility rules must refer to declared capabilities".into(),
            ));
        }
        if self
            .webhook_types
            .iter()
            .any(|event| event.trim().is_empty())
        {
            return Err(IntegrationError::InvalidManifest(
                "webhook event types must not be blank".into(),
            ));
        }
        if !self
            .simulation_capabilities
            .iter()
            .all(|capability| self.capabilities.contains(capability))
        {
            return Err(IntegrationError::InvalidManifest(
                "simulation capabilities must be declared capabilities".into(),
            ));
        }
        Ok(())
    }

    pub fn declares(&self, capability: &CapabilityId) -> bool {
        self.capabilities.contains(capability)
    }

    /// A manifest communicates shape and readiness only. It never activates a
    /// connector without a tenant-scoped instance and enabled binding.
    pub fn is_operational_on_its_own(&self) -> bool {
        false
    }
}

fn ensure_unique_nonblank<'a>(
    names: impl IntoIterator<Item = &'a str>,
    label: &str,
) -> Result<(), IntegrationError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if name.trim().is_empty() || !seen.insert(name) {
            return Err(IntegrationError::InvalidManifest(format!(
                "{label} names must be nonblank and unique"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInstance {
    pub id: ProviderInstanceId,
    pub tenant_id: String,
    pub provider_id: ProviderId,
    pub manifest_version: String,
    pub config_revision: String,
    pub config: BTreeMap<String, NonSecretConfigValue>,
    pub secret_refs: BTreeMap<String, SecretRef>,
    pub lifecycle: ProviderLifecycle,
    pub health: ProviderHealth,
    pub readiness: ProviderReadiness,
}

impl ProviderInstance {
    pub fn validate(&self) -> Result<(), IntegrationError> {
        if self.tenant_id.trim().is_empty()
            || self.manifest_version.trim().is_empty()
            || self.config_revision.trim().is_empty()
            || self.config.keys().any(|key| key.trim().is_empty())
            || self.secret_refs.keys().any(|key| key.trim().is_empty())
        {
            return Err(IntegrationError::InvalidManifest(
                "instance tenant, version, revision, and configuration keys must not be blank"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderBinding {
    pub id: ProviderBindingId,
    pub tenant_id: String,
    pub provider_instance_id: ProviderInstanceId,
    pub capability: CapabilityId,
    pub config_revision: String,
    pub enabled: bool,
}

impl ProviderBinding {
    pub fn validate(&self) -> Result<(), IntegrationError> {
        if self.tenant_id.trim().is_empty() || self.config_revision.trim().is_empty() {
            return Err(IntegrationError::InvalidManifest(
                "binding tenant and revision must not be blank".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProviderBinding {
    pub manifest: ProviderManifest,
    pub instance: ProviderInstance,
    pub binding: ProviderBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    pub minor: i64,
    pub currency: String,
}

impl Money {
    pub fn new(minor: i64, currency: impl Into<String>) -> Result<Self, IntegrationError> {
        let currency = currency.into();
        if currency.trim().is_empty() {
            return Err(IntegrationError::BlankValue { kind: "currency" });
        }
        Ok(Self { minor, currency })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        ApiVersion, CapabilityId, CompatibilityRule, ConfigField, ConfigValueType, HttpsOrigin,
        IntegrationError, ProviderId, ProviderManifest, ProviderReadiness, SecretPurpose,
        SecretRef, SecretRequirement, SecretValueType,
    };

    #[test]
    fn secret_ref_accepts_only_opaque_keystore_uris() {
        assert!(SecretRef::new("keystore://fixture/provider-key-a").is_ok());
        assert!(SecretRef::new("keystore://deployment/provider-key-a").is_ok());
        assert_eq!(
            SecretRef::new("raw-secret-value").unwrap_err(),
            IntegrationError::InvalidSecretReference
        );
        assert_eq!(
            SecretRef::new("secret://fixture/provider-key-a").unwrap_err(),
            IntegrationError::InvalidSecretReference
        );
    }

    #[test]
    fn secret_ref_deserialization_cannot_bypass_validation() {
        let parsed = serde_json::from_str::<SecretRef>(r#""raw-secret-value""#);
        assert!(parsed.is_err());
    }

    #[test]
    fn manifest_rejects_plaintext_secret_paths_in_configuration_schema() {
        let base = || ProviderManifest {
            provider_id: ProviderId::new("fixture-payment").unwrap(),
            version: "1".into(),
            capabilities: BTreeSet::from([CapabilityId::new("fixture.payment").unwrap()]),
            config_schema: vec![ConfigField {
                name: "endpoint".into(),
                required: true,
                value_type: ConfigValueType::HttpsOrigin,
            }],
            secret_schema: vec![SecretRequirement {
                name: "api_key".into(),
                required: true,
                value_type: SecretValueType::OpaqueCredential,
                purpose: SecretPurpose::ProviderAuthentication,
            }],
            api_versions: BTreeMap::new(),
            webhook_types: BTreeSet::new(),
            simulation_capabilities: BTreeSet::new(),
            readiness: ProviderReadiness::Fixture,
            compatibility: BTreeMap::new(),
        };

        let mut overlapping = base();
        overlapping.config_schema[0].name = "api_key".into();
        assert!(matches!(
            overlapping.validate(),
            Err(IntegrationError::InvalidManifest(_))
        ));

        assert!(HttpsOrigin::new("https://user:password@example.invalid").is_err());
        assert!(HttpsOrigin::new("https://example.invalid/?token=raw").is_err());
        assert!(HttpsOrigin::new("https://example.invalid/provider-secret").is_err());
        assert!(
            serde_json::from_str::<ConfigField>(
                r#"{"name":"free-form","required":false,"valueType":"string"}"#,
            )
            .is_err()
        );
        assert!(serde_json::from_str::<SecretRequirement>(
            r#"{"name":"api_key","required":true,"schemaRef":"secret/string","purpose":"fixture dispatch"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ApiVersion>(r#""credential-value""#).is_err());

        let mut undeclared_api_version = base();
        undeclared_api_version.api_versions = BTreeMap::from([(
            CapabilityId::new("fixture.undeclared").unwrap(),
            ApiVersion::new("v1").unwrap(),
        )]);
        assert!(matches!(
            undeclared_api_version.validate(),
            Err(IntegrationError::InvalidManifest(_))
        ));

        let mut undeclared_compatibility = base();
        undeclared_compatibility.compatibility = BTreeMap::from([(
            CapabilityId::new("fixture.undeclared").unwrap(),
            CompatibilityRule::BackwardCompatible,
        )]);
        assert!(matches!(
            undeclared_compatibility.validate(),
            Err(IntegrationError::InvalidManifest(_))
        ));
    }
}
