//! R4-P6 permission-aware host operations over the already-frozen P3 lanes.
//!
//! This module does not create another request/event/work state machine. It
//! binds an immutable, context-bound `PluginExecutionAdmission` to the exact
//! semantic identity already consumed by Interconnect/Registry, then delegates
//! to the existing lane implementation. ExternalEffect remains fail-closed in
//! R4 until durable ExternalOperation persists the exact plugin executable pin.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use system_core::ExecutionContext;
use system_core::security::plugin::PluginPermission;
use system_core::transport::interconnect::{
    ConsumerId, InterconnectError, InterconnectErrorCode, MessageEnvelope, MessageId,
    RequestMessage, RequestResult, Subject,
};

#[cfg(feature = "postgres")]
use super::interconnect::postgres::PostgresDurableDriver;
use super::interconnect::{CancellationSignal, EventRecord, InProcessDriver, InterconnectFabric};
#[cfg(feature = "postgres")]
use super::interconnect_plugin::enqueue_postgres_plugin_work;
use super::interconnect_plugin::{InProcessPluginWorkQueue, PluginWorkAdmission};
use super::plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionRuntimeBinding};

#[derive(Clone)]
enum PluginDurableLane {
    Development {
        events: InProcessDriver,
        work: InProcessPluginWorkQueue,
    },
    #[cfg(feature = "postgres")]
    Postgres(PostgresDurableDriver),
}

/// Host-owned operation surface handed to an admitted plugin runtime. The
/// runtime cannot select a weaker durability profile per call, and admissions
/// minted by another runtime instance are rejected before touching any lane.
pub struct PluginHostOperations {
    fabric: Arc<InterconnectFabric>,
    durable: PluginDurableLane,
    runtime_binding: PluginExecutionRuntimeBinding,
}

impl PluginHostOperations {
    pub(crate) fn development(
        fabric: Arc<InterconnectFabric>,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        let events = fabric.in_process().clone();
        let work = InProcessPluginWorkQueue::new(events.clone());
        Self {
            fabric,
            durable: PluginDurableLane::Development { events, work },
            runtime_binding,
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(
        fabric: Arc<InterconnectFabric>,
        durable: PostgresDurableDriver,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            fabric,
            durable: PluginDurableLane::Postgres(durable),
            runtime_binding,
        }
    }

    /// Capability invoke is bound to the exact Registry target identity. The
    /// human-readable `module.command` permission form is safe only because the
    /// plugin boundary requires both components to be canonical single Subject
    /// segments; the general P3 RequestTarget remains unchanged.
    pub fn invoke(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        request: &RequestMessage,
        now_ms: u64,
        cancellation: &CancellationSignal,
    ) -> RequestResult {
        let permission = match capability_invoke_permission(request) {
            Ok(permission) => permission,
            Err(error) => return RequestResult::Err(error),
        };
        if let Err(error) =
            require_permission(context, admission, &self.runtime_binding, permission)
        {
            return RequestResult::Err(error);
        }
        self.fabric
            .dispatch_request(request, context, now_ms, cancellation)
    }

    pub async fn emit_event(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        envelope: MessageEnvelope,
    ) -> Result<u64, InterconnectError> {
        require_permission(
            context,
            admission,
            &self.runtime_binding,
            PluginPermission::EventEmit(envelope.subject.clone()),
        )?;
        match &self.durable {
            PluginDurableLane::Development { events, .. } => events.append_event(context, envelope),
            #[cfg(feature = "postgres")]
            PluginDurableLane::Postgres(driver) => driver.append_event(context, envelope).await,
        }
    }

    /// Consumer identity is host-derived from the exact installation/executable
    /// pin, including provider binding when present. A plugin cannot submit a
    /// different consumer id and overwrite another cursor.
    pub async fn replay_events(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        subject: Subject,
        after_sequence: u64,
        limit: usize,
        now_ms: u64,
    ) -> Result<Vec<EventRecord>, InterconnectError> {
        require_permission(
            context,
            admission,
            &self.runtime_binding,
            PluginPermission::EventSubscribe(subject.clone()),
        )?;
        let consumer = plugin_consumer_id(admission)?;
        match &self.durable {
            PluginDurableLane::Development { events, .. } => {
                #[cfg(not(feature = "postgres"))]
                let _ = now_ms;
                events.replay_events(context, consumer, subject, after_sequence, limit)
            }
            #[cfg(feature = "postgres")]
            PluginDurableLane::Postgres(driver) => {
                driver
                    .replay_events(context, consumer, subject, after_sequence, limit, now_ms)
                    .await
            }
        }
    }

    /// The plugin supplies only Work envelope/scheduling semantics. The exact
    /// executable pin always comes from the admitted token, preventing a plugin
    /// from queueing work for another package/version.
    pub async fn enqueue_background_job(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        envelope: MessageEnvelope,
        retry_budget: u32,
        available_at_ms: u64,
    ) -> Result<MessageId, InterconnectError> {
        require_permission(
            context,
            admission,
            &self.runtime_binding,
            PluginPermission::BackgroundJob(envelope.subject.as_str().to_owned()),
        )?;
        let work = PluginWorkAdmission {
            envelope,
            executable: admission.executable().clone(),
            retry_budget,
            available_at_ms,
        };
        match &self.durable {
            PluginDurableLane::Development { work: queue, .. } => queue.enqueue(context, work),
            #[cfg(feature = "postgres")]
            PluginDurableLane::Postgres(driver) => {
                enqueue_postgres_plugin_work(driver, context, work).await
            }
        }
    }
}

fn require_permission(
    context: &ExecutionContext,
    admission: &PluginExecutionAdmission,
    runtime_binding: &PluginExecutionRuntimeBinding,
    permission: PluginPermission,
) -> Result<(), InterconnectError> {
    admission
        .require_runtime(runtime_binding)
        .map_err(|_| policy_denied("plugin execution admission belongs to a different runtime"))?;
    admission
        .authorize(context, &permission)
        .map_err(|_| policy_denied("plugin execution admission does not authorize this operation"))
}

fn capability_invoke_permission(
    request: &RequestMessage,
) -> Result<PluginPermission, InterconnectError> {
    let module = canonical_capability_segment(&request.target.module, "module")?;
    let command = canonical_capability_segment(&request.target.command, "command")?;
    let capability = format!("{module}.{command}");
    Ok(PluginPermission::CapabilityInvoke(capability))
}

fn canonical_capability_segment<'a>(
    value: &'a str,
    kind: &'static str,
) -> Result<&'a str, InterconnectError> {
    // Without this restriction `a.b` + `c` and `a` + `b.c` would collapse to
    // the same `CapabilityInvoke("a.b.c")` permission while Registry targets
    // remain distinct. Require one canonical Subject segment to keep the
    // human-readable mapping injective.
    if value.contains('.') {
        return Err(InterconnectError::new(
            InterconnectErrorCode::ContractIncompatible,
            format!("plugin capability {kind} must be one canonical subject segment"),
        ));
    }
    let canonical = Subject::new(value).map_err(|_| {
        InterconnectError::new(
            InterconnectErrorCode::ContractIncompatible,
            format!("plugin capability {kind} is not a valid subject segment"),
        )
    })?;
    if canonical.as_str() != value {
        return Err(InterconnectError::new(
            InterconnectErrorCode::ContractIncompatible,
            format!("plugin capability {kind} must already be canonical lowercase"),
        ));
    }
    Ok(value)
}

