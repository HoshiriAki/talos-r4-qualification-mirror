//! TALOS R4 Interconnect Fabric stable semantic contract.
//!
//! This module is intentionally transport-neutral. It defines the message,
//! subject, contract, content-reference, capability and durable-lane vocabulary
//! that drivers and plugins may implement without becoming a second business
//! authority. Trusted Principal / tenant / role / execution-plane authority is
//! deliberately absent from `MessageEnvelope`; callers pass `ExecutionContext`
//! out-of-band at the governed runtime boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const MAX_SUBJECT_BYTES: usize = 192;
pub const MAX_SUBJECT_SEGMENTS: usize = 16;
pub const MAX_SUBJECT_SEGMENT_BYTES: usize = 48;
pub const MAX_INLINE_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_EXTENSION_BYTES: usize = 4 * 1024;
pub const MAX_EXTENSION_KEYS: usize = 32;
pub const MAX_CONTENT_LOCATIONS: usize = 4;
pub const MAX_CONTENT_LOCATION_BYTES: usize = 2048;
pub const MAX_CONTENT_BYTES: u64 = 1_099_511_627_776; // 1 TiB bounded reference seam.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InterconnectErrorCode {
    InvalidIdentifier,
    InvalidSubject,
    InvalidContractVersion,
    InvalidSchema,
    InvalidPayload,
    InvalidContentRef,
    InvalidExtensions,
    CapabilityUnavailable,
    ContractIncompatible,
    SchemaIncompatible,
    DeadlineExceeded,
    Cancelled,
    CursorExpired,
    StaleClaimGeneration,
    PolicyDenied,
    ResourceBudgetExceeded,
    DriverFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterconnectError {
    pub code: InterconnectErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl InterconnectError {
    pub fn new(code: InterconnectErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

impl fmt::Display for InterconnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for InterconnectError {}

fn validate_identifier(
    value: &str,
    kind: &'static str,
    max: usize,
) -> Result<(), InterconnectError> {
    if value.is_empty()
        || value.len() > max
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(InterconnectError::new(
            InterconnectErrorCode::InvalidIdentifier,
            format!("{kind} must be 1..={max} printable ASCII bytes without whitespace"),
        ));
    }
    Ok(())
}

macro_rules! bounded_string_id {
    ($name:ident, $kind:literal, $max:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, InterconnectError> {
                let value = value.into();
                validate_identifier(&value, $kind, $max)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

bounded_string_id!(MessageId, "message id", 128);
bounded_string_id!(CorrelationId, "correlation id", 128);
bounded_string_id!(CausationId, "causation id", 128);
bounded_string_id!(OrderingKey, "ordering key", 192);
bounded_string_id!(IdempotencyKey, "idempotency key", 192);
bounded_string_id!(ContractRef, "contract ref", 192);
bounded_string_id!(SchemaRef, "schema ref", 192);
bounded_string_id!(ConsumerId, "consumer id", 128);
bounded_string_id!(LeaseOwner, "lease owner", 128);
bounded_string_id!(PluginId, "plugin id", 192);
bounded_string_id!(ProviderInstanceRef, "provider instance ref", 192);
bounded_string_id!(BindingRevisionRef, "binding revision", 192);
bounded_string_id!(AccessPolicyRef, "access policy ref", 192);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ContractVersion(String);

impl ContractVersion {
    pub fn new(value: impl Into<String>) -> Result<Self, InterconnectError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value.is_ascii()
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_')
            })
            || !value.bytes().any(|byte| byte.is_ascii_digit())
        {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidContractVersion,
                "contract version must be a bounded semver-compatible ASCII token",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ContractVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Exact canonical subject used by R4 stable core.
///
/// Grammar is case-insensitive at input and canonicalized to lowercase:
/// `segment(.segment)*`, up to 16 segments, `[a-z0-9_-]` per segment.
/// Wildcards are intentionally unsupported in the stable R4 subject type. A
/// future wildcard subscription facet therefore cannot silently broaden an R4
/// exact-subject ACL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Subject(String);

impl Subject {
    pub fn new(value: impl AsRef<str>) -> Result<Self, InterconnectError> {
        let canonical = value.as_ref().trim().to_ascii_lowercase();
        if canonical.is_empty() || canonical.len() > MAX_SUBJECT_BYTES || !canonical.is_ascii() {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidSubject,
                "subject is empty, non-ASCII, or too long",
            ));
        }
        let segments: Vec<&str> = canonical.split('.').collect();
        if segments.is_empty() || segments.len() > MAX_SUBJECT_SEGMENTS {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidSubject,
                "subject has an invalid segment count",
            ));
        }
        if segments.iter().any(|segment| {
            segment.is_empty()
                || segment.len() > MAX_SUBJECT_SEGMENT_BYTES
                || !segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
        }) {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidSubject,
                "subject segments must match [a-z0-9_-]+ and remain bounded",
            ));
        }
        Ok(Self(canonical))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_reserved(&self) -> bool {
        matches!(
            self.0.split('.').next(),
            Some("talos") | Some("system") | Some("security") | Some("control")
        )
    }
}

