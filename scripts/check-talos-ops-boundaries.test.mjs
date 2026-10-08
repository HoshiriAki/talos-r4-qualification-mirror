#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  CORE_PRIMITIVE_DISPOSITION,
  checkApplicationServicesBoundary,
  checkBoundedStaleDeadSuppressions,
  checkCoreExperimentalPrimitiveDisposition,
  checkRemovedLegacyExecutionContext,
  checkRemovedLegacySfPaths,
  checkRemovedUnmountedReservationAdapterPath,
  collectFindings,
  compareFindingSets,
} from './check-talos-ops-boundaries.mjs'

function failures(baselineFiles, currentFiles) {
  return compareFindingSets(collectFindings(baselineFiles), collectFindings(currentFiles))
}

function rules(items) {
  return new Set(items.map((item) => item.rule))
}

const unchangedBaseline = {
  'backend/src/routes/orders.rs': 'fn route() { sqlx::query("UPDATE orders SET status = ? WHERE id = ?"); }',
}
assert.deepEqual(failures(unchangedBaseline, unchangedBaseline), [], 'unchanged legacy debt must remain permitted')

const lineEndingEquivalent = failures(
  {
    'backend/src/routes/orders.rs': 'fn route() {\n  sqlx::query("UPDATE orders SET status = ? WHERE id = ?");\n}\n',
  },
  {
    'backend/src/routes/orders.rs': 'fn route() {\r\n  sqlx::query("UPDATE orders SET status = ? WHERE id = ?");\r\n}\r\n',
  },
)
assert.deepEqual(lineEndingEquivalent, [], 'LF and CRLF forms of the same debt must share one fingerprint')

const equalCountReplacement = failures(
  { 'backend/src/routes/orders.rs': 'fn route() { sqlx::query("UPDATE orders SET status = ? WHERE id = ?"); }' },
  { 'backend/src/routes/orders.rs': 'fn route() { sqlx::query("DELETE FROM orders WHERE id = ?"); }' },
)
assert(rules(equalCountReplacement).has('TALOS-OPS-001'), 'equal-count replacement in the same file must fail')

const multilineInsertOrReplace = failures(
  {},
  {
    'backend/src/routes/import.rs': `fn write() {
      sqlx::query(
        "INSERT OR REPLACE
         INTO orders (id, status)
         VALUES (?, ?)"
      );
    }`,
  },
)
assert(rules(multilineInsertOrReplace).has('TALOS-OPS-001'), 'multiline INSERT OR REPLACE must be detected')

const unregisteredWriteService = failures(
  { 'backend/src/registry/assembler.rs': 'pub fn assemble() {}' },
  {
    'backend/src/registry/assembler.rs': 'pub fn assemble() {}',
    'backend/src/services/new_checkout.rs': 'pub async fn save() { sqlx::query("UPDATE orders SET status = ?"); }',
  },
)
assert(rules(unregisteredWriteService).has('TALOS-OPS-009'), 'new unregistered Domain Write Service must fail')

const nonCoreWriteService = failures(
  { 'backend/src/registry/assembler.rs': 'pub fn assemble() {}' },
  {
    'backend/src/registry/assembler.rs': 'pub fn assemble() {}',
    'backend/src/services/preferences.rs': 'pub async fn save() { sqlx::query("INSERT INTO user_settings (user_id) VALUES (?)"); }',
  },
)
assert(rules(nonCoreWriteService).has('TALOS-OPS-009'), 'unregistered writes to non-core tables must also fail')

const movedDebt = failures(
  { 'backend/src/routes/legacy.rs': 'fn write() { sqlx::query("UPDATE orders SET status = ?"); }' },
  { 'backend/src/routes/replacement.rs': 'fn write() { sqlx::query("UPDATE orders SET status = ?"); }' },
)
assert(rules(movedDebt).has('TALOS-OPS-001'), 'removing old debt must not create reusable baseline allowance elsewhere')

const renamedOrderCompatibilityDebt = failures(
  { 'backend/src/services/order_service.rs': 'fn sync() { sqlx::query("UPDATE orders SET status = ?"); }' },
  { 'backend/src/services/order_compatibility_support.rs': 'fn sync() { sqlx::query("UPDATE orders SET status = ?"); }' },
)
assert.deepEqual(renamedOrderCompatibilityDebt, [], 'the approved order compatibility service rename preserves its exact baseline allowance')

