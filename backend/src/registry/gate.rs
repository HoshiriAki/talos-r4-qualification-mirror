use system_core::{
    AccessRequirement, AuthorityContext, CommandMetadata, ExecutionContext, ExecutionMode,
    PlatformCapability, SimulationSupport, TenantRole,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    PreviewWriteBlocked,
    PreviewEffectBlocked,
    SimulationUnsupported,
    AccessDenied,
}

pub fn authorize_platform_capability(
    ctx: &ExecutionContext,
    metadata: &CommandMetadata,
    module: &str,
    command: &str,
) -> Result<(), GateError> {
    if module == "tenant_preview"
        && matches!(command, "session.get" | "session.end" | "session.resolve")
    {
        let can_read_any_workspace = [
            PlatformCapability::TenantPreviewRead,
            PlatformCapability::TenantDiagnosticsRead,
            PlatformCapability::TenantSimulationRead,
        ]
        .into_iter()
        .any(|capability| ctx.actor().has_platform_capability(capability));
        return can_read_any_workspace
            .then_some(())
            .ok_or(GateError::AccessDenied);
    }
    let required = match (module, command) {
        ("tenant_governance", "change_intent.record") => {
            Some(PlatformCapability::TenantGovernanceManage)
        }
        ("tenant_governance", command) if command.starts_with("audit.") => {
            Some(PlatformCapability::AuditRead)
        }
        ("tenant_governance", _) => Some(PlatformCapability::TenantGovernanceRead),
        ("tenant_preview", "session.create") => Some(PlatformCapability::TenantPreviewCreate),
        ("tenant_preview", "workspace.create_preview") => {
            Some(PlatformCapability::TenantPreviewCreate)
        }
        ("tenant_preview", "workspace.create_diagnostics") => {
            Some(PlatformCapability::TenantDiagnosticsRead)
        }
        ("tenant_preview", "workspace.create_simulation") => {
            Some(PlatformCapability::TenantSimulationRead)
        }
        ("tenant_preview", _) => Some(PlatformCapability::TenantPreviewRead),
        ("tenant_simulation", "session.create") => Some(PlatformCapability::TenantSimulationCreate),
        ("tenant_simulation", "session.discard") => {
            Some(PlatformCapability::TenantSimulationDiscard)
        }
        ("tenant_simulation", _) => Some(PlatformCapability::TenantSimulationRead),
        ("integration", "register_manifest") => Some(PlatformCapability::PlatformOperationsManage),
        _ => None,
    };
    match (metadata.access, required) {
        (AccessRequirement::Platform, None) => Err(GateError::AccessDenied),
        (_, Some(capability)) if !ctx.actor().has_platform_capability(capability) => {
            Err(GateError::AccessDenied)
        }
        _ => Ok(()),
    }
}

impl GateError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::PreviewWriteBlocked => "EXEC_PREVIEW_WRITE_BLOCKED",
            Self::PreviewEffectBlocked => "EXEC_PREVIEW_EFFECT_BLOCKED",
            Self::SimulationUnsupported => "EXEC_SIMULATION_UNSUPPORTED",
            Self::AccessDenied => "EXEC_ACCESS_DENIED",
        }
    }
}

pub fn authorize_access(
    ctx: &ExecutionContext,
    metadata: &CommandMetadata,
) -> Result<(), GateError> {
    let allowed = match metadata.access {
        AccessRequirement::Authenticated => matches!(
            ctx.authority(),
            Some(AuthorityContext::Tenant { .. } | AuthorityContext::Platform { .. })
        ),
        AccessRequirement::TenantAdmin => matches!(
            ctx.authority(),
            Some(AuthorityContext::Tenant {
                role: TenantRole::Admin | TenantRole::Owner,
                ..
            })
        ),
        AccessRequirement::Platform => {
            matches!(ctx.authority(), Some(AuthorityContext::Platform { .. }))
        }
        AccessRequirement::System => ctx.user_id().is_none() && ctx.authority().is_none(),
    };
    if allowed {
        Ok(())
    } else {
        Err(GateError::AccessDenied)
    }
}

pub fn authorize_execution_mode(
    mode: &ExecutionMode,
    metadata: &CommandMetadata,
) -> Result<(), GateError> {
    authorize_execution_mode_for(mode, metadata, None)
}

