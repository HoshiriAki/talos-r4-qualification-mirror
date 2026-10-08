#!/usr/bin/env node

import assert from 'node:assert/strict'

import { checkSp03cTrustedTenantResolutionBoundary } from './check-sp03c-trusted-tenant-resolution-boundary.mjs'

const clean = {
  'backend/src/middleware/tenant_extractors.rs': `
pub struct TrustedTenantUser {
    tenant: Tenant,
    user: AuthUserInfo,
    context: ExecutionContext,
}
pub struct TrustedTenantAdmin {
    tenant: Tenant,
    user: AuthUserInfo,
    context: ExecutionContext,
}
fn resolve_trusted_tenant_context(
    tenant: &Tenant,
    user: &AuthUserInfo,
    http_client: Arc<dyn HttpClient>,
) -> Result<ExecutionContext, TrustedTenantResolutionError> {
    if tenant.status != "active" { return Err(TrustedTenantResolutionError::TenantInactive); }
    let authority_tenant = match &user.authority {
        AuthorityContext::Tenant { tenant_id, .. } => tenant_id,
        AuthorityContext::Platform { .. } => return Err(TrustedTenantResolutionError::TenantAuthorityRequired),
    };
    if authority_tenant.as_str() != tenant.id { return Err(TrustedTenantResolutionError::TenantMismatch); }
    let tenant_id = TenantId::new(tenant.id.clone())?;
    let data_scope = DataScope::production(tenant_id.clone(), Revision::new("trusted")?)?;
    ExecutionContext::new(
        ActorIdentity::with_authority(user.id.clone(), user.authority.clone())?,
        TenantScope::tenant(tenant_id),
        data_scope,
        ExecutionMode::Normal,
        RequestId::new(format!("http:{}", uuid::Uuid::new_v4()))?,
        None,
        http_client,
    )
}`,
  'backend/src/routes/notify.rs': `
use crate::middleware::tenant_extractors::{TrustedTenantAdmin, TrustedTenantUser};
async fn a(admin: TrustedTenantAdmin) { registry.execute("notify", "a", payload, admin.context()); }
async fn b(admin: TrustedTenantAdmin) { registry.execute("notify", "b", payload, admin.context()); }
async fn c(admin: TrustedTenantAdmin) { registry.execute("notify", "c", payload, admin.context()); }
async fn d(auth: TrustedTenantUser) { registry.execute("notify", "d", payload, auth.context()); }
async fn e(auth: TrustedTenantUser) { registry.execute("notify", "e", payload, auth.context()); }
async fn f(admin: TrustedTenantAdmin) { registry.execute("notify", "f", payload, admin.context()); }
`,
  'backend/src/application/workers.rs': `
impl WorkerContextFactory {
    pub fn platform_context(&self, job: WorkerJobId) -> Result<ExecutionContext, String> { todo!() }
}
`,
}

function changed(path, transform) {
  const fixture = structuredClone(clean)
  fixture[path] = transform(fixture[path] ?? '')
  return fixture
}

function expectFailure(number, fixture, evidence, changedPaths = [], options = {}) {
  const failures = checkSp03cTrustedTenantResolutionBoundary(fixture, changedPaths, options)
  assert(
    failures.some((item) => item.evidence.includes(evidence)),
    `${number}. expected failure containing: ${evidence}\n${JSON.stringify(failures, null, 2)}`,
  )
}

assert.deepEqual(checkSp03cTrustedTenantResolutionBoundary(clean), [], '1. clean boundary passes')
expectFailure(2, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('context: ExecutionContext', 'pub context: ExecutionContext')), 'privately bind')
expectFailure(3, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('tenant: &Tenant', 'tenant: TenantId')), 'middleware Tenant')
expectFailure(4, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('tenant.status != "active"', 'false')), 'inactive tenants')
expectFailure(5, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('AuthorityContext::Platform { .. }', 'AuthorityContext::Platform { roles, .. }')), 'reject platform authority')
expectFailure(6, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('authority_tenant.as_str() != tenant.id', 'false')), 'must match exactly')
expectFailure(7, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('TenantId::new(tenant.id.clone())', 'TenantId::new("default")')), 'matched active tenant')
expectFailure(8, changed('backend/src/middleware/tenant_extractors.rs', (source) => source.replace('uuid::Uuid::new_v4()', '"fixed"')), 'fresh server-generated')
expectFailure(9, changed('backend/src/routes/notify.rs', (source) => source.replace('tenant_extractors::{TrustedTenantAdmin, TrustedTenantUser}', 'auth::{AdminUser, AuthUser}')), 'trusted tenant extractors')
expectFailure(10, changed('backend/src/routes/notify.rs', (source) => `${source}\nuse crate::registry::make_ctx;`), 'must not reconstruct')
expectFailure(11, changed('backend/src/routes/notify.rs', (source) => source.replace('async fn f(admin: TrustedTenantAdmin)', 'async fn f(admin: AdminUser)')), 'all six notification handlers')
expectFailure(12, changed('backend/src/routes/notify.rs', (source) => source.replace('admin.context()); }\nasync fn b', '&ctx); }\nasync fn b')), 'all notification Registry calls')
expectFailure(13, changed('backend/src/application/workers.rs', (source) => `${source}\npub fn tenant_context(&self, tenant_id: TenantId, job: WorkerJobId) {}`), 'must not trust a caller-supplied TenantId')
expectFailure(14, clean, 'exceed the authorized', ['backend/src/routes/orders.rs'], { enforceSliceScope: true })
expectFailure(15, clean, 'changed-path discovery failed closed', ['GIT_CHANGED_PATH_DISCOVERY_FAILED:boom'])
assert.deepEqual(
  checkSp03cTrustedTenantResolutionBoundary(clean, [
    '.github/workflows/repository-quality.yml',
    '.github/workflows/ui-lab-export.yml',
    'README.md',
    'scripts/check-repository-layout.mjs',
    'documentation-link-check.log',
    'repository-layout-check.log',
    'talos-ops-boundary-check.log',
  ]),
  [],
  '16. post-merge CI and documentation work is not blocked by historical slice scope',
)

console.log('SP-03C trusted tenant resolution boundary checker self-test passed: 16 cases.')