const expandedOrderCompatibilityDebt = failures(
  { 'backend/src/services/order_service.rs': 'fn sync() { sqlx::query("UPDATE orders SET status = ?"); }' },
  { 'backend/src/services/order_compatibility_support.rs': 'fn sync() { sqlx::query("UPDATE orders SET status = ?"); sqlx::query("DELETE FROM orders"); }' },
)
assert(rules(expandedOrderCompatibilityDebt).has('TALOS-OPS-009'), 'new compatibility service writes must not be hidden by the rename allowance')

const removedDebt = failures(
  { 'backend/src/routes/legacy.rs': 'fn write() { sqlx::query("UPDATE orders SET status = ?"); }' },
  { 'backend/src/routes/legacy.rs': 'fn read() { sqlx::query("SELECT * FROM orders"); }' },
)
assert.deepEqual(removedDebt, [], 'removing existing debt must pass')

assert.deepEqual(checkRemovedLegacySfPaths({}), [], 'both deleted legacy SF paths must remain absent')

assert.equal(
  checkRemovedLegacySfPaths({ 'backend/src/routes/sf_express.rs': 'legacy route' })[0]?.rule,
  'TALOS-OPS-011',
  'recreating the deleted legacy SF route must fail',
)

assert.equal(
  checkRemovedLegacySfPaths({ 'backend/src/services/sf_express.rs': 'legacy service' })[0]?.rule,
  'TALOS-OPS-011',
  'recreating the deleted legacy SF service must fail',
)

assert.deepEqual(
  checkRemovedLegacySfPaths({
    'backend/thirdparty/sf-express/src/lib.rs': 'active connector',
    'backend/src/routes/webhooks.rs': 'sf_express webhook adapter',
  }),
  [],
  'active third-party SF connector and webhook paths must remain allowed',
)

assert.deepEqual(
  checkRemovedLegacyExecutionContext({ 'backend/system/core/src/lib.rs': 'pub struct ExecutionContext;' }),
  [],
  'modern source without the legacy identifier must pass',
)

assert.equal(
  checkRemovedLegacyExecutionContext({ 'backend/system/core/src/lib.rs': 'pub struct LegacyExecutionContext;' })[0]?.rule,
  'TALOS-OPS-012',
  'reintroducing the legacy Rust type must fail',
)

assert.equal(
  checkRemovedLegacyExecutionContext({ 'backend/official/order/src/lib.rs': 'use system_core::LegacyExecutionContext;' })[0]?.rule,
  'TALOS-OPS-012',
  'reintroducing a legacy Rust import must fail',
)

assert.equal(
  checkRemovedLegacyExecutionContext({ 'backend/system/core/tests/legacy.rs': 'let _ = LegacyExecutionContext;' })[0]?.rule,
  'TALOS-OPS-012',
  'reintroducing the identifier in compiled test source must fail',
)

assert.equal(
  checkRemovedLegacyExecutionContext({ 'module-kit/MODULE_TEMPLATE.md': 'pub struct LegacyExecutionContext;' })[0]?.rule,
  'TALOS-OPS-012',
  'an active module template emitting the identifier must fail',
)

