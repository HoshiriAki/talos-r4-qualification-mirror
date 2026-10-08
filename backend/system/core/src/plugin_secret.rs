//! R4-P6 language-neutral plugin secret reference semantics.
//!
//! A handle is an identifier, not authority. Host execution still intersects
//! `SecretPurpose` permission and trusted runtime binding before any operation.
//! Raw secret material is deliberately absent from this contract.

use serde::{Deserialize, Deserializer, Serialize};

use super::plugin::PluginContractError;

const MAX_SECRET_TOKEN_BYTES: usize = 192;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PluginSecretHandleRef {
    handle_id: String,
    purpose: String,
    revision: u64,
}

impl PluginSecretHandleRef {
    pub fn new(
        handle_id: impl Into<String>,
        purpose: impl Into<String>,
        revision: u64,
    ) -> Result<Self, PluginContractError> {
        let handle_id = handle_id.into();
        let purpose = purpose.into();
        validate_token(&handle_id)?;
        validate_token(&purpose)?;
        if revision == 0 {
            return Err(PluginContractError::InvalidToken);
        }
        Ok(Self {
            handle_id,
            purpose,
            revision,
        })
    }

    pub fn handle_id(&self) -> &str {
        &self.handle_id
    }

    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

impl<'de> Deserialize<'de> for PluginSecretHandleRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            handle_id: String,
            purpose: String,
            revision: u64,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.handle_id, raw.purpose, raw.revision).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSecretOperationKind {
    Sign,
    Decrypt,
}

fn validate_token(value: &str) -> Result<(), PluginContractError> {
    if value.is_empty()
        || value.len() > MAX_SECRET_TOKEN_BYTES
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(PluginContractError::InvalidToken);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_handle_contains_no_secret_material_and_is_revisioned() {
        let handle = PluginSecretHandleRef::new("kms/provider-a", "payment.sign", 7).unwrap();
        assert_eq!(handle.handle_id(), "kms/provider-a");
        assert_eq!(handle.purpose(), "payment.sign");
        assert_eq!(handle.revision(), 7);
        let encoded = serde_json::to_string(&handle).unwrap();
        assert!(!encoded.contains("key_bytes"));
        assert!(!encoded.contains("secret_material"));
    }

    #[test]
    fn forged_zero_revision_or_unbounded_tokens_fail_closed() {
        assert!(PluginSecretHandleRef::new("kms/a", "purpose", 0).is_err());
        assert!(PluginSecretHandleRef::new("has space", "purpose", 1).is_err());
    }
}
