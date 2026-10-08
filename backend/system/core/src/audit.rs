use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ActorIdentity, DataScope, ExecutionContext, ExecutionMode, RequestId, TenantScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditPayloadPolicy {
    Full,
    Masked,
    HashOnly,
    ReferenceOnly,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStatus {
    Clean,
    Stale,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Attempted,
    Succeeded,
    Failed { error_code: String },
}

/// Immutable record of a command attempt. Events are only created and appended;
/// they intentionally expose no update or delete operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    event_id: String,
    actor: ActorIdentity,
    tenant_scope: TenantScope,
    data_scope: DataScope,
    execution_mode: ExecutionMode,
    command: String,
    result: AuditResult,
    occurred_at: String,
    correlation_id: RequestId,
    payload_policy: AuditPayloadPolicy,
    payload: Value,
}

impl AuditEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn from_execution(
        event_id: impl Into<String>,
        ctx: &ExecutionContext,
        command: impl Into<String>,
        result: AuditResult,
        occurred_at: impl Into<String>,
        payload_policy: AuditPayloadPolicy,
        payload: Value,
    ) -> Result<Self, String> {
        let event_id = non_blank(event_id.into(), "audit event id")?;
        let command = non_blank(command.into(), "audit command")?;
        let occurred_at = non_blank(occurred_at.into(), "audit timestamp")?;
        if let AuditResult::Failed { error_code } = &result {
            non_blank(error_code.clone(), "audit failure code")?;
        }

        Ok(Self {
            event_id,
            actor: ctx.actor().clone(),
            tenant_scope: ctx.tenant_scope().clone(),
            data_scope: ctx.data_scope().clone(),
            execution_mode: ctx.execution_mode().clone(),
            command,
            result,
            occurred_at,
            correlation_id: ctx.correlation_id().clone(),
            payload_policy,
            payload,
        })
    }

    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn actor(&self) -> &ActorIdentity {
        &self.actor
    }
    pub fn tenant_scope(&self) -> &TenantScope {
        &self.tenant_scope
    }
    pub fn data_scope(&self) -> &DataScope {
        &self.data_scope
    }
    pub fn execution_mode(&self) -> &ExecutionMode {
        &self.execution_mode
    }
    pub fn command(&self) -> &str {
        &self.command
    }
    pub fn result(&self) -> &AuditResult {
        &self.result
    }
    pub fn occurred_at(&self) -> &str {
        &self.occurred_at
    }
    pub fn correlation_id(&self) -> &RequestId {
        &self.correlation_id
    }
    pub fn payload_policy(&self) -> AuditPayloadPolicy {
        self.payload_policy
    }
    pub fn payload(&self) -> &Value {
        &self.payload
    }
}

fn non_blank(value: String, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
        NoopHttpClient, PlatformMembershipId, PlatformRole, RequestId, Revision, TenantId,
        TenantScope,
    };

    use super::{AuditEvent, AuditPayloadPolicy, AuditResult};

    #[test]
    fn audit_event_keeps_execution_identity_and_payload_policy() {
        let tenant_id = TenantId::new("tenant-acme").unwrap();
        let scope = DataScope::new(
            tenant_id.clone(),
            Namespace::production(),
            Revision::new("rev-42").unwrap(),
        )
        .unwrap();
        let ctx = ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner-1",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership-1").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id),
            scope,
            ExecutionMode::ReadOnlyPreview(crate::PreviewSessionId::new("preview-1").unwrap()),
            RequestId::new("request-123").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();

        let event = AuditEvent::from_execution(
            "event-123",
            &ctx,
            "device.list",
            AuditResult::Succeeded,
            "2026-07-16T12:00:00+08:00",
            AuditPayloadPolicy::Masked,
            json!({ "serial_no": "***1234" }),
        )
        .unwrap();

        assert_eq!(event.actor().id(), Some("platform-owner-1"));
        assert_eq!(
            event.tenant_scope().effective_tenant_id().as_str(),
            "tenant-acme"
        );
        assert_eq!(
            event.execution_mode(),
            &ExecutionMode::ReadOnlyPreview(crate::PreviewSessionId::new("preview-1").unwrap())
        );
        assert_eq!(event.command(), "device.list");
        assert_eq!(event.result(), &AuditResult::Succeeded);
        assert_eq!(event.correlation_id().as_str(), "request-123");
        assert_eq!(event.payload_policy(), AuditPayloadPolicy::Masked);
    }
}