assert.deepEqual(
  checkRemovedLegacyExecutionContext({ 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-02-legacy-execution-context-removal.md': 'LegacyExecutionContext' }),
  [],
  'the current SP-02 report may retain historical evidence',
)

assert.deepEqual(
  checkRemovedLegacyExecutionContext({ 'policy/qualification/legacy-evidence/docs/historical/legacy.md': 'LegacyExecutionContext' }),
  [],
  'historical Markdown may retain historical evidence',
)

assert.deepEqual(
  checkRemovedLegacyExecutionContext({ 'backend/system/core/src/lib.rs': 'NotLegacyExecutionContextReplacement' }),
  [],
  'longer unrelated identifiers must not match the exact identifier rule',
)

assert.deepEqual(
  checkRemovedLegacyExecutionContext({ 'backend/system/core/src/lib.rs': 'ExecutionContext::new()' }),
  [],
  'modern ExecutionContext usage must pass',
)

assert.deepEqual(
  checkRemovedLegacyExecutionContext({ 'backend/system/core/src/lib.rs': 'Arc::new(NoopHttpClient)' }),
  [],
  'NoopHttpClient usage must pass',
)

assert.deepEqual(checkRemovedUnmountedReservationAdapterPath({}), [], 'removed Reservation adapter path must remain absent')
assert.equal(
  checkRemovedUnmountedReservationAdapterPath({ 'backend/src/routes/reservation.rs': 'legacy adapter' })[0]?.rule,
  'TALOS-OPS-013',
  'recreating the exact legacy Reservation adapter path must fail',
)
assert.deepEqual(
  checkRemovedUnmountedReservationAdapterPath({ 'backend/system/admin/src/reservation.rs': 'FeatureReservation' }),
  [],
  'the active Reservation module must remain allowed',
)
assert.deepEqual(
  checkRemovedUnmountedReservationAdapterPath({ 'backend/src/routes/booking.rs': 'booking_routes' }), [], 'Booking remains allowed')
assert.deepEqual(
  checkRemovedUnmountedReservationAdapterPath({ 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03-unmounted-reservation-route-removal.md': 'reservation.rs' }),
  [],
  'current SP-03 Markdown evidence remains allowed',
)
assert.deepEqual(checkRemovedUnmountedReservationAdapterPath({ 'policy/qualification/legacy-evidence/docs/historical/legacy.md': 'reservation.rs' }), [], 'historical Markdown remains allowed')
assert.deepEqual(
  checkRemovedUnmountedReservationAdapterPath({ 'backend/src/routes/reservation_v2.rs': 'future V2' }),
  [],
  'a separately approved future V2 path remains allowed',
)
assert.deepEqual(checkRemovedLegacySfPaths({}), [], 'TALOS-OPS-011 behavior remains unchanged')
assert.deepEqual(checkRemovedLegacyExecutionContext({}), [], 'TALOS-OPS-012 behavior remains unchanged')

assert.deepEqual(
  checkBoundedStaleDeadSuppressions({ 'backend/src/services/order_service.rs': 'pub fn active() {}' }),
  [],
  'clean order service source must pass TALOS-OPS-014',
)
assert.equal(
  checkBoundedStaleDeadSuppressions({ 'backend/src/services/order_service.rs': '#![allow(dead_code)]' })[0]?.rule,
  'TALOS-OPS-014',
  'recreating the file-level dead_code suppression must fail',
)
assert.equal(
  checkBoundedStaleDeadSuppressions({ 'backend/src/services/order_service.rs': '#![allow(unused)]' })[0]?.rule,
  'TALOS-OPS-014',
  'replacing the file-level suppression with allow(unused) must fail',
)
assert.deepEqual(
  checkBoundedStaleDeadSuppressions({ 'backend/src/services/device_service.rs': '#![allow(dead_code)]' }),
  [],
  'unrelated service suppressions remain outside TALOS-OPS-014',
)
assert.equal(
  checkBoundedStaleDeadSuppressions({ 'backend/src/registry/mod.rs': '集中管理 14 个 SystemModule' })[0]?.rule,
  'TALOS-OPS-014',
  'the stale Registry module count must fail',
)
assert.deepEqual(
  checkBoundedStaleDeadSuppressions({ 'backend/src/registry/mod.rs': '集中管理已装配 SystemModule' }),
  [],
  'count-independent Registry wording must pass',
)
assert.equal(
  checkBoundedStaleDeadSuppressions({
    'backend/official/order/src/state_machine.rs': '#[allow(clippy::if_same_then_else)]',
  })[0]?.rule,
  'TALOS-OPS-014',
  'the named state-machine allowance must fail',
)
assert.deepEqual(
  checkBoundedStaleDeadSuppressions({
    'backend/official/order/src/state_machine.rs': 'pub fn initial_status(_: &str, _: &str) -> &str { "draft" }',
  }),
  [],
  'a simplified or deleted initial_status must pass',
)
assert.deepEqual(
  checkBoundedStaleDeadSuppressions({
    'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/evidence.md': '#![allow(dead_code)] 集中管理 14 个 SystemModule #[allow(clippy::if_same_then_else)]',
  }),
  [],
  'Markdown evidence must remain outside TALOS-OPS-014',
)
assert.deepEqual(
  [
    ...checkRemovedLegacySfPaths({}),
    ...checkRemovedLegacyExecutionContext({}),
    ...checkRemovedUnmountedReservationAdapterPath({}),
  ],
  [],
  'TALOS-OPS-011, TALOS-OPS-012, and TALOS-OPS-013 remain unchanged',
)

const cleanCorePolicy = {
  'backend/system/core/Cargo.toml': `[features]
default = []
experimental-orchestration = []`,
  'backend/system/core/src/lib.rs': `
#[cfg(feature = "experimental-orchestration")]
pub mod experimental;
pub trait DataTransport {}
pub struct TransportMetadata;
pub enum TransportPriority { Normal }
pub struct TransportOptions;
pub struct TransportMessage;
pub struct TransportHealth;`,
  'backend/system/core/src/experimental/mod.rs': `
#[cfg(feature = "experimental-orchestration")]
pub mod orchestration;
pub use orchestration::{CompensationLog, ModuleOp, OrchestrationError, SagaStep};`,
  'backend/system/core/src/experimental/orchestration.rs': `
pub struct ModuleOp;
pub struct SagaStep;
pub struct OrchestrationError;
pub struct CompensationLog;`,
  'module-kit/INIT.md': `orchestration=EXPERIMENTAL_FEATURE; transport=KEEP_CORE_CANONICAL
Current Rust Runtime Profile
experimental-orchestration
system_core::experimental`,
  'module-kit/MODULE_TEMPLATE.md': `orchestration=EXPERIMENTAL_FEATURE; transport=KEEP_CORE_CANONICAL
Current Rust Runtime Profile
experimental-orchestration
system_core::experimental`,
}

assert.deepEqual(
  checkCoreExperimentalPrimitiveDisposition(cleanCorePolicy),
  [],
  'selected clean SP-05 policy must pass',
)

const rootOrchestrationResurrection = structuredClone(cleanCorePolicy)
rootOrchestrationResurrection['backend/system/core/src/lib.rs'] += '\npub struct ModuleOp;'
assert.equal(
  checkCoreExperimentalPrimitiveDisposition(rootOrchestrationResurrection)[0]?.rule,
  'TALOS-OPS-015',
  'root-level resurrection of moved orchestration must fail',
)

const missingCanonicalTransport = structuredClone(cleanCorePolicy)
missingCanonicalTransport['backend/system/core/src/lib.rs'] = missingCanonicalTransport['backend/system/core/src/lib.rs']
  .replace('pub trait DataTransport {}', '')
assert.equal(
  checkCoreExperimentalPrimitiveDisposition(missingCanonicalTransport)[0]?.rule,
  'TALOS-OPS-015',
  'the selected KEEP_CORE_CANONICAL transport disposition must fail when its root trait disappears',
)

const missingExperimentalFeature = structuredClone(cleanCorePolicy)
missingExperimentalFeature['backend/system/core/Cargo.toml'] = '[features]\ndefault = []'
assert.equal(
  checkCoreExperimentalPrimitiveDisposition(missingExperimentalFeature)[0]?.rule,
  'TALOS-OPS-015',
  'missing experimental-orchestration feature declaration must fail',
)

const unconditionalRootReExport = structuredClone(cleanCorePolicy)
unconditionalRootReExport['backend/system/core/src/lib.rs'] += '\npub use experimental::ModuleOp;'
assert(
  checkCoreExperimentalPrimitiveDisposition(unconditionalRootReExport)
    .some((finding) => finding.evidence.includes('unconditional root re-export')),
  'unconditional root re-export must fail',
)

const correctExperimentalModule = structuredClone(cleanCorePolicy)
correctExperimentalModule['backend/system/core/src/experimental/orchestration.rs'] += '\n// data contracts only; no runtime'
assert.deepEqual(
  checkCoreExperimentalPrimitiveDisposition(correctExperimentalModule),
  [],
  'correct feature-gated experimental module must pass',
)

const contradictoryModuleKit = structuredClone(cleanCorePolicy)
contradictoryModuleKit['module-kit/MODULE_TEMPLATE.md'] += '\npub struct SagaStep;'
assert.equal(
  checkCoreExperimentalPrimitiveDisposition(contradictoryModuleKit)[0]?.rule,
  'TALOS-OPS-015',
  'active module-kit contradiction must fail',
)

const markdownEvidence = structuredClone(cleanCorePolicy)
markdownEvidence['policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-05-core-experimental-primitives.md'] = 'pub struct ModuleOp; DataTransport'
assert.deepEqual(
  checkCoreExperimentalPrimitiveDisposition(markdownEvidence),
  [],
  'SP-05 Markdown evidence must pass',
)

const historicalPrimitiveEvidence = structuredClone(cleanCorePolicy)
historicalPrimitiveEvidence['policy/qualification/legacy-evidence/docs/historical/core.md'] = 'pub struct ModuleOp; pub trait DataTransport {}'
assert.deepEqual(
  checkCoreExperimentalPrimitiveDisposition(historicalPrimitiveEvidence),
  [],
  'historical primitive references must pass',
)

const unrelatedTransport = structuredClone(cleanCorePolicy)
unrelatedTransport['backend/thirdparty/webhook/src/lib.rs'] = 'pub struct WebhookTransport; pub struct HttpTransport;'
assert.deepEqual(
  checkCoreExperimentalPrimitiveDisposition(unrelatedTransport),
  [],
  'unrelated provider and webhook transport types must pass',
)

assert.deepEqual(
  [
    ...checkRemovedLegacySfPaths({}),
    ...checkRemovedLegacyExecutionContext({}),
    ...checkRemovedUnmountedReservationAdapterPath({}),
    ...checkBoundedStaleDeadSuppressions({}),
  ],
  [],
  'TALOS-OPS-011 through TALOS-OPS-014 remain unchanged',
)

assert.deepEqual(
  CORE_PRIMITIVE_DISPOSITION,
  { orchestration: 'EXPERIMENTAL_FEATURE', transport: 'KEEP_CORE_CANONICAL' },
  'each selected SP-05 disposition must be represented in policy',
)

const cleanApplicationBoundary = {
  'backend/src/application/mod.rs': `
mod clock;
mod module_client;
mod services;
pub use clock::{Clock, SystemClock};
pub use module_client::{ModuleClient, RegistryModuleClient};
pub use services::ApplicationServices;`,
  'backend/src/application/clock.rs': `
use chrono::{DateTime, Utc};
pub trait Clock: Send + Sync { fn now_utc(&self) -> DateTime<Utc>; }
pub struct SystemClock;
impl Clock for SystemClock { fn now_utc(&self) -> DateTime<Utc> { Utc::now() } }
pub struct FixedClock { instant: DateTime<Utc> }
impl Clock for FixedClock { fn now_utc(&self) -> DateTime<Utc> { self.instant } }`,
  'backend/src/application/module_client.rs': `
pub trait ModuleClient: Send + Sync { fn execute(&self); }
pub struct RegistryModuleClient { registry: Arc<ModuleRegistry> }
impl ModuleClient for RegistryModuleClient {
  fn execute(&self) { self.registry.execute("module", "command", payload, ctx); }
}`,
  'backend/src/application/services.rs': `
pub struct ApplicationServices {
  clock: Arc<dyn Clock>,
  module_client: Arc<dyn ModuleClient>,
  worker_runner: Arc<dyn WorkerRunner>,
  repository_provider: Arc<dyn RepositoryProvider>,
}
impl ApplicationServices { pub fn production(registry: Arc<ModuleRegistry>) -> Self { todo!() } }`,
  'backend/src/state.rs': `
pub struct AppState {
  pub pool: Pool,
  pub registry: Arc<ModuleRegistry>,
  application_services: Arc<ApplicationServices>,
}
impl AppState { pub fn application_services(&self) -> Arc<ApplicationServices> { self.application_services.clone() } }`,
  'backend/src/main.rs': `
mod application;
fn main() {
  let application_services = Arc::new(ApplicationServices::production(registry.clone()));
  let state = AppState::new(pool, config, registry, http_client, application_services);
}`,
}

assert.deepEqual(
  checkApplicationServicesBoundary(cleanApplicationBoundary),
  [],
  'clean ApplicationServices foundation must pass TALOS-OPS-016',
)

const missingApplicationModule = structuredClone(cleanApplicationBoundary)
delete missingApplicationModule['backend/src/application/mod.rs']
assert.equal(
  checkApplicationServicesBoundary(missingApplicationModule)[0]?.rule,
  'TALOS-OPS-016',
  'missing application module must fail',
)

const publicApplicationServiceField = structuredClone(cleanApplicationBoundary)
publicApplicationServiceField['backend/src/application/services.rs'] = publicApplicationServiceField['backend/src/application/services.rs']
  .replace('clock: Arc<dyn Clock>', 'pub clock: Arc<dyn Clock>')
assert(
  checkApplicationServicesBoundary(publicApplicationServiceField)
    .some((finding) => finding.evidence.includes('fields must remain private')),
  'public ApplicationServices fields must fail',
)

const missingApplicationWorker = structuredClone(cleanApplicationBoundary)
missingApplicationWorker['backend/src/application/services.rs'] = missingApplicationWorker['backend/src/application/services.rs']
  .replace('  worker_runner: Arc<dyn WorkerRunner>,\n', '')
assert(
  checkApplicationServicesBoundary(missingApplicationWorker)
    .some((finding) => finding.evidence.includes('exactly Clock')),
  'ApplicationServices missing WorkerRunner must fail',
)

const publicAppStateAggregate = structuredClone(cleanApplicationBoundary)
publicAppStateAggregate['backend/src/state.rs'] = publicAppStateAggregate['backend/src/state.rs']
  .replace('application_services: Arc<ApplicationServices>', 'pub application_services: Arc<ApplicationServices>')
assert(
  checkApplicationServicesBoundary(publicAppStateAggregate)
    .some((finding) => finding.evidence.includes('aggregate must remain private')),
  'public AppState aggregate must fail',
)

const nakedClockState = structuredClone(cleanApplicationBoundary)
nakedClockState['backend/src/state.rs'] = nakedClockState['backend/src/state.rs']
  .replace('application_services: Arc<ApplicationServices>,', 'application_services: Arc<ApplicationServices>,\n  clock: Arc<dyn Clock>,')
assert(
  checkApplicationServicesBoundary(nakedClockState)
    .some((finding) => finding.evidence.includes('naked application service')),
  'naked AppState Clock field must fail',
)

const nakedWorkerState = structuredClone(cleanApplicationBoundary)
nakedWorkerState['backend/src/state.rs'] = nakedWorkerState['backend/src/state.rs']
  .replace('application_services: Arc<ApplicationServices>,', 'application_services: Arc<ApplicationServices>,\n  worker_runner: Arc<dyn WorkerRunner>,')
assert(
  checkApplicationServicesBoundary(nakedWorkerState)
    .some((finding) => finding.evidence.includes('naked application service')),
  'naked AppState WorkerRunner field must fail',
)

const serviceWallClockRead = structuredClone(cleanApplicationBoundary)
serviceWallClockRead['backend/src/application/services.rs'] += '\nfn hidden_wall_read() { let _ = Utc::now(); }'
assert(
  checkApplicationServicesBoundary(serviceWallClockRead)
    .some((finding) => finding.evidence.includes('wall-clock read')),
  'wall-clock reads in application services must fail',
)

assert.deepEqual(
  checkApplicationServicesBoundary(cleanApplicationBoundary),
  [],
  'the Clock adapter remains the allowed wall-clock read boundary',
)

const registryBypass = structuredClone(cleanApplicationBoundary)
registryBypass['backend/src/application/module_client.rs'] = registryBypass['backend/src/application/module_client.rs']
  .replace('self.registry.execute("module", "command", payload, ctx)', 'self.registry.get("module").unwrap().execute("command", payload, ctx)')
assert(
  checkApplicationServicesBoundary(registryBypass)
    .some((finding) => finding.evidence.includes('must not bypass')),
  'direct Registry module bypass must fail',
)

assert.deepEqual(
  checkApplicationServicesBoundary(cleanApplicationBoundary),
  [],
  'RegistryModuleClient delegation through ModuleRegistry::execute must pass',
)

const genericServiceLocator = structuredClone(cleanApplicationBoundary)
genericServiceLocator['backend/src/application/services.rs'] += '\ntype Services = HashMap<String, Arc<dyn Any>>;'
assert(
  checkApplicationServicesBoundary(genericServiceLocator)
    .some((finding) => finding.evidence.includes('service-locator')),
  'generic string-keyed service locators must fail',
)

const applicationHistoricalEvidence = structuredClone(cleanApplicationBoundary)
applicationHistoricalEvidence['policy/qualification/legacy-evidence/docs/historical/application-services.md'] = 'registry.get("legacy").execute and Utc::now()'
assert.deepEqual(
  checkApplicationServicesBoundary(applicationHistoricalEvidence),
  [],
  'historical application composition text must remain allowed',
)

const unrelatedServiceStruct = structuredClone(cleanApplicationBoundary)
unrelatedServiceStruct['backend/src/services/report_service.rs'] = 'pub struct ReportService { pub clock_label: String }'
assert.deepEqual(
  checkApplicationServicesBoundary(unrelatedServiceStruct),
  [],
  'unrelated service structs must remain outside TALOS-OPS-016',
)

assert.deepEqual(
  [
    ...checkRemovedLegacySfPaths({}),
    ...checkRemovedLegacyExecutionContext({}),
    ...checkRemovedUnmountedReservationAdapterPath({}),
    ...checkBoundedStaleDeadSuppressions({}),
    ...checkCoreExperimentalPrimitiveDisposition(cleanCorePolicy),
  ],
  [],
  'TALOS-OPS-011 through TALOS-OPS-015 remain unchanged by TALOS-OPS-016',
)

console.log('TALOS Operations boundary checker self-test passed: 68 cases.')
