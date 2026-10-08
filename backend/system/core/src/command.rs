use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessRequirement {
    Authenticated,
    TenantAdmin,
    Platform,
    System,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectClass {
    DatabaseRead,
    DatabaseWrite,
    Notification,
    Payment,
    Logistics,
    Webhook,
    HardwareControl,
    ExternalHttp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationSupport {
    Supported,
    SupportedWithStub,
    Blocked,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CommandMetadata {
    pub name: &'static str,
    pub access: AccessRequirement,
    pub effects: &'static [EffectClass],
    pub simulation: SimulationSupport,
}
impl CommandMetadata {
    pub const fn new(
        name: &'static str,
        access: AccessRequirement,
        effects: &'static [EffectClass],
        simulation: SimulationSupport,
    ) -> Self {
        Self {
            name,
            access,
            effects,
            simulation,
        }
    }
    pub fn writes_database(&self) -> bool {
        self.effects.contains(&EffectClass::DatabaseWrite)
    }
    pub fn has_external_effect(&self) -> bool {
        self.effects.iter().any(|effect| {
            !matches!(
                effect,
                EffectClass::DatabaseRead | EffectClass::DatabaseWrite
            )
        })
    }
}
