#!/usr/bin/env node

import assert from 'node:assert/strict'
import { checkSp07ModuleFactoryWorkerBoundary } from './check-sp07-module-factory-worker-boundary.mjs'

const clean = {
  'backend/src/registry/descriptors.rs': `
pub enum ModuleClass { Core }
pub enum ModuleActivation { Always }
pub enum ModuleRequirement { SqlitePool }
pub enum ModuleFactoryId { Audit }
pub struct ModuleDescriptor {
  pub registry_key: &'static str,
  pub metadata_name: &'static str,
  pub schema_name: &'static str,
  pub class: ModuleClass,
  pub activation: ModuleActivation,
  pub requirements: &'static [ModuleRequirement],
  pub factory: ModuleFactoryId,
}
pub const MODULE_DESCRIPTORS: &[ModuleDescriptor] = &[
  ModuleDescriptor { registry_key: "audit", metadata_name: "feature-audit", schema_name: "feature-audit", class: ModuleClass::Core, activation: ModuleActivation::Always, requirements: &[], factory: ModuleFactoryId::Audit },
];`,
  'backend/src/registry/factory.rs': `
pub struct ConstructionReceipt {
  pub factory: ModuleFactoryId,
  pub registry_key: &'static str,
}
trait ConstructionIdentity {
  const FACTORY: ModuleFactoryId;
  const REGISTRY_KEY: &'static str;
}
fn constructed<T: ConstructionIdentity>(module: T) -> ConstructedModule {
  ConstructedModule {
    receipt: ConstructionReceipt { factory: T::FACTORY, registry_key: T::REGISTRY_KEY },
    module,
  }
}
fn build() {
  let mut construction_receipts = Vec::new();
  let mut insert = |constructed: ConstructedModule| {
    construction_receipts.push(constructed.receipt);
    modules.insert(constructed.receipt.registry_key.to_string(), constructed.module);
  };
  insert(constructed(audit));
  validate_descriptor_projection(&descriptors, &modules, &build_order, &construction_receipts, &self.provider_config)?;
}`,
  'backend/src/registry/provider_config.rs': `
#[derive(Clone, Default)]
pub struct ProviderDeploymentConfig;
impl ProviderDeploymentConfig {
  pub fn is_configured(&self, _provider: ProviderId) -> bool { false }
}`,
  'backend/src/registry/validation.rs': `
fn validate_descriptor_projection() {
  if descriptor.factory != receipt.factory { return Err("factory"); }
  if module_keys != active { return Err("modules"); }
  if build_order_keys != active { return Err("order"); }
  if receipt_keys != active { return Err("receipts"); }
  if metadata_name != descriptor.metadata_name { return Err("metadata"); }
  if schema_name != descriptor.schema_name { return Err("schema"); }
}`,
  'backend/src/registry/assembler.rs': `
use super::factory::ModuleFactory;
fn assemble(pool: Pool, http_client: Client) {
  let provider_config = ProviderDeploymentConfig::default();
  let built = ModuleFactory::new(pool, http_client, provider_config).build()?;
  ModuleRegistry::new(built.into_modules())
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
}`,
  'backend/src/application/workers.rs': `
pub trait WorkerRunner: Send + Sync { fn run_startup(&self); }
pub struct WorkerContextFactory;
impl WorkerContextFactory { fn make(&self) { ExecutionContext::new(actor, tenant, data, mode, request, None, http); } }`,
  'backend/src/state.rs': `
pub struct AppState {
  application_services: Arc<ApplicationServices>,
}`,
  'backend/src/main.rs': `
fn main() {
  worker_runner.run_startup();
  let _maintenance_worker = worker_runner.spawn_periodic();
  tokio::spawn(async move { unrelated_observer().await; });
}`,
}

function changed(path, transform) {
  const fixture = structuredClone(clean)
  fixture[path] = transform(fixture[path])
  return fixture
}

function added(path, source) {
  const fixture = structuredClone(clean)
  fixture[path] = source
  return fixture
}

