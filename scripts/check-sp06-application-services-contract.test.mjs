#!/usr/bin/env node

import assert from 'node:assert/strict'
import { checkSp06ApplicationServicesContract } from './check-sp06-application-services-contract.mjs'

const clean = {
  'backend/src/application/module_client.rs': `
use std::sync::Arc;
use serde_json::Value;
use system_core::ExecutionContext;
use crate::registry::ModuleRegistry;
pub trait ModuleClient: Send + Sync {
  fn execute(
    &self,
    module_name: &str,
    command: &str,
    payload: Value,
    ctx: &ExecutionContext,
  ) -> Result<Value, String>;
}
pub struct RegistryModuleClient {
  registry: Arc<ModuleRegistry>,
}
impl ModuleClient for RegistryModuleClient {
  fn execute(
    &self,
    module_name: &str,
    command: &str,
    payload: Value,
    ctx: &ExecutionContext,
  ) -> Result<Value, String> {
    self.registry.execute(module_name, command, payload, ctx)
  }
}`,
  'backend/src/application/services.rs': `
pub struct ApplicationServices {
  clock: Arc<dyn Clock>,
  module_client: Arc<dyn ModuleClient>,
  worker_runner: Arc<dyn WorkerRunner>,
  repository_provider: Arc<dyn RepositoryProvider>,
}
impl ApplicationServices {
  pub fn worker_runner(&self) -> Arc<dyn WorkerRunner> { self.worker_runner.clone() }
  pub fn repository_provider(&self) -> Arc<dyn RepositoryProvider> { self.repository_provider.clone() }
}`,
  'backend/src/state.rs': `
pub struct AppState {
  pub config: AppConfig,
  pub registry: Arc<ModuleRegistry>,
  pub http_client: Arc<dyn HttpClient>,
  pub integration_webhook_ingress: Arc<FixtureWebhookIngress>,
  pub integration_webhook_rate_limiter: Arc<FixtureWebhookRateLimiter>,
  application_services: Arc<ApplicationServices>,
  audit_compatibility_repository: AuditCompatibilityRepository,
  auth_security_repository: AuthSecurityRepository,
  identity_authority_repository: IdentityAuthorityRepository,
  machine_authority_repository: MachineAuthorityRepository,
  platform_membership_repository: PlatformMembershipRepository,
  platform_tenant_repository: PlatformTenantRepository,
  tenant_resolution_repository: TenantResolutionRepository,
  #[cfg(feature = "postgres")]
  pub pg_pool: Option<PgPool>,
}`,
}

assert.deepEqual(
  checkSp06ApplicationServicesContract(clean),
  [],
  '1. exact SP-06 contract must pass',
)

const wrongSignature = structuredClone(clean)
wrongSignature['backend/src/application/module_client.rs'] = wrongSignature['backend/src/application/module_client.rs']
  .replace('fn execute(\n    &self,\n    module_name: &str,\n    command: &str,\n    payload: Value,\n    ctx: &ExecutionContext,\n  ) -> Result<Value, String>;', 'fn execute(&self);')
assert(
  checkSp06ApplicationServicesContract(wrongSignature)
    .some((item) => item.evidence.includes('ModuleClient::execute ABI')),
  '2. shortened ModuleClient ABI must fail',
)

const missingContext = structuredClone(clean)
missingContext['backend/src/application/module_client.rs'] = missingContext['backend/src/application/module_client.rs']
  .replace('    ctx: &ExecutionContext,\n', '')
assert(
  checkSp06ApplicationServicesContract(missingContext)
    .some((item) => item.evidence.includes('ModuleClient::execute ABI')),
  '3. ModuleClient ABI without ExecutionContext must fail',
)

const registryExtraField = structuredClone(clean)
registryExtraField['backend/src/application/module_client.rs'] = registryExtraField['backend/src/application/module_client.rs']
  .replace('  registry: Arc<ModuleRegistry>,', '  registry: Arc<ModuleRegistry>,\n  bypass: bool,')
assert(
  checkSp06ApplicationServicesContract(registryExtraField)
    .some((item) => item.evidence.includes('exactly one private')),
  '4. RegistryModuleClient extra state must fail',
)

const publicFutureService = structuredClone(clean)
publicFutureService['backend/src/application/services.rs'] = publicFutureService['backend/src/application/services.rs']
  .replace('  worker_runner: Arc<dyn WorkerRunner>,', '  worker_runner: Arc<dyn WorkerRunner>,\n  pub scheduler: Arc<dyn Scheduler>,')
assert(
  checkSp06ApplicationServicesContract(publicFutureService)
    .some((item) => item.evidence.includes('exactly four private')),
  '5. public unapproved ApplicationServices field must fail',
)

