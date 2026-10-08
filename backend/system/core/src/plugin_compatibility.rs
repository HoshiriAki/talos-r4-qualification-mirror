//! R4-P6 host/plugin compatibility semantics.
//!
//! The universal plugin contract does not depend on a Rust/Wasm runtime or a
//! third-party semver crate. R4 freezes a small comparator grammar sufficient
//! for host-contract admission and fails closed on syntax it does not
//! understand. The implementation can be replaced later without changing the
//! semantic contract.

use std::cmp::Ordering;

use super::plugin::{CompatibilityRange, PluginContractError};
use crate::transport::interconnect::ContractVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl SemanticVersion {
    fn parse(value: &str) -> Result<Self, PluginContractError> {
        // R4 host compatibility deliberately accepts only the stable numeric
        // core. Pre-release/build labels can exist as ContractVersion tokens,
        // but cannot silently participate in compatibility ordering until a
        // later contract explicitly freezes those semantics.
        if value.is_empty()
            || value
                .bytes()
                .any(|byte| matches!(byte, b'-' | b'+' | b'_') || byte.is_ascii_whitespace())
        {
            return Err(PluginContractError::InvalidCompatibilityRange);
        }
        let mut parts = value.split('.');
        let major = parse_component(parts.next())?;
        let minor = parse_component(parts.next())?;
        let patch = parse_component(parts.next())?;
        if parts.next().is_some() {
            return Err(PluginContractError::InvalidCompatibilityRange);
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

fn parse_component(value: Option<&str>) -> Result<u64, PluginContractError> {
    let value = value.ok_or(PluginContractError::InvalidCompatibilityRange)?;
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(PluginContractError::InvalidCompatibilityRange);
    }
    value
        .parse::<u64>()
        .map_err(|_| PluginContractError::InvalidCompatibilityRange)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparator {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl Comparator {
    fn matches(self, actual: SemanticVersion, expected: SemanticVersion) -> bool {
        let ordering = actual.cmp(&expected);
        match self {
            Self::Eq => ordering == Ordering::Equal,
            Self::Gt => ordering == Ordering::Greater,
            Self::Gte => matches!(ordering, Ordering::Greater | Ordering::Equal),
            Self::Lt => ordering == Ordering::Less,
            Self::Lte => matches!(ordering, Ordering::Less | Ordering::Equal),
        }
    }
}

/// Returns whether a trusted host-contract version is accepted by a package's
/// declared compatibility range.
///
/// Grammar frozen for R4:
///
/// ```text
/// range      := comparator ("," comparator)*
/// comparator := (">=" | "<=" | ">" | "<" | "=")? version
/// version    := DIGITS "." DIGITS "." DIGITS
/// ```
///
/// Whitespace, wildcard/caret/tilde ranges and pre-release ordering are
/// rejected rather than guessed.
pub fn host_contract_is_compatible(
    range: &CompatibilityRange,
    host_contract_version: &ContractVersion,
) -> Result<bool, PluginContractError> {
    let actual = SemanticVersion::parse(host_contract_version.as_str())?;
    let raw = range.as_str();
    if raw.is_empty() {
        return Err(PluginContractError::InvalidCompatibilityRange);
    }

    let mut comparator_count = 0usize;
    for clause in raw.split(',') {
        comparator_count += 1;
        if comparator_count > 8 || clause.is_empty() {
            return Err(PluginContractError::InvalidCompatibilityRange);
        }
        let (operator, version) = if let Some(value) = clause.strip_prefix(">=") {
            (Comparator::Gte, value)
        } else if let Some(value) = clause.strip_prefix("<=") {
            (Comparator::Lte, value)
        } else if let Some(value) = clause.strip_prefix('>') {
            (Comparator::Gt, value)
        } else if let Some(value) = clause.strip_prefix('<') {
            (Comparator::Lt, value)
        } else if let Some(value) = clause.strip_prefix('=') {
            (Comparator::Eq, value)
        } else {
            (Comparator::Eq, clause)
        };
        let expected = SemanticVersion::parse(version)?;
        if !operator.matches(actual, expected) {
            return Ok(false);
        }
    }
    Ok(comparator_count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(value: &str) -> CompatibilityRange {
        CompatibilityRange::new(value).unwrap()
    }

    fn version(value: &str) -> ContractVersion {
        ContractVersion::new(value).unwrap()
    }

    #[test]
    fn bounded_comparator_range_matches_expected_host_versions() {
        let supported = range(">=1.0.0,<2.0.0");
        assert_eq!(
            host_contract_is_compatible(&supported, &version("1.0.0")),
            Ok(true)
        );
        assert_eq!(
            host_contract_is_compatible(&supported, &version("1.9.99")),
            Ok(true)
        );
        assert_eq!(
            host_contract_is_compatible(&supported, &version("0.9.9")),
            Ok(false)
        );
        assert_eq!(
            host_contract_is_compatible(&supported, &version("2.0.0")),
            Ok(false)
        );
    }

    #[test]
    fn exact_and_closed_ranges_are_supported() {
        assert_eq!(
            host_contract_is_compatible(&range("1.2.3"), &version("1.2.3")),
            Ok(true)
        );
        assert_eq!(
            host_contract_is_compatible(&range(">=1.2.3,<=1.2.3"), &version("1.2.3")),
            Ok(true)
        );
    }

    #[test]
    fn unknown_range_or_version_semantics_fail_closed() {
        for invalid in ["^1.2.3", "~1.2.3", "1.2.*", ">=1.0", ">=01.0.0"] {
            let compatibility = CompatibilityRange::new(invalid).unwrap();
            assert!(host_contract_is_compatible(&compatibility, &version("1.2.3")).is_err());
        }
        for invalid_version in ["1.2.3-alpha", "1.2.3+build", "1.2"] {
            assert!(
                host_contract_is_compatible(&range(">=1.0.0,<2.0.0"), &version(invalid_version))
                    .is_err()
            );
        }
    }
}