impl<'de> Deserialize<'de> for Subject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Query,
    Command,
    Event,
    Work,
    Effect,
    StateSnapshot,
    StateDelta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractBinding {
    pub contract: ContractRef,
    pub version: ContractVersion,
    pub schema: SchemaRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestAlgorithm {
    Sha256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentRef {
    pub algorithm: DigestAlgorithm,
    pub digest: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub codec: Option<String>,
    pub schema: Option<SchemaRef>,
    /// Resolution hints only. A location is never content identity or trusted
    /// fetch authority and must be mediated by the network/storage gateway.
    pub locations: Vec<String>,
    pub access_policy: AccessPolicyRef,
}

impl ContentRef {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        algorithm: DigestAlgorithm,
        digest: impl Into<String>,
        size_bytes: u64,
        media_type: impl Into<String>,
        codec: Option<String>,
        schema: Option<SchemaRef>,
        locations: Vec<String>,
        access_policy: AccessPolicyRef,
    ) -> Result<Self, InterconnectError> {
        let value = Self {
            algorithm,
            digest: digest.into().to_ascii_lowercase(),
            size_bytes,
            media_type: media_type.into(),
            codec,
            schema,
            locations,
            access_policy,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), InterconnectError> {
        let expected_digest_len = match self.algorithm {
            DigestAlgorithm::Sha256 => 64,
        };
        if self.digest.len() != expected_digest_len
            || !self.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.digest.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidContentRef,
                "content digest does not match the declared digest algorithm",
            ));
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_CONTENT_BYTES {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidContentRef,
                "content size is outside the bounded R4 content-reference budget",
            ));
        }
        validate_identifier(&self.media_type, "content media type", 128)?;
        if let Some(codec) = &self.codec {
            validate_identifier(codec, "content codec", 64)?;
        }
        if self.locations.len() > MAX_CONTENT_LOCATIONS
            || self.locations.iter().any(|location| {
                location.is_empty()
                    || location.len() > MAX_CONTENT_LOCATION_BYTES
                    || location.chars().any(char::is_control)
            })
        {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidContentRef,
                "content location hints exceed the bounded reference budget",
            ));
        }
        Ok(())
    }

    pub fn stable_identity(&self) -> String {
        match self.algorithm {
            DigestAlgorithm::Sha256 => format!("sha256:{}:{}", self.digest, self.size_bytes),
        }
    }
}

impl<'de> Deserialize<'de> for ContentRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            algorithm: DigestAlgorithm,
            digest: String,
            size_bytes: u64,
            media_type: String,
            codec: Option<String>,
            schema: Option<SchemaRef>,
            locations: Vec<String>,
            access_policy: AccessPolicyRef,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(
            raw.algorithm,
            raw.digest,
            raw.size_bytes,
            raw.media_type,
            raw.codec,
            raw.schema,
            raw.locations,
            raw.access_policy,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "storage", content = "value", rename_all = "snake_case")]
pub enum PayloadRef {
    Inline(Value),
    Content(ContentRef),
}

impl PayloadRef {
    pub fn validate(&self) -> Result<(), InterconnectError> {
        match self {
            Self::Inline(value) => {
                let encoded = serde_json::to_vec(value).map_err(|_| {
                    InterconnectError::new(
                        InterconnectErrorCode::InvalidPayload,
                        "inline payload cannot be encoded",
                    )
                })?;
                if encoded.len() > MAX_INLINE_PAYLOAD_BYTES {
                    return Err(InterconnectError::new(
                        InterconnectErrorCode::ResourceBudgetExceeded,
                        "inline payload exceeds the R4 message budget; use ContentRef",
                    ));
                }
                Ok(())
            }
            Self::Content(reference) => reference.validate(),
        }
    }
}