const privateFutureService = structuredClone(clean)
privateFutureService['backend/src/application/services.rs'] = privateFutureService['backend/src/application/services.rs']
  .replace('  worker_runner: Arc<dyn WorkerRunner>,', '  worker_runner: Arc<dyn WorkerRunner>,\n  integration_runtime: Arc<dyn IntegrationRuntime>,')
assert(
  checkSp06ApplicationServicesContract(privateFutureService)
    .some((item) => item.evidence.includes('exactly four private')),
  '6. private but unapproved ApplicationServices field must fail',
)

const nakedRepositoryProvider = structuredClone(clean)
nakedRepositoryProvider['backend/src/state.rs'] = nakedRepositoryProvider['backend/src/state.rs']
  .replace('  application_services: Arc<ApplicationServices>,', '  application_services: Arc<ApplicationServices>,\n  repository_provider: RepositoryProvider,')
assert(
  checkSp06ApplicationServicesContract(nakedRepositoryProvider)
    .some((item) => item.evidence.includes('field set must remain exactly')),
  '7. naked AppState RepositoryProvider must fail',
)

const nakedWorkerRunner = structuredClone(clean)
nakedWorkerRunner['backend/src/state.rs'] = nakedWorkerRunner['backend/src/state.rs']
  .replace('  application_services: Arc<ApplicationServices>,', '  application_services: Arc<ApplicationServices>,\n  worker_runner: Arc<WorkerRunner>,')
assert(
  checkSp06ApplicationServicesContract(nakedWorkerRunner)
    .some((item) => item.evidence.includes('field set must remain exactly')),
  '8. naked AppState WorkerRunner must fail',
)

const unapprovedStateHandle = structuredClone(clean)
unapprovedStateHandle['backend/src/state.rs'] = unapprovedStateHandle['backend/src/state.rs']
  .replace('  pub integration_webhook_rate_limiter: Arc<FixtureWebhookRateLimiter>,', '  pub integration_webhook_rate_limiter: Arc<FixtureWebhookRateLimiter>,\n  pub integration_worker: Arc<FixtureIntegrationWorker>,')
assert(
  checkSp06ApplicationServicesContract(unapprovedStateHandle)
    .some((item) => item.evidence.includes('field set must remain exactly')),
  '9. unapproved AppState runtime handle must fail',
)

const privateWebhookIngress = structuredClone(clean)
privateWebhookIngress['backend/src/state.rs'] = privateWebhookIngress['backend/src/state.rs']
  .replace('  pub integration_webhook_ingress: Arc<FixtureWebhookIngress>,', '  integration_webhook_ingress: Arc<FixtureWebhookIngress>,')
assert(
  checkSp06ApplicationServicesContract(privateWebhookIngress)
    .some((item) => item.evidence.includes('integration_webhook_ingress must remain a public')),
  '10. private webhook ingress handle must fail',
)

const wrongWebhookRateLimiter = structuredClone(clean)
wrongWebhookRateLimiter['backend/src/state.rs'] = wrongWebhookRateLimiter['backend/src/state.rs']
  .replace('Arc<FixtureWebhookRateLimiter>', 'Arc<dyn RateLimiter>')
assert(
  checkSp06ApplicationServicesContract(wrongWebhookRateLimiter)
    .some((item) => item.evidence.includes('integration_webhook_rate_limiter must remain a public')),
  '11. substituted webhook rate limiter must fail',
)

const missingWorkerRunner = structuredClone(clean)
missingWorkerRunner['backend/src/application/services.rs'] = missingWorkerRunner['backend/src/application/services.rs']
  .replace('  worker_runner: Arc<dyn WorkerRunner>,\n', '')
assert(
  checkSp06ApplicationServicesContract(missingWorkerRunner)
    .some((item) => item.evidence.includes('exactly four private')),
  '12. missing ApplicationServices WorkerRunner must fail',
)

const publicWorkerRunner = structuredClone(clean)
publicWorkerRunner['backend/src/application/services.rs'] = publicWorkerRunner['backend/src/application/services.rs']
  .replace('  worker_runner: Arc<dyn WorkerRunner>,', '  pub worker_runner: Arc<dyn WorkerRunner>,')
assert(
  checkSp06ApplicationServicesContract(publicWorkerRunner)
    .some((item) => item.evidence.includes('exactly four private')),
  '13. public ApplicationServices WorkerRunner must fail',
)

const missingWorkerAccessor = structuredClone(clean)
missingWorkerAccessor['backend/src/application/services.rs'] = missingWorkerAccessor['backend/src/application/services.rs']
  .replace('  pub fn worker_runner(&self) -> Arc<dyn WorkerRunner> { self.worker_runner.clone() }\n', '')
assert(
  checkSp06ApplicationServicesContract(missingWorkerAccessor)
    .some((item) => item.evidence.includes('cloned Arc<dyn WorkerRunner> accessor')),
  '14. missing WorkerRunner accessor must fail',
)

