use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            pub fn parse(value: impl Into<String>) -> Result<Self, IdParseError> {
                let value = value.into();
                let parsed = Uuid::parse_str(value.trim()).map_err(|_| IdParseError)?;
                Ok(Self(parsed.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdParseError;

impl Display for IdParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier must be a UUID")
    }
}

impl Error for IdParseError {}

uuid_id!(QuoteId);
uuid_id!(QuoteLineId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Money {
    pub minor: i64,
    pub currency: String,
}

impl Money {
    pub fn new(minor: i64, currency: impl Into<String>) -> Result<Self, String> {
        if minor < 0 {
            return Err("money minor units must not be negative".into());
        }
        let currency = currency.into().trim().to_ascii_uppercase();
        if currency.len() != 3 || !currency.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return Err("currency must be a 3-letter ISO-style code".into());
        }
        Ok(Self { minor, currency })
    }

    pub fn from_major_f64(value: f64, currency: &str) -> Result<Self, String> {
        if !value.is_finite() || value < 0.0 {
            return Err("server price must be a finite non-negative amount".into());
        }
        let scaled = value * 100.0;
        if scaled > i64::MAX as f64 {
            return Err("server price exceeds Money range".into());
        }
        Self::new(scaled.round() as i64, currency)
    }

    pub fn checked_mul(&self, quantity: u32) -> Result<Self, String> {
        let minor = self
            .minor
            .checked_mul(i64::from(quantity))
            .ok_or_else(|| "money multiplication overflow".to_string())?;
        Self::new(minor, self.currency.clone())
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, String> {
        if self.currency != other.currency {
            return Err("cannot add Money values with different currencies".into());
        }
        let minor = self
            .minor
            .checked_add(other.minor)
            .ok_or_else(|| "money addition overflow".to_string())?;
        Self::new(minor, self.currency.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteStatus {
    Draft,
    Confirmed,
    Expired,
    Cancelled,
    Converted,
}

impl QuoteStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Confirmed => "confirmed",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Converted => "converted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteLineKind {
    Model,
    Accessory,
}

impl QuoteLineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Accessory => "accessory",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Money;

    #[test]
    fn money_uses_minor_units_and_checked_arithmetic() {
        let unit = Money::from_major_f64(12.345, "cny").unwrap();
        assert_eq!(unit.minor, 1235);
        assert_eq!(unit.currency, "CNY");
        assert_eq!(unit.checked_mul(3).unwrap().minor, 3705);
        assert!(Money::new(-1, "CNY").is_err());
        assert!(unit.checked_add(&Money::new(1, "USD").unwrap()).is_err());
    }
}
