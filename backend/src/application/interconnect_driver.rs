//! Stable driver SPI for R4 Interconnect capability negotiation.
//!
//! The SPI is deliberately smaller than any broker SDK. Semantic lane
//! operations remain in the reference drivers; this trait freezes the common
//! capability/identity boundary future adapters must satisfy before a caller
//! can select them.

use system_core::transport::interconnect::{CallRequirements, DriverDescriptor, InterconnectError};

use super::interconnect::InProcessDriver;

pub trait InterconnectDriver: Send + Sync {
    fn descriptor(&self) -> DriverDescriptor;

    fn require(&self, requirements: &CallRequirements) -> Result<(), InterconnectError> {
        self.descriptor().satisfies(requirements)
    }
}

impl InterconnectDriver for InProcessDriver {
    fn descriptor(&self) -> DriverDescriptor {
        InProcessDriver::descriptor(self)
    }
}

#[cfg(feature = "postgres")]
impl InterconnectDriver for super::interconnect::postgres::PostgresDurableDriver {
    fn descriptor(&self) -> DriverDescriptor {
        super::interconnect::postgres::PostgresDurableDriver::descriptor(self)
    }
}

pub fn require_driver_capabilities(
    driver: &dyn InterconnectDriver,
    requirements: &CallRequirements,
) -> Result<DriverDescriptor, InterconnectError> {
    driver.require(requirements)?;
    Ok(driver.descriptor())
}

#[cfg(test)]
mod tests {
    use system_core::transport::interconnect::{CallRequirements, TransportCapability};

    use super::*;

    #[test]
    fn in_process_driver_fails_closed_on_unsupported_durability() {
        let driver = InProcessDriver::default();
        let error = require_driver_capabilities(
            &driver,
            &CallRequirements::strict([TransportCapability::Durable]),
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            system_core::transport::interconnect::InterconnectErrorCode::CapabilityUnavailable
        );
    }

    #[test]
    fn in_process_driver_advertises_only_implemented_requirements() {
        let driver = InProcessDriver::default();
        let descriptor = require_driver_capabilities(
            &driver,
            &CallRequirements::strict([
                TransportCapability::Replayable,
                TransportCapability::ConsumerCheckpoint,
            ]),
        )
        .unwrap();
        assert_eq!(descriptor.name, "in_process");
    }
}