const RESERVED_EXTENSION_KEYS: &[&str] = &[
    "tenant_id",
    "tenant",
    "principal",
    "identity",
    "role",
    "roles",
    "authority",
    "platform_capability",
    "execution_plane",
    "execution_mode",
    "data_scope",
    "execution_context",
    "plugin_grant",
];

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
pub struct Extensions(BTreeMap<String, Value>);

impl Extensions {
    pub fn new(values: BTreeMap<String, Value>) -> Result<Self, InterconnectError> {
        let extension = Self(values);
        extension.validate()?;
        Ok(extension)
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn as_map(&self) -> &BTreeMap<String, Value> {
        &self.0
    }

    pub fn validate(&self) -> Result<(), InterconnectError> {
        if self.0.len() > MAX_EXTENSION_KEYS {
            return Err(InterconnectError::new(
                InterconnectErrorCode::InvalidExtensions,
                "too many envelope extensions",
            ));
        }
        for key in self.0.keys() {
            if key.is_empty()
                || key.len() > 64
                || !key.is_ascii()
                || !key.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-' | b'.')
                })
                || RESERVED_EXTENSION_KEYS.contains(&key.as_str())
            {
                return Err(InterconnectError::new(
                    InterconnectErrorCode::InvalidExtensions,
                    format!("extension key {key:?} is invalid or authority-reserved"),
                ));
            }
        }
        let encoded = serde_json::to_vec(&self.0).map_err(|_| {
            InterconnectError::new(
                InterconnectErrorCode::InvalidExtensions,
                "extensions cannot be encoded",
            )
        })?;
        if encoded.len() > MAX_EXTENSION_BYTES {
            return Err(InterconnectError::new(
                InterconnectErrorCode::ResourceBudgetExceeded,
                "envelope extensions exceed the bounded R4 budget",
            ));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Extensions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<String, Value>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub id: MessageId,
    pub kind: MessageKind,
    pub subject: Subject,
    pub contract: ContractBinding,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
    /// Unix epoch milliseconds. Stored as a scalar so the semantic contract is
    /// independent of chrono/timezone libraries and wire formats.
    pub created_at_ms: u64,
    pub deadline_ms: Option<u64>,
    pub ordering_key: Option<OrderingKey>,
    pub idempotency_key: Option<IdempotencyKey>,
    pub payload: PayloadRef,
    pub extensions: Extensions,
}

impl MessageEnvelope {
    pub fn validate(&self) -> Result<(), InterconnectError> {
        self.payload.validate()?;
        self.extensions.validate()?;
        if self
            .deadline_ms
            .is_some_and(|deadline| deadline < self.created_at_ms)
        {
            return Err(InterconnectError::new(
                InterconnectErrorCode::DeadlineExceeded,
                "message deadline precedes message creation",
            ));
        }
        Ok(())
    }

