pub mod damage;
pub mod depreciation;
pub mod device;
pub mod excel_import;
pub mod model;
pub mod procurement;
pub mod repair;
pub mod roa;

pub use damage::FeatureDamage;
pub use depreciation::FeatureDepreciation;
pub use device::FeatureDevice;
pub use excel_import::FeatureExcelImport;
pub use model::FeatureModel;
pub use procurement::FeatureProcurement;
pub use repair::FeatureRepair;

#[cfg(test)]
pub(crate) fn test_context() -> system_core::ExecutionContext {
    use std::sync::Arc;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    let tenant_id = TenantId::new("test-tenant").unwrap();
    ExecutionContext::new(
        ActorIdentity::system(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("test-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("test-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}
pub use roa::FeatureRoa;
