use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerIdParseError;

impl Display for CustomerIdParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("CustomerId must be a UUID")
    }
}

impl Error for CustomerIdParseError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CustomerId(String);

impl CustomerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, CustomerIdParseError> {
        let value = value.into();
        let parsed = Uuid::parse_str(value.trim()).map_err(|_| CustomerIdParseError)?;
        Ok(Self(parsed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CustomerId {
    fn default() -> Self {
        Self::new()
    }
}

impl<'de> Deserialize<'de> for CustomerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerStatus {
    Active,
    Inactive,
    Anonymized,
}

impl CustomerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Anonymized => "anonymized",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerRiskStatus {
    Clear,
    ReviewRequired,
    Blocked,
}

impl CustomerRiskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::ReviewRequired => "review_required",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactKind {
    Phone,
    Email,
    Other,
}

impl ContactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Email => "email",
            Self::Other => "other",
        }
    }
}

pub fn normalize_contact_value(kind: ContactKind, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("contact value must not be blank".into());
    }

    match kind {
        ContactKind::Phone => {
            let has_plus = trimmed.starts_with('+');
            let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
            if !(7..=15).contains(&digits.len()) {
                return Err("phone contact must contain 7 to 15 digits".into());
            }
            Ok(if has_plus {
                format!("+{digits}")
            } else {
                digits
            })
        }
        ContactKind::Email => {
            let normalized = trimmed.to_ascii_lowercase();
            let mut pieces = normalized.split('@');
            let local = pieces.next().unwrap_or_default();
            let domain = pieces.next().unwrap_or_default();
            if local.is_empty()
                || domain.is_empty()
                || pieces.next().is_some()
                || !domain.contains('.')
            {
                return Err("email contact is invalid".into());
            }
            Ok(normalized)
        }
        ContactKind::Other => Ok(trimmed.to_string()),
    }
}

pub fn mask_contact_value(kind: ContactKind, normalized: &str) -> String {
    match kind {
        ContactKind::Phone => {
            let digits: String = normalized.chars().filter(char::is_ascii_digit).collect();
            if digits.len() <= 4 {
                return "****".into();
            }
            format!("***{}", &digits[digits.len() - 4..])
        }
        ContactKind::Email => {
            let Some((local, domain)) = normalized.split_once('@') else {
                return "***".into();
            };
            let first = local.chars().next().unwrap_or('*');
            format!("{first}***@{domain}")
        }
        ContactKind::Other => "***".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ContactKind, CustomerId, mask_contact_value, normalize_contact_value};

    #[test]
    fn customer_id_is_uuid_only() {
        let generated = CustomerId::new();
        assert!(uuid::Uuid::parse_str(generated.as_str()).is_ok());
        assert!(CustomerId::parse("not-a-uuid").is_err());
    }

    #[test]
    fn contacts_normalize_and_mask_without_exposing_raw_values() {
        assert_eq!(
            normalize_contact_value(ContactKind::Phone, "+86 138-0013-8000").unwrap(),
            "+8613800138000"
        );
        assert_eq!(
            mask_contact_value(ContactKind::Phone, "+8613800138000"),
            "***8000"
        );
        assert_eq!(
            normalize_contact_value(ContactKind::Email, " User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert_eq!(
            mask_contact_value(ContactKind::Email, "user@example.com"),
            "u***@example.com"
        );
    }
}
