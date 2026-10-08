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

            pub fn parse(value: impl Into<String>) -> Result<Self, ReservationIdParseError> {
                let value = value.into();
                let parsed = Uuid::parse_str(value.trim()).map_err(|_| ReservationIdParseError)?;
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
pub struct ReservationIdParseError;

impl Display for ReservationIdParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("reservation/allocation identifier must be a UUID")
    }
}

impl Error for ReservationIdParseError {}

uuid_id!(ReservationId);
uuid_id!(ReservationRequirementId);
uuid_id!(AllocationId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationStatus {
    Hold,
    Confirmed,
    Released,
    Expired,
}

impl ReservationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Confirmed => "confirmed",
            Self::Released => "released",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationStatus {
    Allocated,
    Released,
}

impl AllocationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allocated => "allocated",
            Self::Released => "released",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AllocationId, ReservationId};

    #[test]
    fn reservation_and_allocation_ids_are_uuid_backed() {
        let reservation = ReservationId::new();
        let allocation = AllocationId::new();
        assert!(ReservationId::parse(reservation.as_str()).is_ok());
        assert!(AllocationId::parse(allocation.as_str()).is_ok());
        assert!(ReservationId::parse("legacy-1").is_err());
    }
}
