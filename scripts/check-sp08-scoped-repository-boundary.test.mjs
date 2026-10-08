#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  checkApplicationServicesBoundary,
  checkBoundedStaleDeadSuppressions,
  checkCoreExperimentalPrimitiveDisposition,
  checkRemovedLegacyExecutionContext,
  checkRemovedLegacySfPaths,
  checkRemovedUnmountedReservationAdapterPath,
} from './check-talos-ops-boundaries.mjs'
import { checkSp05Projection } from './check-sp05-module-kit-projection.mjs'
import { checkSp06ApplicationServicesContract } from './check-sp06-application-services-contract.mjs'
import { checkSp07ModuleFactoryWorkerBoundary } from './check-sp07-module-factory-worker-boundary.mjs'
import {
  checkSp08ScopedRepositoryBoundary,
  discoverChangedPaths,
} from './check-sp08-scoped-repository-boundary.mjs'

const clean = {
  'backend/src/repositories/contracts/binding.rs': `
pub enum RepositoryAccess { ReadOnly, ReadWrite }
pub struct RepositoryBinding { data_scope: DataScope, access: RepositoryAccess, correlation_id: RequestId }
impl RepositoryBinding {
  pub fn from_execution(ctx: &ExecutionContext) -> Result<Self, RepositoryError> {
    match ctx.execution_mode() {
      ExecutionMode::Normal => RepositoryAccess::ReadWrite,
      ExecutionMode::ReadOnlyPreview(_) => RepositoryAccess::ReadOnly,
      ExecutionMode::Simulation(_) => return Err(RepositoryError::SimulationUnsupported),
    };
    if ctx.data_scope().is_platform() { return Err(RepositoryError::TenantScopeRequired); }
    todo!()
  }
  pub fn tenant_id(&self) -> &TenantId { self.data_scope.tenant_id() }
}`,
  'backend/src/repositories/contracts/provider.rs': `
pub trait RepositoryProvider: Send + Sync {
  fn bind(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, RepositoryError>;
}
pub struct ScopedRepositories { session: SqliteRepositorySession }
impl ScopedRepositories {
  pub(in crate::repositories) fn sqlite(binding: RepositoryBinding, pool: Pool<SqliteConnectionManager>) -> Self { todo!() }
  pub fn binding(&self) -> &RepositoryBinding { self.session.binding() }
  pub(in crate::repositories) fn session(&self) -> &SqliteRepositorySession { &self.session }
}`,
  'backend/src/repositories/contracts/error.rs': `
pub enum RepositoryError { TenantScopeRequired, ScopeUnresolved, SimulationUnsupported, PreviewWriteDenied }
`,
  'backend/src/repositories/sqlite/mod.rs': `
mod session;
pub(in crate::repositories) use session::SqliteRepositorySession;
`,
  'backend/src/repositories/sqlite/provider.rs': `
pub struct SqliteRepositoryProvider { pool: Pool<SqliteConnectionManager> }
impl RepositoryProvider for SqliteRepositoryProvider {
  fn bind(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, RepositoryError> {
    let binding = RepositoryBinding::from_execution(ctx)?;
    Ok(ScopedRepositories::sqlite(binding, self.pool.clone()))
  }
}`,
  'backend/src/repositories/sqlite/session.rs': `
pub(in crate::repositories) struct SqliteRepositorySession {
  binding: RepositoryBinding,
  pool: Pool<SqliteConnectionManager>,
}
struct QueryOnlyGuard<'a> { connection: &'a Connection, restored: bool }
impl QueryOnlyGuard<'_> {
  fn restore(&mut self) { self.connection.pragma_update(None, "query_only", 0); }
}
impl Drop for QueryOnlyGuard<'_> {
  fn drop(&mut self) { self.connection.pragma_update(None, "query_only", 0); }
}
impl SqliteRepositorySession {
  pub(in crate::repositories) fn new(binding: RepositoryBinding, pool: Pool<SqliteConnectionManager>) -> Self { todo!() }
  pub(in crate::repositories) fn binding(&self) -> &RepositoryBinding { &self.binding }
  pub(in crate::repositories) fn read<T, F>(&self, operation: F) -> Result<T, RepositoryError> {
    self.connection.pragma_query_value(None, "query_only", |row| row.get(0));
    self.connection.pragma_update(None, "query_only", 1);
    todo!()
  }
  pub(in crate::repositories) fn write<T, F>(&self, operation: F) -> Result<T, RepositoryError> {
    if self.binding.access() == RepositoryAccess::ReadOnly { return Err(RepositoryError::PreviewWriteDenied); }
    Ok(())
  }
}`,
  'backend/src/application/services.rs': `
pub struct ApplicationServices {
  clock: Arc<dyn Clock>,
  module_client: Arc<dyn ModuleClient>,
  worker_runner: Arc<dyn WorkerRunner>,
  repository_provider: Arc<dyn RepositoryProvider>,
}`,
  'backend/src/state.rs': `
pub struct AppState {
  pub pool: Pool,
  application_services: Arc<ApplicationServices>,
}`,
  'backend/src/main.rs': 'fn main() {}',
  'backend/src/routes/orders.rs': 'pub fn orders() {}',
}

