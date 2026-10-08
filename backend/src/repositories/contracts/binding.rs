use system_core::{
    DataScope, ExecutionContext, ExecutionMode, Namespace, RepositoryScope, RequestId, Revision,
    TenantId,
};

use super::RepositoryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone)]
pub struct RepositoryBinding {
    data_scope: DataScope,
    access: RepositoryAccess,
    correlation_id: RequestId,
}

impl RepositoryBinding {
    pub fn from_execution(ctx: &ExecutionContext) -> Result<Self, RepositoryError> {
        let data_scope = ctx.data_scope();
        if !data_scope.is_resolved() {
            return Err(RepositoryError::ScopeUnresolved);
        }
        if data_scope.is_platform() || ctx.tenant_scope().is_platform() {
            return Err(RepositoryError::TenantScopeRequired);
        }

        let access = match (ctx.execution_mode(), data_scope.namespace()) {
            (ExecutionMode::Normal, Namespace::Production) => RepositoryAccess::ReadWrite,
            (ExecutionMode::ReadOnlyPreview(_), Namespace::Production) => {
                RepositoryAccess::ReadOnly
            }
            (ExecutionMode::Simulation(_), Namespace::Simulation(_)) => {
                return Err(RepositoryError::SimulationUnsupported);
            }
            (_, Namespace::Simulation(_)) => {
                return Err(RepositoryError::SimulationUnsupported);
            }
            _ => {
                return Err(RepositoryError::ContractViolation(
                    "execution mode and repository namespace disagree".into(),
                ));
            }
        };

        Ok(Self {
            data_scope: data_scope.clone(),
            access,
            correlation_id: ctx.correlation_id().clone(),
        })
    }

    pub fn data_scope(&self) -> &DataScope {
        &self.data_scope
    }

    pub fn tenant_id(&self) -> &TenantId {
        self.data_scope.tenant_id()
    }

    pub fn namespace(&self) -> &Namespace {
        self.data_scope.namespace()
    }

    pub fn base_revision(&self) -> &Revision {
        self.data_scope.base_revision()
    }

    pub fn access(&self) -> RepositoryAccess {
        self.access
    }

    pub fn correlation_id(&self) -> &RequestId {
        &self.correlation_id
    }
}

impl RepositoryScope for RepositoryBinding {
    fn data_scope(&self) -> &DataScope {
        &self.data_scope
    }
}