/// Fail-closed execution-mode gate with the exact MVP3 simulation capability
/// registry. Metadata support is necessary but never sufficient: only the
/// reference-resource commands backed by the simulation module are admitted.
pub fn authorize_execution_mode_for(
    mode: &ExecutionMode,
    metadata: &CommandMetadata,
    command: Option<(&str, &str)>,
) -> Result<(), GateError> {
    match mode {
        ExecutionMode::Normal => Ok(()),
        ExecutionMode::ReadOnlyPreview(_) if metadata.writes_database() => {
            Err(GateError::PreviewWriteBlocked)
        }
        ExecutionMode::ReadOnlyPreview(_) if metadata.has_external_effect() => {
            Err(GateError::PreviewEffectBlocked)
        }
        ExecutionMode::ReadOnlyPreview(_) => Ok(()),
        ExecutionMode::Simulation(_) => {
            let exact_adapter = matches!(
                command,
                Some((
                    "tenant_simulation",
                    "reference_config.list"
                        | "reference_config.get"
                        | "reference_config.put"
                        | "reference_config.delete"
                        | "reference_config.record_effect"
                        | "pricing_config.get"
                        | "pricing_config.put"
                        | "pricing_config.estimate"
                        | "diff.evaluate"
                        | "diff.get"
                        | "effects.list"
                ))
            );
            if !exact_adapter || metadata.simulation == SimulationSupport::Blocked {
                return Err(GateError::SimulationUnsupported);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use system_core::{
        AccessRequirement, ActorIdentity, AuthorityContext, CommandMetadata, DataScope,
        EffectClass, ExecutionContext, ExecutionMode, NoopHttpClient, PlatformMembershipId,
        PlatformRole, RequestId, Revision, SimulationId, SimulationSupport, TenantId, TenantScope,
    };

    use super::{
        GateError, authorize_access, authorize_execution_mode, authorize_execution_mode_for,
        authorize_platform_capability,
    };

    const READ: CommandMetadata = CommandMetadata::new(
        "read",
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseRead],
        SimulationSupport::Supported,
    );
    const WRITE: CommandMetadata = CommandMetadata::new(
        "write",
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseWrite],
        SimulationSupport::Supported,
    );
    const PAYMENT: CommandMetadata = CommandMetadata::new(
        "pay",
        AccessRequirement::Authenticated,
        &[EffectClass::Payment],
        SimulationSupport::Blocked,
    );

    #[test]
    fn preview_rejects_writes_before_handlers_run() {
        assert_eq!(
            authorize_execution_mode(
                &ExecutionMode::ReadOnlyPreview(
                    system_core::PreviewSessionId::new("preview-1").unwrap()
                ),
                &WRITE
            ),
            Err(GateError::PreviewWriteBlocked)
        );
        assert_eq!(
            authorize_execution_mode(
                &ExecutionMode::ReadOnlyPreview(
                    system_core::PreviewSessionId::new("preview-1").unwrap()
                ),
                &READ
            ),
            Ok(())
        );
    }

    #[test]
    fn simulation_rejects_blocked_external_effects() {
        let simulation = ExecutionMode::Simulation(SimulationId::new("sim-1").unwrap());
        assert_eq!(
            authorize_execution_mode(&simulation, &PAYMENT),
            Err(GateError::SimulationUnsupported)
        );
        assert_eq!(
            authorize_execution_mode(&simulation, &READ),
            Err(GateError::SimulationUnsupported)
        );
    }

    #[test]
    fn simulation_requires_exact_registered_adapter() {
        let simulation = ExecutionMode::Simulation(SimulationId::new("sim-1").unwrap());
        assert_eq!(
            authorize_execution_mode_for(
                &simulation,
                &WRITE,
                Some(("tenant_simulation", "reference_config.put")),
            ),
            Ok(())
        );
        assert_eq!(
            authorize_execution_mode_for(
                &simulation,
                &WRITE,
                Some(("pricing", "reference_config.put")),
            ),
            Err(GateError::SimulationUnsupported)
        );
    }

    #[test]
    fn access_requirement_is_enforced_before_dispatch() {
        let tenant = TenantId::new("tenant-a").unwrap();
        let staff = ExecutionContext::new(
            ActorIdentity::authenticated("staff-1", "staff").unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("req-1").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();
        let admin_command = CommandMetadata::new(
            "admin",
            AccessRequirement::TenantAdmin,
            &[EffectClass::DatabaseWrite],
            SimulationSupport::Supported,
        );
        assert_eq!(
            authorize_access(&staff, &admin_command),
            Err(GateError::AccessDenied)
        );
    }

    #[test]
    fn registry_boundary_rejects_platform_role_without_command_capability() {
        let actor = ActorIdentity::with_authority(
            "auditor-1",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-a").unwrap(),
                roles: vec![PlatformRole::SecurityAuditor],
            },
        )
        .unwrap();
        let ctx = ExecutionContext::new(
            actor,
            TenantScope::platform(),
            DataScope::platform(Revision::new("control-current").unwrap()),
            ExecutionMode::Normal,
            RequestId::new("request-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();
        assert_eq!(
            authorize_platform_capability(
                &ctx,
                &CommandMetadata::new(
                    "write",
                    AccessRequirement::Platform,
                    &[EffectClass::DatabaseWrite],
                    SimulationSupport::Blocked,
                ),
                "tenant_governance",
                "change_intent.record"
            ),
            Err(GateError::AccessDenied)
        );
        assert_eq!(
            authorize_platform_capability(
                &ctx,
                &CommandMetadata::new(
                    "read",
                    AccessRequirement::Platform,
                    &[EffectClass::DatabaseRead],
                    SimulationSupport::Blocked,
                ),
                "tenant_governance",
                "audit.query"
            ),
            Ok(())
        );
        assert_eq!(
            authorize_platform_capability(
                &ctx,
                &CommandMetadata::new(
                    "unknown",
                    AccessRequirement::Platform,
                    &[EffectClass::DatabaseRead],
                    SimulationSupport::Blocked,
                ),
                "future_platform_module",
                "unknown"
            ),
            Err(GateError::AccessDenied)
        );
    }
}