function changed(path, transform) {
  const fixture = structuredClone(clean)
  fixture[path] = transform(fixture[path] ?? '')
  return fixture
}

function added(path, source) {
  const fixture = structuredClone(clean)
  fixture[path] = source
  return fixture
}

function expectFailure(number, fixture, evidence, changedPaths = []) {
  assert(
    checkSp08ScopedRepositoryBoundary(fixture, changedPaths).some((item) => item.evidence.includes(evidence)),
    `${number}. expected failure containing: ${evidence}`,
  )
}

assert.deepEqual(checkSp08ScopedRepositoryBoundary(clean), [], '1. clean boundary passes')
assert.match(clean['backend/src/repositories/contracts/provider.rs'], /ctx: &ExecutionContext/, '2. provider bind accepts &ExecutionContext')

expectFailure(3, changed('backend/src/repositories/contracts/provider.rs', (source) => source.replace('ctx: &ExecutionContext', 'tenant_id: TenantId')), 'accept only &ExecutionContext')
expectFailure(4, changed('backend/src/repositories/contracts/provider.rs', (source) => source.replace('ctx: &ExecutionContext', 'tenant_id: String')), 'accept only &ExecutionContext')
expectFailure(5, changed('backend/src/repositories/contracts/provider.rs', (source) => source.replace('ctx: &ExecutionContext', 'scope: DataScope')), 'accept only &ExecutionContext')
expectFailure(6, changed('backend/src/repositories/sqlite/provider.rs', (source) => source.replace('pool: Pool', 'pub pool: Pool')), 'storage handles must be private')
expectFailure(7, added('backend/src/repositories/sqlite/leak.rs', 'pub fn connection(&self) -> &Connection { &self.connection }'), 'must not expose Pool, Connection, or Transaction getters')
expectFailure(8, added('backend/src/repositories/sqlite/leak.rs', "pub fn transaction(&self) -> &Transaction<'_> { todo!() }"), 'must not expose Pool, Connection, or Transaction getters')
expectFailure(9, added('backend/src/repositories/sqlite/leak.rs', 'pub fn execute<T, F>(&self, sql: &str, operation: F) -> T { todo!() }'), 'generic SQL executors')
expectFailure(10, changed('backend/src/state.rs', (source) => source.replace('application_services:', 'repository_provider: Arc<dyn RepositoryProvider>,\n  application_services:')), 'AppState must not own')
expectFailure(11, changed('backend/src/state.rs', (source) => source.replace('application_services:', 'scoped_repositories: ScopedRepositories,\n  application_services:')), 'AppState must not own')
expectFailure(12, changed('backend/src/routes/orders.rs', (source) => `use crate::repositories::RepositoryProvider;\n${source}`), 'routes must not import')
expectFailure(13, changed('backend/src/routes/orders.rs', () => 'fn route(state: AppState) { state.application_services().repository_provider(); }'), 'routes must not call')
expectFailure(14, changed('backend/src/application/services.rs', (source) => `${source}\ntype Locator = HashMap<String, Service>;`), 'generic service locator')
expectFailure(15, changed('backend/src/application/services.rs', (source) => `${source}\nfn cast(value: &dyn Any) { value.downcast_ref::<u8>(); }`), 'generic service locator')
expectFailure(16, added('backend/src/repositories/fallback.rs', 'const DEFAULT_TENANT: &str = "default";'), 'default or fallback tenant')
expectFailure(17, changed('backend/src/repositories/contracts/binding.rs', (source) => source.replace('ExecutionMode::Simulation(_) => return Err(RepositoryError::SimulationUnsupported)', 'ExecutionMode::Simulation(_) => Namespace::Production')), 'Simulation must not fall back')
expectFailure(18, changed('backend/src/application/services.rs', (source) => source.replace('  repository_provider: Arc<dyn RepositoryProvider>,\n', '')), 'exactly four private')
expectFailure(19, changed('backend/src/application/services.rs', (source) => source.replace('  repository_provider:', '  pub repository_provider:')), 'exactly four private')
expectFailure(20, changed('backend/src/application/services.rs', (source) => source.replace('  repository_provider:', '  scheduler: Arc<dyn Scheduler>,\n  repository_provider:')), 'exactly four private')