const missingRepositoryProvider = structuredClone(clean)
missingRepositoryProvider['backend/src/application/services.rs'] = missingRepositoryProvider['backend/src/application/services.rs']
  .replace('  repository_provider: Arc<dyn RepositoryProvider>,\n', '')
assert(
  checkSp06ApplicationServicesContract(missingRepositoryProvider)
    .some((item) => item.evidence.includes('exactly four private')),
  '15. missing RepositoryProvider must fail',
)

const publicRepositoryProvider = structuredClone(clean)
publicRepositoryProvider['backend/src/application/services.rs'] = publicRepositoryProvider['backend/src/application/services.rs']
  .replace('  repository_provider: Arc<dyn RepositoryProvider>,', '  pub repository_provider: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(publicRepositoryProvider)
    .some((item) => item.evidence.includes('exactly four private')),
  '16. public RepositoryProvider must fail',
)

const missingRepositoryAccessor = structuredClone(clean)
missingRepositoryAccessor['backend/src/application/services.rs'] = missingRepositoryAccessor['backend/src/application/services.rs']
  .replace('  pub fn repository_provider(&self) -> Arc<dyn RepositoryProvider> { self.repository_provider.clone() }\n', '')
assert(
  checkSp06ApplicationServicesContract(missingRepositoryAccessor)
    .some((item) => item.evidence.includes('cloned Arc<dyn RepositoryProvider> accessor')),
  '17. missing RepositoryProvider accessor must fail',
)

const publicAggregate = structuredClone(clean)
publicAggregate['backend/src/state.rs'] = publicAggregate['backend/src/state.rs']
  .replace('  application_services: Arc<ApplicationServices>,', '  pub application_services: Arc<ApplicationServices>,')
assert(
  checkSp06ApplicationServicesContract(publicAggregate)
    .some((item) => item.evidence.includes('must remain a private')),
  '18. public AppState aggregate must fail',
)

const publicAuditCompatibilityRepository = structuredClone(clean)
publicAuditCompatibilityRepository['backend/src/state.rs'] = publicAuditCompatibilityRepository['backend/src/state.rs']
  .replace('  audit_compatibility_repository: AuditCompatibilityRepository,', '  pub audit_compatibility_repository: AuditCompatibilityRepository,')
assert(
  checkSp06ApplicationServicesContract(publicAuditCompatibilityRepository)
    .some((item) => item.evidence.includes('audit_compatibility_repository must remain a private')),
  '30. public AuditCompatibilityRepository authority handle must fail',
)

const wrongAuditCompatibilityRepository = structuredClone(clean)
wrongAuditCompatibilityRepository['backend/src/state.rs'] = wrongAuditCompatibilityRepository['backend/src/state.rs']
  .replace('  audit_compatibility_repository: AuditCompatibilityRepository,', '  audit_compatibility_repository: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(wrongAuditCompatibilityRepository)
    .some((item) => item.evidence.includes('audit_compatibility_repository must remain a private')),
  '31. substituted AuditCompatibilityRepository authority handle must fail',
)

const publicAuthSecurityRepository = structuredClone(clean)
publicAuthSecurityRepository['backend/src/state.rs'] = publicAuthSecurityRepository['backend/src/state.rs']
  .replace('  auth_security_repository: AuthSecurityRepository,', '  pub auth_security_repository: AuthSecurityRepository,')
assert(
  checkSp06ApplicationServicesContract(publicAuthSecurityRepository)
    .some((item) => item.evidence.includes('auth_security_repository must remain a private')),
  '19. public AuthSecurityRepository authority handle must fail',
)

const wrongIdentityAuthorityRepository = structuredClone(clean)
wrongIdentityAuthorityRepository['backend/src/state.rs'] = wrongIdentityAuthorityRepository['backend/src/state.rs']
  .replace('  identity_authority_repository: IdentityAuthorityRepository,', '  identity_authority_repository: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(wrongIdentityAuthorityRepository)
    .some((item) => item.evidence.includes('identity_authority_repository must remain a private')),
  '20. substituted IdentityAuthorityRepository authority handle must fail',
)

const publicMachineAuthorityRepository = structuredClone(clean)
publicMachineAuthorityRepository['backend/src/state.rs'] = publicMachineAuthorityRepository['backend/src/state.rs']
  .replace('  machine_authority_repository: MachineAuthorityRepository,', '  pub machine_authority_repository: MachineAuthorityRepository,')
assert(
  checkSp06ApplicationServicesContract(publicMachineAuthorityRepository)
    .some((item) => item.evidence.includes('machine_authority_repository must remain a private')),
  '21. public MachineAuthorityRepository authority handle must fail',
)

