use system_core::tenant::{DataScope, TenantContext, TenantType};

/// Build the data scope used by every middleware-resolved tenant request.
pub fn scoped_tenant_context(tenant_id: String) -> TenantContext {
    TenantContext {
        tenant_type: TenantType::Customer,
        tenant_id: Some(tenant_id),
        scope: DataScope::Owned,
    }
}
