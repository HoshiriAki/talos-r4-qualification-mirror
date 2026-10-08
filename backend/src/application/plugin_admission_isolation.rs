//! R4-P6 admission-visible technical isolation facts.
//!
//! `PluginExecutionAdmission::isolation()` is the normative TALOS capability
//! policy attached to the persistence- and ExecutionContext-bound public token.
//! It is not proof that the operating system prevents a trusted native module
//! from issuing a syscall. Callers that need a technical security claim must
//! inspect this enforcement matrix instead of inferring sandboxing from policy
//! booleans.

use system_core::security::plugin_isolation::{
    PluginIsolationEnforcementMatrix, r4_plugin_isolation_enforcement,
};

use super::plugin_execution_admission::PluginExecutionAdmission;

impl PluginExecutionAdmission {
    /// Return the actual R4 technical isolation classification for this admitted
    /// executable. `FirstPartyNative` truthfully reports trusted ambient process
    /// authority; disabled profiles report the sandbox/remote boundary that must
    /// exist before they can ever be enabled.
    pub fn technical_isolation_enforcement(&self) -> PluginIsolationEnforcementMatrix {
        r4_plugin_isolation_enforcement(self.isolation().profile)
    }
}
