#[path = "../src/registry/tenant_context.rs"]
mod tenant_context;

#[test]
fn tenant_context_contains_the_authenticated_tenant_id() {
    let ctx = tenant_context::scoped_tenant_context("tenant-a".to_string());

    assert_eq!(ctx.tenant_id.as_deref(), Some("tenant-a"));
    assert_eq!(ctx.scope, system_core::tenant::DataScope::Owned);
}