const wrongMachineAuthorityRepository = structuredClone(clean)
wrongMachineAuthorityRepository['backend/src/state.rs'] = wrongMachineAuthorityRepository['backend/src/state.rs']
  .replace('  machine_authority_repository: MachineAuthorityRepository,', '  machine_authority_repository: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(wrongMachineAuthorityRepository)
    .some((item) => item.evidence.includes('machine_authority_repository must remain a private')),
  '22. substituted MachineAuthorityRepository authority handle must fail',
)

const publicPlatformMembershipRepository = structuredClone(clean)
publicPlatformMembershipRepository['backend/src/state.rs'] = publicPlatformMembershipRepository['backend/src/state.rs']
  .replace('  platform_membership_repository: PlatformMembershipRepository,', '  pub platform_membership_repository: PlatformMembershipRepository,')
assert(
  checkSp06ApplicationServicesContract(publicPlatformMembershipRepository)
    .some((item) => item.evidence.includes('platform_membership_repository must remain a private')),
  '23. public PlatformMembershipRepository authority handle must fail',
)

const wrongPlatformMembershipRepository = structuredClone(clean)
wrongPlatformMembershipRepository['backend/src/state.rs'] = wrongPlatformMembershipRepository['backend/src/state.rs']
  .replace('  platform_membership_repository: PlatformMembershipRepository,', '  platform_membership_repository: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(wrongPlatformMembershipRepository)
    .some((item) => item.evidence.includes('platform_membership_repository must remain a private')),
  '24. substituted PlatformMembershipRepository authority handle must fail',
)

const publicPlatformTenantRepository = structuredClone(clean)
publicPlatformTenantRepository['backend/src/state.rs'] = publicPlatformTenantRepository['backend/src/state.rs']
  .replace('  platform_tenant_repository: PlatformTenantRepository,', '  pub platform_tenant_repository: PlatformTenantRepository,')
assert(
  checkSp06ApplicationServicesContract(publicPlatformTenantRepository)
    .some((item) => item.evidence.includes('platform_tenant_repository must remain a private')),
  '25. public PlatformTenantRepository authority handle must fail',
)

const wrongPlatformTenantRepository = structuredClone(clean)
wrongPlatformTenantRepository['backend/src/state.rs'] = wrongPlatformTenantRepository['backend/src/state.rs']
  .replace('  platform_tenant_repository: PlatformTenantRepository,', '  platform_tenant_repository: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(wrongPlatformTenantRepository)
    .some((item) => item.evidence.includes('platform_tenant_repository must remain a private')),
  '26. substituted PlatformTenantRepository authority handle must fail',
)

const publicTenantResolutionRepository = structuredClone(clean)
publicTenantResolutionRepository['backend/src/state.rs'] = publicTenantResolutionRepository['backend/src/state.rs']
  .replace('  tenant_resolution_repository: TenantResolutionRepository,', '  pub tenant_resolution_repository: TenantResolutionRepository,')
assert(
  checkSp06ApplicationServicesContract(publicTenantResolutionRepository)
    .some((item) => item.evidence.includes('tenant_resolution_repository must remain a private')),
  '27. public TenantResolutionRepository authority handle must fail',
)

const wrongTenantResolutionRepository = structuredClone(clean)
wrongTenantResolutionRepository['backend/src/state.rs'] = wrongTenantResolutionRepository['backend/src/state.rs']
  .replace('  tenant_resolution_repository: TenantResolutionRepository,', '  tenant_resolution_repository: Arc<dyn RepositoryProvider>,')
assert(
  checkSp06ApplicationServicesContract(wrongTenantResolutionRepository)
    .some((item) => item.evidence.includes('tenant_resolution_repository must remain a private')),
  '28. substituted TenantResolutionRepository authority handle must fail',
)

const registryBypass = structuredClone(clean)
registryBypass['backend/src/application/module_client.rs'] = registryBypass['backend/src/application/module_client.rs']
  .replace('self.registry.execute(module_name, command, payload, ctx)', 'self.registry.get(module_name).unwrap().execute(command, payload, ctx)')
assert(
  checkSp06ApplicationServicesContract(registryBypass)
    .some((item) => item.evidence.includes('delegate only')),
  '29. direct module bypass must fail',
)

const rawSqlitePool = structuredClone(clean)
rawSqlitePool['backend/src/state.rs'] = rawSqlitePool['backend/src/state.rs']
  .replace(
    'pub struct AppState {',
    'pub struct AppState {\n  pub pool: Pool<SqliteConnectionManager>,',
  )
assert(
  checkSp06ApplicationServicesContract(rawSqlitePool)
    .some((item) => item.evidence.includes('field set must remain exactly')),
  '32. raw SQLite pool in AppState must fail',
)

console.log('SP-06 ApplicationServices contract checker self-test passed: 32 cases.')