function expectFailure(number, fixture, evidence) {
  assert(
    checkSp07ModuleFactoryWorkerBoundary(fixture).some((item) => item.evidence.includes(evidence)),
    `${number}. expected failure containing: ${evidence}`,
  )
}

assert.deepEqual(checkSp07ModuleFactoryWorkerBoundary(clean), [], '1. clean SP-07 boundary must pass')

const missing = structuredClone(clean)
delete missing['backend/src/registry/factory.rs']
expectFailure(2, missing, 'required SP-07 boundary source is missing')

expectFailure(3, changed('backend/src/registry/descriptors.rs', (source) => source.replace(
  "  pub registry_key: &'static str,",
  "  pub name: &'static str,",
)), 'explicit registry_key')
expectFailure(4, changed('backend/src/registry/descriptors.rs', (source) => source.replace(
  "  pub factory: ModuleFactoryId,",
  "  pub factory: ModuleFactoryId,\n  pub decorative: bool,",
)), 'explicit registry_key')
expectFailure(5, changed('backend/src/registry/descriptors.rs', (source) => `${source}\npub struct Legacy { name: &'static str }`), 'single ambiguous descriptor name')

expectFailure(6, changed('backend/src/registry/assembler.rs', (source) => `use official_audit::FeatureAudit;\n${source}`), 'concrete Feature imports')
expectFailure(7, changed('backend/src/registry/assembler.rs', (source) => `${source}\nfn env_read() { std::env::var("SF_CLIENT_ID"); }`), 'assembler must not read')
expectFailure(8, changed('backend/src/registry/assembler.rs', (source) => `${source}\nconst SF_CLIENT_SECRET: &str = "secret";`), 'secret names')
expectFailure(9, changed('backend/src/registry/assembler.rs', (source) => `${source}\nfn mutate() { order.device = device; }`), 'dependency-field mutation')
expectFailure(10, changed('backend/src/registry/assembler.rs', (source) => `${source}\nfn insert() { modules.insert("audit", audit); }`), 'per-module map insertion')
expectFailure(11, changed('backend/src/registry/assembler.rs', (source) => source.replace('ModuleFactory::new', 'LegacyAssembler::new')), 'delegate module construction')
expectFailure(12, changed('backend/src/registry/factory.rs', (source) => `${source}\nfn forbidden() { std::env::var("SF_CLIENT_ID"); }`), 'Integration KeyStore boundary')
expectFailure(13, changed('backend/src/registry/provider_config.rs', (source) => source.replace('false', 'true')), 'legacy provider modules must remain disabled')

expectFailure(14, changed('backend/src/registry/factory.rs', (source) => source.replace('pub struct ConstructionReceipt', 'pub struct DecorativeReceipt')), 'construction receipts')
expectFailure(15, changed('backend/src/registry/factory.rs', (source) => source.replace('construction_receipts.push', 'decorative_receipts.push')), 'construction receipts')
expectFailure(16, changed('backend/src/registry/factory.rs', (source) => source.replace('&construction_receipts', '&[]')), 'construction receipts')
expectFailure(17, changed('backend/src/registry/factory.rs', (source) => source.replace('factory: T::FACTORY', 'factory: ModuleFactoryId::Order')), 'concrete-type-bound construction receipts')
expectFailure(18, changed('backend/src/registry/validation.rs', (source) => source.replace('descriptor.factory != receipt.factory', 'false')), 'receipt-to-descriptor')
expectFailure(19, changed('backend/src/registry/validation.rs', (source) => source.replace('receipt_keys != active', 'receipt_keys.is_empty()')), 'receipt-to-descriptor')
expectFailure(20, changed('backend/src/registry/validation.rs', (source) => source.replace('module_keys != active', 'module_keys.is_empty()')), 'identity closure')
expectFailure(21, changed('backend/src/registry/validation.rs', (source) => source.replace('build_order_keys != active', 'build_order_keys.is_empty()')), 'identity closure')
expectFailure(22, changed('backend/src/registry/validation.rs', (source) => source.replace('metadata_name != descriptor.metadata_name', 'metadata_name.is_empty()')), 'identity closure')
expectFailure(23, changed('backend/src/registry/validation.rs', (source) => source.replace('schema_name != descriptor.schema_name', 'schema_name.is_empty()')), 'identity closure')
expectFailure(24, changed('backend/src/registry/validation.rs', (source) => `${source}\nfn forbidden_identity_collapse(descriptor: &ModuleDescriptor) { assert!(descriptor.registry_key != descriptor.metadata_name); }`), 'must not be forced equal')