fn plugin_consumer_id(
    admission: &PluginExecutionAdmission,
) -> Result<ConsumerId, InterconnectError> {
    let executable = admission.executable();
    let mut digest = Sha256::new();
    for component in [
        admission.tenant_id().as_str(),
        admission.installation_id(),
        executable.plugin_id.as_str(),
        executable.version.as_str(),
        executable.package_digest_sha256.as_str(),
        executable.manifest_digest_sha256.as_str(),
        executable.capability_contract_version.as_str(),
    ] {
        hash_component(&mut digest, component);
    }
    hash_optional_component(
        &mut digest,
        executable
            .provider_instance_id
            .as_ref()
            .map(|value| value.as_str()),
    );
    hash_optional_component(
        &mut digest,
        executable
            .binding_revision
            .as_ref()
            .map(|value| value.as_str()),
    );
    ConsumerId::new(format!("plugin-sub:{:x}", digest.finalize()))
}

fn hash_component(digest: &mut Sha256, component: &str) {
    digest.update([1]);
    digest.update((component.len() as u64).to_be_bytes());
    digest.update(component.as_bytes());
}

fn hash_optional_component(digest: &mut Sha256, component: Option<&str>) {
    match component {
        Some(value) => hash_component(digest, value),
        None => digest.update([0]),
    }
}

fn policy_denied(message: &'static str) -> InterconnectError {
    InterconnectError::new(InterconnectErrorCode::PolicyDenied, message)
}

#[cfg(test)]
mod tests {
    use super::canonical_capability_segment;

    #[test]
    fn plugin_capability_mapping_rejects_delimiter_aliases_and_noncanonical_case() {
        assert_eq!(
            canonical_capability_segment("orders", "module").unwrap(),
            "orders"
        );
        assert!(canonical_capability_segment("orders.admin", "module").is_err());
        assert!(canonical_capability_segment("Orders", "module").is_err());
        assert!(canonical_capability_segment("order/admin", "module").is_err());
    }
}