assert.deepEqual(checkSp08ScopedRepositoryBoundary(clean), [], '21. private SQLite provider implementation passes')
const testFixture = changed('backend/src/repositories/sqlite/session.rs', (source) => `${source}\n#[cfg(test)]\nmod tests { pub fn connection() -> &Connection { todo!() } }`)
assert.deepEqual(checkSp08ScopedRepositoryBoundary(testFixture), [], '22. test-only fixture repository passes')
assert.deepEqual(checkSp08ScopedRepositoryBoundary({ ...clean, 'policy/qualification/legacy-evidence/docs/repository-notes.md': 'public Pool field' }), [], '23. unrelated repository documentation passes')
const afterTest = changed('backend/src/repositories/sqlite/session.rs', (source) => `${source}\n#[cfg(test)]\nmod tests { pub fn connection() -> &Connection { todo!() } }\npub fn connection() -> &Connection { todo!() }`)
expectFailure(24, afterTest, 'must not expose Pool, Connection, or Transaction getters')

const priorGateRules = new Set([
  checkRemovedLegacySfPaths({ 'backend/src/routes/sf_express.rs': 'legacy' })[0]?.rule,
  checkRemovedLegacyExecutionContext({ 'backend/src/legacy.rs': 'LegacyExecutionContext' })[0]?.rule,
  checkRemovedUnmountedReservationAdapterPath({ 'backend/src/routes/reservation.rs': 'legacy' })[0]?.rule,
  checkBoundedStaleDeadSuppressions({ 'backend/src/services/order_service.rs': '#[allow(dead_code)] fn stale() {}' })[0]?.rule,
  checkCoreExperimentalPrimitiveDisposition({})[0]?.rule,
  checkApplicationServicesBoundary({})[0]?.rule,
  checkSp05Projection({})[0]?.rule,
  checkSp06ApplicationServicesContract({})[0]?.rule,
  checkSp07ModuleFactoryWorkerBoundary({})[0]?.rule,
])
assert(priorGateRules.has('TALOS-OPS-011') && priorGateRules.has('TALOS-OPS-012')
  && priorGateRules.has('TALOS-OPS-013') && priorGateRules.has('TALOS-OPS-014')
  && priorGateRules.has('TALOS-OPS-015') && priorGateRules.has('TALOS-OPS-016')
  && priorGateRules.has('TALOS-OPS-017'), '25. TALOS-OPS-011 through 017 remain active')