expectFailure(25, changed('backend/src/application/services.rs', (source) => source.replace('  worker_runner: Arc<dyn WorkerRunner>,\n', '')), 'exactly four private')
expectFailure(26, changed('backend/src/application/services.rs', (source) => source.replace('  worker_runner:', '  pub worker_runner:')), 'exactly four private')
expectFailure(27, changed('backend/src/application/services.rs', (source) => source.replace('  worker_runner: Arc<dyn WorkerRunner>,', '  worker_runner: Arc<dyn WorkerRunner>,\n  scheduler: Arc<dyn Scheduler>,')), 'exactly four private')
expectFailure(28, changed('backend/src/application/services.rs', (source) => source.replace('  pub fn worker_runner', '  fn worker_runner')), 'expose the WorkerRunner')
expectFailure(29, changed('backend/src/state.rs', (source) => source.replace('  application_services:', '  worker_runner: Arc<dyn WorkerRunner>,\n  application_services:')), 'naked WorkerRunner')

expectFailure(30, changed('backend/src/main.rs', (source) => `${source}\nfn direct_seed() { seed_overdue_tasks(&pool); }`), 'legacy maintenance functions directly')
expectFailure(31, changed('backend/src/main.rs', (source) => `${source}\nfn direct_sync() { sync_order_statuses(&pool); }`), 'legacy maintenance functions directly')
expectFailure(32, changed('backend/src/main.rs', (source) => `${source}\nfn direct_cleanup() { cleanup_expired_sessions(&pool); }`), 'legacy maintenance functions directly')
expectFailure(33, changed('backend/src/main.rs', (source) => `${source}\nfn legacy_loop() { let mut interval = tokio::time::interval(duration); loop { interval.tick(); } }`), 'periodic maintenance loop')
expectFailure(34, changed('backend/src/main.rs', (source) => source.replace('  worker_runner.run_startup();\n', '')), 'start maintenance only')
expectFailure(35, changed('backend/src/main.rs', (source) => source.replace('  let _maintenance_worker = worker_runner.spawn_periodic();\n', '')), 'start maintenance only')
expectFailure(36, changed('backend/src/application/workers.rs', (source) => source.replace('WorkerRunner: Send + Sync', 'WorkerRunner')), 'typed WorkerRunner')
expectFailure(37, changed('backend/src/application/services.rs', (source) => `${source}\ntype Locator = HashMap<String, Arc<dyn Any>>;`), 'generic service-locator')
expectFailure(38, changed('backend/src/registry/factory.rs', (source) => `${source}\nfn locate(value: &dyn Any) { value.downcast_ref::<Factory>(); }`), 'generic service-locator')
expectFailure(39, changed('backend/src/application/workers.rs', (source) => `${source}\nfn mutate_dependency() { order.device = device; }`), 'dependency-field mutation')
expectFailure(40, changed('backend/src/application/services.rs', (source) => `${source}\nfn lock_pool() { self.pool.lock(); }`), 'direct pool lock')
expectFailure(41, changed('backend/src/main.rs', (source) => `${source}\nfn assign_pool() { *pool_guard = Some(pool.clone()); }`), 'direct pool lock')
expectFailure(42, changed('backend/src/application/workers.rs', (source) => `${source}\nfn forbidden_env() { std::env::var("SF_CLIENT_ID"); }`), 'Integration KeyStore boundary')

