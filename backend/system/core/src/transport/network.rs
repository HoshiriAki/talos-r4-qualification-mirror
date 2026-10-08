//! Transport-neutral outbound network authority contract.
//!
//! This module contains authority facts only. DNS resolution, sockets, HTTP
//! clients, deployment CIDR exceptions and audit persistence live outside Core.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressProtocol {
    Http,
    Https,
}

impl EgressProtocol {
    pub const fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }

    pub fn from_scheme(scheme: &str) -> Option<Self> {
        match scheme {
            "http" => Some(Self::Http),
            "https" => Some(Self::Https),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressMethodClass {
    Safe,
    Mutation,
}

impl EgressMethodClass {
    pub fn from_http_method(method: &str) -> Option<Self> {
        match method.to_ascii_uppercase().as_str() {
            "GET" | "HEAD" | "OPTIONS" => Some(Self::Safe),
            "POST" | "PUT" | "PATCH" | "DELETE" => Some(Self::Mutation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressRedirectPolicy {
    /// Redirect responses are returned/failed without following another URL.
    Deny,
    /// A future/profile transport may follow a redirect only after the new URL
    /// is revalidated against the same grant and destination policy.
    Revalidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressGrant {
    pub grant_id: String,
    pub destination_host: String,
    pub ports: Vec<u16>,
    pub protocols: Vec<EgressProtocol>,
    pub method_classes: Vec<EgressMethodClass>,
    pub path_prefixes: Vec<String>,
    pub redirect_policy: EgressRedirectPolicy,
    pub max_connect_timeout_ms: u64,
    pub max_read_timeout_ms: u64,
    pub max_overall_timeout_ms: u64,
    pub max_response_bytes: usize,
    pub max_concurrency: u32,
    pub rate_limit_per_minute: u32,
    pub secret_purposes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressGrantError {
    InvalidGrantId,
    InvalidDestinationHost,
    InvalidPort,
    EmptyProtocolSet,
    MultipleProtocolsNotAllowed,
    EmptyMethodClassSet,
    MultipleMethodClassesNotAllowed,
    InvalidPathConstraint,
    InvalidTimeoutBudget,
    InvalidResponseBudget,
    InvalidConcurrencyBudget,
    InvalidRateBudget,
    InvalidSecretPurpose,
}

impl EgressGrant {
    /// Validate serialized/configured authority before it reaches any network
    /// transport. Enforcement points must call this before using the grant.
    pub fn validate(&self) -> Result<(), EgressGrantError> {
        if !valid_identifier(&self.grant_id) {
            return Err(EgressGrantError::InvalidGrantId);
        }
        if !valid_host(&self.destination_host) {
            return Err(EgressGrantError::InvalidDestinationHost);
        }
        if self.ports.is_empty() || self.ports.iter().any(|port| *port == 0) {
            return Err(EgressGrantError::InvalidPort);
        }
        if self.protocols.is_empty() {
            return Err(EgressGrantError::EmptyProtocolSet);
        }
        if self.protocols.len() != 1 {
            return Err(EgressGrantError::MultipleProtocolsNotAllowed);
        }
        if self.method_classes.is_empty() {
            return Err(EgressGrantError::EmptyMethodClassSet);
        }
        if self.method_classes.len() != 1 {
            return Err(EgressGrantError::MultipleMethodClassesNotAllowed);
        }
        if self.path_prefixes.is_empty()
            || self.path_prefixes.iter().any(|prefix| {
                !prefix.starts_with('/')
                    || prefix.contains('?')
                    || prefix.contains('#')
                    || prefix.chars().any(char::is_whitespace)
            })
        {
            return Err(EgressGrantError::InvalidPathConstraint);
        }
        if self.max_connect_timeout_ms == 0
            || self.max_read_timeout_ms == 0
            || self.max_overall_timeout_ms == 0
            || self.max_connect_timeout_ms > self.max_overall_timeout_ms
            || self.max_read_timeout_ms > self.max_overall_timeout_ms
        {
            return Err(EgressGrantError::InvalidTimeoutBudget);
        }
        if self.max_response_bytes == 0 {
            return Err(EgressGrantError::InvalidResponseBudget);
        }
        if self.max_concurrency == 0 {
            return Err(EgressGrantError::InvalidConcurrencyBudget);
        }
        if self.rate_limit_per_minute == 0 {
            return Err(EgressGrantError::InvalidRateBudget);
        }
        if self
            .secret_purposes
            .iter()
            .any(|purpose| !valid_identifier(purpose))
        {
            return Err(EgressGrantError::InvalidSecretPurpose);
        }
        Ok(())
    }

    pub fn allows_protocol(&self, protocol: EgressProtocol) -> bool {
        self.protocols.contains(&protocol)
    }

    pub fn allows_port(&self, port: u16) -> bool {
        self.ports.contains(&port)
    }

    pub fn allows_method_class(&self, method_class: EgressMethodClass) -> bool {
        self.method_classes.contains(&method_class)
    }

    pub fn allows_path(&self, path: &str) -> bool {
        self.path_prefixes.iter().any(|prefix| {
            if prefix == "/" {
                return true;
            }
            path == prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|remainder| remainder.starts_with('/'))
        })
    }

    /// A request that uses no secret passes `None`. If it declares a secret
    /// purpose, the purpose must be explicitly granted.
    pub fn allows_secret_purpose(&self, purpose: Option<&str>) -> bool {
        purpose.is_none_or(|purpose| self.secret_purposes.iter().any(|item| item == purpose))
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
}

/// R4 stable authority uses one unambiguous serialization: canonical DNS host
/// or canonical dotted-decimal IPv4 literal. Direct IPv6 literals are rejected
/// because URL serialization brackets them while `IpAddr` does not; admitting
/// both representations into a String grant would create a second authority
/// normalization rule. DNS names may still resolve to IPv6 and are classified
/// by the governed gateway before connect.
fn valid_host(host: &str) -> bool {
    if host.is_empty()
        || host.len() > 253
        || host != host.to_ascii_lowercase()
        || host.ends_with('.')
        || host.contains('*')
        || host.contains('/')
        || host.contains('@')
        || host.contains(':')
        || host.contains('[')
        || host.contains(']')
        || host.chars().any(char::is_whitespace)
    {
        return false;
    }

    if let Ok(address) = host.parse::<IpAddr>() {
        return matches!(address, IpAddr::V4(v4) if v4.to_string() == host);
    }

    // Numeric dotted spellings that are not the canonical IpAddr rendering
    // must never fall through and acquire DNS-name semantics. URL parsers and
    // resolvers have historically disagreed on octal/short/legacy IPv4 forms.
    if host
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return false;
    }

    host.split('.').all(valid_dns_label)
}

fn valid_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && label
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::{
        EgressGrant, EgressGrantError, EgressMethodClass, EgressProtocol, EgressRedirectPolicy,
    };

    fn grant() -> EgressGrant {
        EgressGrant {
            grant_id: "fixture.egress".into(),
            destination_host: "api.example.test".into(),
            ports: vec![443],
            protocols: vec![EgressProtocol::Https],
            method_classes: vec![EgressMethodClass::Mutation],
            path_prefixes: vec!["/v1".into()],
            redirect_policy: EgressRedirectPolicy::Deny,
            max_connect_timeout_ms: 2_000,
            max_read_timeout_ms: 5_000,
            max_overall_timeout_ms: 10_000,
            max_response_bytes: 1024 * 1024,
            max_concurrency: 4,
            rate_limit_per_minute: 60,
            secret_purposes: vec!["provider.api".into()],
        }
    }

    #[test]
    fn exact_authority_dimensions_are_checked() {
        let grant = grant();
        assert_eq!(grant.validate(), Ok(()));
        assert!(grant.allows_protocol(EgressProtocol::Https));
        assert!(!grant.allows_protocol(EgressProtocol::Http));
        assert!(grant.allows_port(443));
        assert!(!grant.allows_port(8443));
        assert!(grant.allows_method_class(EgressMethodClass::Mutation));
        assert!(!grant.allows_method_class(EgressMethodClass::Safe));
        assert!(grant.allows_path("/v1"));
        assert!(grant.allows_path("/v1/orders"));
        assert!(!grant.allows_path("/v10/orders"));
        assert!(grant.allows_secret_purpose(Some("provider.api")));
        assert!(!grant.allows_secret_purpose(Some("provider.signing")));
        assert!(grant.allows_secret_purpose(None));
    }

    #[test]
    fn multiple_authority_dimensions_fail_closed() {
        let mut candidate = grant();
        candidate.protocols.push(EgressProtocol::Http);
        assert_eq!(
            candidate.validate(),
            Err(EgressGrantError::MultipleProtocolsNotAllowed)
        );

        let mut candidate = grant();
        candidate.method_classes.push(EgressMethodClass::Safe);
        assert_eq!(
            candidate.validate(),
            Err(EgressGrantError::MultipleMethodClassesNotAllowed)
        );
    }

    #[test]
    fn wildcard_host_and_invalid_budgets_fail_closed() {
        let mut candidate = grant();
        candidate.destination_host = "*.example.test".into();
        assert_eq!(
            candidate.validate(),
            Err(EgressGrantError::InvalidDestinationHost)
        );

        let mut candidate = grant();
        candidate.max_overall_timeout_ms = 1_000;
        assert_eq!(
            candidate.validate(),
            Err(EgressGrantError::InvalidTimeoutBudget)
        );

        let mut candidate = grant();
        candidate.path_prefixes.clear();
        assert_eq!(
            candidate.validate(),
            Err(EgressGrantError::InvalidPathConstraint)
        );
    }

    #[test]
    fn host_authority_requires_canonical_dns_or_ipv4_serialization() {
        for invalid in [
            "[::1]",
            "::1",
            "EXAMPLE.COM",
            "example.com.",
            "-bad.example",
            "bad-.example",
            "bad_label.example",
            "127.000.000.001",
            "127.1",
            "2130706433",
        ] {
            let mut candidate = grant();
            candidate.destination_host = invalid.into();
            assert_eq!(
                candidate.validate(),
                Err(EgressGrantError::InvalidDestinationHost),
                "unexpectedly accepted {invalid}"
            );
        }

        for valid in ["api.example.test", "service-1", "127.0.0.1"] {
            let mut candidate = grant();
            candidate.destination_host = valid.into();
            assert_eq!(candidate.validate(), Ok(()), "rejected {valid}");
        }
    }

    #[test]
    fn method_class_rejects_ambient_protocol_methods() {
        assert_eq!(
            EgressMethodClass::from_http_method("GET"),
            Some(EgressMethodClass::Safe)
        );
        assert_eq!(
            EgressMethodClass::from_http_method("POST"),
            Some(EgressMethodClass::Mutation)
        );
        assert_eq!(EgressMethodClass::from_http_method("CONNECT"), None);
        assert_eq!(EgressProtocol::from_scheme("ftp"), None);
    }
}