    /// CloudEvents attribute mapping for public Event representation. TALOS
    /// keeps its internal wire format independent; payload/data encoding and
    /// RFC3339 time formatting are performed by the public API adapter.
    pub fn cloudevents_attributes(&self) -> BTreeMap<String, String> {
        let mut attributes = BTreeMap::from([
            ("specversion".into(), "1.0".into()),
            ("id".into(), self.id.as_str().to_owned()),
            ("type".into(), self.contract.contract.as_str().to_owned()),
            ("subject".into(), self.subject.as_str().to_owned()),
            ("source".into(), "talos://interconnect".into()),
            (
                "taloscontractversion".into(),
                self.contract.version.as_str().to_owned(),
            ),
            (
                "talosschema".into(),
                self.contract.schema.as_str().to_owned(),
            ),
            (
                "correlationid".into(),
                self.correlation_id.as_str().to_owned(),
            ),
        ]);
        if let Some(causation) = &self.causation_id {
            attributes.insert("causationid".into(), causation.as_str().to_owned());
        }
        attributes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportCapability {
    Durable,
    Replayable,
    RequestReply,
    Fanout,
    CompetingConsumers,
    OrderedByKey,
    ConsumerCheckpoint,
    DelayedDelivery,
    DeadLetter,
    TransactionalPublish,
    FlowControl,
    ContentReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverDescriptor {
    pub name: String,
    pub version: ContractVersion,
    pub capabilities: BTreeSet<TransportCapability>,
}

impl DriverDescriptor {
    pub fn validate(&self) -> Result<(), InterconnectError> {
        validate_identifier(&self.name, "driver name", 96)
    }

    pub fn satisfies(&self, requirements: &CallRequirements) -> Result<(), InterconnectError> {
        self.validate()?;
        let missing: Vec<_> = requirements
            .required
            .difference(&self.capabilities)
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(InterconnectError::new(
                InterconnectErrorCode::CapabilityUnavailable,
                format!(
                    "driver {} lacks required capabilities: {missing:?}",
                    self.name
                ),
            ));
        }
        Ok(())
    }
}

/// Strict call requirement set. R4 deliberately has no caller-controlled
/// correctness-downgrade field. Any weaker deployment profile must be selected
/// by trusted Runtime/deployment policy outside the message payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CallRequirements {
    pub required: BTreeSet<TransportCapability>,
}

impl CallRequirements {
    pub fn strict(required: impl IntoIterator<Item = TransportCapability>) -> Self {
        Self {
            required: required.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestTarget {
    pub module: String,
    pub command: String,
}

impl RequestTarget {
    pub fn new(
        module: impl Into<String>,
        command: impl Into<String>,
    ) -> Result<Self, InterconnectError> {
        let target = Self {
            module: module.into(),
            command: command.into(),
        };
        validate_identifier(&target.module, "request module", 96)?;
        validate_identifier(&target.command, "request command", 96)?;
        Ok(target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestMessage {
    pub envelope: MessageEnvelope,
    pub target: RequestTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum RequestResult {
    Ok(Value),
    Err(InterconnectError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionMetadata {
    pub earliest_sequence: u64,
    pub latest_sequence: u64,
    pub retention_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCursor {
    pub consumer: ConsumerId,
    pub subject: Subject,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkClaim {
    pub work_id: MessageId,
    pub claim_generation: u64,
    pub lease_owner: LeaseOwner,
    pub lease_deadline_ms: u64,
    pub attempt: u32,
    pub retry_budget: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateRevision(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WatchCursor(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ResumeDirective {
    Resume {
        cursor: WatchCursor,
    },
    ResyncRequired {
        latest_revision: StateRevision,
        earliest_available_cursor: WatchCursor,
    },
}

/// Immutable executable identity pinned by durable plugin-backed work/effect
/// admission. Location is deliberately absent; package/manifest digests are the
/// identity, and queued work must not silently retarget after an upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginExecutableRef {
    pub plugin_id: PluginId,
    pub version: ContractVersion,
    pub package_digest_sha256: String,
    pub manifest_digest_sha256: String,
    pub capability_contract_version: ContractVersion,
    pub provider_instance_id: Option<ProviderInstanceRef>,
    pub binding_revision: Option<BindingRevisionRef>,
}

impl PluginExecutableRef {
    pub fn validate(&self) -> Result<(), InterconnectError> {
        for (kind, digest) in [
            ("package", &self.package_digest_sha256),
            ("manifest", &self.manifest_digest_sha256),
        ] {
            if digest.len() != 64
                || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                || digest.bytes().any(|byte| byte.is_ascii_uppercase())
            {
                return Err(InterconnectError::new(
                    InterconnectErrorCode::InvalidContentRef,
                    format!("{kind} digest must be lowercase SHA-256 hex"),
                ));
            }
        }
        if self.provider_instance_id.is_some() != self.binding_revision.is_some() {
            return Err(InterconnectError::new(
                InterconnectErrorCode::ContractIncompatible,
                "provider-backed plugin executable identity requires provider instance and binding revision together",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> ContractBinding {
        ContractBinding {
            contract: ContractRef::new("order.query").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            schema: SchemaRef::new("order.query.v1").unwrap(),
        }
    }

    #[test]
    fn subject_is_canonical_exact_and_rejects_wildcards() {
        let subject = Subject::new("  Order.Read_Model  ").unwrap();
        assert_eq!(subject.as_str(), "order.read_model");
        for invalid in [
            "order.*",
            "order.>",
            "order..read",
            "订单.read",
            "order/read",
        ] {
            assert!(Subject::new(invalid).is_err(), "{invalid} must be rejected");
        }
        assert!(Subject::new("talos.security.audit").unwrap().is_reserved());
    }

    #[test]
    fn envelope_has_no_self_declared_authority_extension_escape() {
        let mut extensions = BTreeMap::new();
        extensions.insert("tenant_id".into(), Value::String("tenant-b".into()));
        assert!(Extensions::new(extensions).is_err());

        let encoded = serde_json::to_string(&MessageEnvelope {
            id: MessageId::new("message-1").unwrap(),
            kind: MessageKind::Query,
            subject: Subject::new("order.query").unwrap(),
            contract: contract(),
            correlation_id: CorrelationId::new("corr-1").unwrap(),
            causation_id: None,
            created_at_ms: 10,
            deadline_ms: Some(20),
            ordering_key: None,
            idempotency_key: None,
            payload: PayloadRef::Inline(Value::Null),
            extensions: Extensions::empty(),
        })
        .unwrap();
        for forbidden in ["tenant_id", "principal", "roles", "execution_plane"] {
            assert!(!encoded.contains(forbidden));
        }
    }

    #[test]
    fn large_inline_payload_requires_content_reference() {
        let payload = PayloadRef::Inline(Value::String("x".repeat(MAX_INLINE_PAYLOAD_BYTES + 1)));
        assert!(matches!(
            payload.validate().unwrap_err().code,
            InterconnectErrorCode::ResourceBudgetExceeded
        ));
    }

    #[test]
    fn content_identity_is_digest_bound_not_location_bound() {
        let digest = "ab".repeat(32);
        let a = ContentRef::new(
            DigestAlgorithm::Sha256,
            digest.clone(),
            64,
            "application/octet-stream",
            None,
            None,
            vec!["object://bucket/a".into()],
            AccessPolicyRef::new("tenant.content.read").unwrap(),
        )
        .unwrap();
        let b = ContentRef::new(
            DigestAlgorithm::Sha256,
            digest,
            64,
            "application/octet-stream",
            None,
            None,
            vec!["https://example.invalid/mutable-location".into()],
            AccessPolicyRef::new("tenant.content.read").unwrap(),
        )
        .unwrap();
        assert_eq!(a.stable_identity(), b.stable_identity());
    }

    #[test]
    fn driver_capability_mismatch_fails_closed() {
        let driver = DriverDescriptor {
            name: "in_process".into(),
            version: ContractVersion::new("1.0.0").unwrap(),
            capabilities: BTreeSet::from([TransportCapability::RequestReply]),
        };
        let required = CallRequirements::strict([
            TransportCapability::RequestReply,
            TransportCapability::Durable,
        ]);
        assert!(matches!(
            driver.satisfies(&required).unwrap_err().code,
            InterconnectErrorCode::CapabilityUnavailable
        ));
    }

    #[test]
    fn cloudevents_mapping_preserves_contract_and_correlation() {
        let envelope = MessageEnvelope {
            id: MessageId::new("message-1").unwrap(),
            kind: MessageKind::Event,
            subject: Subject::new("order.closed").unwrap(),
            contract: contract(),
            correlation_id: CorrelationId::new("corr-1").unwrap(),
            causation_id: Some(CausationId::new("cause-1").unwrap()),
            created_at_ms: 10,
            deadline_ms: None,
            ordering_key: Some(OrderingKey::new("order-1").unwrap()),
            idempotency_key: Some(IdempotencyKey::new("event-1").unwrap()),
            payload: PayloadRef::Inline(Value::Null),
            extensions: Extensions::empty(),
        };
        let mapped = envelope.cloudevents_attributes();
        assert_eq!(mapped.get("specversion").map(String::as_str), Some("1.0"));
        assert_eq!(
            mapped.get("correlationid").map(String::as_str),
            Some("corr-1")
        );
        assert_eq!(
            mapped.get("causationid").map(String::as_str),
            Some("cause-1")
        );
    }

    #[test]
    fn plugin_executable_ref_requires_content_bound_digests() {
        let reference = PluginExecutableRef {
            plugin_id: PluginId::new("official.sf-express").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: "ab".repeat(32),
            manifest_digest_sha256: "cd".repeat(32),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            provider_instance_id: Some(ProviderInstanceRef::new("provider-a").unwrap()),
            binding_revision: Some(BindingRevisionRef::new("rev-1").unwrap()),
        };
        assert!(reference.validate().is_ok());

        let mut missing_binding = reference.clone();
        missing_binding.binding_revision = None;
        assert_eq!(
            missing_binding.validate().unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );

        let mut missing_instance = reference.clone();
        missing_instance.provider_instance_id = None;
        assert_eq!(
            missing_instance.validate().unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );

        let mut provider_neutral = reference;
        provider_neutral.provider_instance_id = None;
        provider_neutral.binding_revision = None;
        assert!(provider_neutral.validate().is_ok());
    }
}