assert.deepEqual(
  checkSp07ModuleFactoryWorkerBoundary(changed('backend/src/registry/factory.rs', (source) => `${source}\nfn permitted_factory_wiring() { module.pool = pool; *pool_guard = Some(pool.clone()); }`)),
  [],
  '43. direct dependency and pool wiring inside ModuleFactory must pass',
)
assert.deepEqual(
  checkSp07ModuleFactoryWorkerBoundary(changed('backend/src/application/workers.rs', (source) => `${source}\npub struct UnrelatedWorker { queue_name: String }`)),
  [],
  '44. unrelated typed worker code must pass',
)

const historical = structuredClone(clean)
historical['docs/historical/sp-07-notes.md'] = 'FeatureAudit std::env::var("SECRET") modules.insert direct worker loop'
assert.deepEqual(checkSp07ModuleFactoryWorkerBoundary(historical), [], '45. historical documentation must remain allowed')

const negativeFixtureText = structuredClone(clean)
negativeFixtureText['backend/src/registry/assembler.rs'] += `
#[cfg(test)]
mod tests {
  fn negative_fixture() {
    let _braces = r###"} /* still a string */"###;
    /* nested comment { /* inner } */ remains test-only */
    modules.insert("bad", feature);
    std::env::var("CLIENT_SECRET");
  }
}`
assert.deepEqual(checkSp07ModuleFactoryWorkerBoundary(negativeFixtureText), [], '46. cfg(test) negative fixtures must remain allowed')

const unequalIdentity = structuredClone(clean)
assert.match(unequalIdentity['backend/src/registry/descriptors.rs'], /registry_key: "audit", metadata_name: "feature-audit"/)
assert.deepEqual(checkSp07ModuleFactoryWorkerBoundary(unequalIdentity), [], '47. explicit unequal registry and runtime identities must pass')

const runtimeAfterTestModule = structuredClone(clean)
runtimeAfterTestModule['backend/src/registry/assembler.rs'] += `
#[cfg(test)]
mod tests {
  fn negative_fixture() { modules.insert("test-only", feature); }
}
fn prohibited_runtime_after_tests() { std::env::var("SF_CLIENT_ID"); }`
expectFailure(48, runtimeAfterTestModule, 'Integration KeyStore boundary')

assert.deepEqual(
  checkSp07ModuleFactoryWorkerBoundary(added('backend/src/registry/unrelated.rs', 'pub struct RegistryNote;')),
  [],
  '49. unrelated Registry Rust files must pass',
)
assert.deepEqual(
  checkSp07ModuleFactoryWorkerBoundary(added('backend/src/application/unrelated.rs', 'pub struct ApplicationNote;')),
  [],
  '50. unrelated Application Rust files must pass',
)

expectFailure(51, added(
  'backend/src/registry/provider_bootstrap.rs',
  'fn load() { let _ = std::env::var("WECHAT_PAY_PRIVATE_KEY"); }',
), 'Integration KeyStore boundary')
expectFailure(52, added(
  'backend/src/registry/legacy_wiring.rs',
  'fn wire(module: &Module, pool: Pool) { module.pool = pool; }',
), 'only inside ModuleFactory')
expectFailure(53, added(
  'backend/src/registry/alternate_factory.rs',
  'use official_device::FeatureDevice; fn build() {}',
), 'concrete Feature imports are allowed only')
expectFailure(54, added(
  'backend/src/application/service_locator.rs',
  'type Locator = HashMap<String, Arc<dyn Any>>;',
), 'generic service-locator')
expectFailure(55, changed(
  'backend/src/registry/provider_config.rs',
  (source) => `${source}\nfn forbidden_env() { std::env::var("SF_CLIENT_ID"); }`,
), 'must not read provider environment')

console.log('SP-07 module factory and worker boundary checker self-test passed: 55 cases.')