assert.deepEqual(
  checkSp08ScopedRepositoryBoundary(clean, ['.harness/feature_list.json']),
  [],
  '26. retired Harness cleanup is governed by repository layout rather than SP-08 scope',
)
expectFailure(27, changed('backend/src/repositories/contracts/binding.rs', (source) => source.replace('ExecutionMode::ReadOnlyPreview(_) => RepositoryAccess::ReadOnly', 'ExecutionMode::ReadOnlyPreview(_) => RepositoryAccess::ReadWrite')), 'Preview must never map to ReadWrite')
expectFailure(28, changed('backend/src/repositories/sqlite/session.rs', (source) => source.replace('struct QueryOnlyGuard', 'struct MissingQueryOnlyGuard')), 'enforce and restore SQLite query_only')
expectFailure(29, changed('backend/src/repositories/sqlite/session.rs', (source) => source.replace('impl Drop for QueryOnlyGuard', 'impl Drop for MissingQueryOnlyGuard')), 'enforce and restore SQLite query_only')
expectFailure(30, changed('backend/src/repositories/contracts/provider.rs', (source) => source.replace('pub(in crate::repositories) fn sqlite', 'pub(crate) fn sqlite')), 'construction and session access')
expectFailure(31, changed('backend/src/repositories/contracts/provider.rs', (source) => source.replace('pub(in crate::repositories) fn session', 'pub(crate) fn session')), 'construction and session access')
expectFailure(32, changed('backend/src/repositories/sqlite/session.rs', (source) => source.replace('pub(in crate::repositories) struct SqliteRepositorySession', 'pub(crate) struct SqliteRepositorySession')), 'session visibility')
expectFailure(33, changed('backend/src/repositories/sqlite/mod.rs', (source) => source.replace('pub(in crate::repositories) use', 'pub(crate) use')), 'session visibility')
expectFailure(34, added('backend/src/repositories/sqlite/leak.rs', 'pub pool: Arc<Pool<SqliteConnectionManager>>,'), 'storage handles must be private')
expectFailure(35, added('backend/src/repositories/sqlite/leak.rs', 'pub connection: Option<Connection>,'), 'storage handles must be private')
expectFailure(36, added('backend/src/repositories/sqlite/leak.rs', 'pub(crate) fn connection(&self) -> &Connection { todo!() }'), 'must not expose Pool, Connection, or Transaction getters')
assert.deepEqual(
  checkSp08ScopedRepositoryBoundary(added('backend/src/repositories/sqlite/internal.rs', 'pub(in crate::repositories) fn connection(&self) -> &Connection { todo!() }')),
  [],
  '37. crate::repositories internal raw handle remains allowed',
)

function stagedPaths(path) {
  return discoverChangedPaths('/repo', (_command, args) => (
    args.includes('--cached') ? `${path}\n` : ''
  ))
}
assert.deepEqual(
  checkSp08ScopedRepositoryBoundary(clean, stagedPaths('.harness/feature_list.json')),
  [],
  '38. staged Harness retirement is outside SP-08 implementation authority',
)
assert.deepEqual(
  checkSp08ScopedRepositoryBoundary(clean, stagedPaths('.harness/PROGRESS.md')),
  [],
  '39. staged Harness progress retirement is outside SP-08 implementation authority',
)
const discoveryFailure = discoverChangedPaths('/repo', () => { throw new Error('boom') })
expectFailure(40, clean, 'changed-path discovery failed closed', discoveryFailure)
assert.deepEqual(
  checkSp08ScopedRepositoryBoundary(clean, stagedPaths('policy/qualification/legacy-evidence/docs/README.md')),
  [],
  '41. ordinary staged path remains allowed',
)

console.log('SP-08 scoped repository boundary checker self-test passed.')
