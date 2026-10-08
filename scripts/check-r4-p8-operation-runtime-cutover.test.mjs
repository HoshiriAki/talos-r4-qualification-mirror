#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8OperationRuntimeSnapshot,
  validateP8OperationRuntimeSnapshot,
} from './check-r4-p8-operation-runtime-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8OperationRuntimeSnapshot())
  mutate(snapshot)
  const errors = validateP8OperationRuntimeSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8OperationRuntimeSnapshot(
  collectP8OperationRuntimeSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Operation Runtime mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Operation Runtime loses backend-neutral persistence port',
  snapshot => {
    snapshot.contract = snapshot.contract.replaceAll(
      'pub trait OperationRuntimePersistence: Send + Sync',
      'pub trait RemovedOperationRuntimePersistence: Send + Sync',
    )
  },
  'OperationRuntimePersistence',
)

expectFailure(
  'Operation Runtime persistence port becomes public API',
  snapshot => {
    snapshot.runtime = snapshot.runtime.replaceAll(
      'pub(crate) use super::operation_runtime_contract::OperationRuntimePersistence;',
      'pub use super::operation_runtime_contract::OperationRuntimePersistence;',
    )
  },
  'must remain internal',
)

expectFailure(
  'Operation Runtime executor regains concrete Store ownership',
  snapshot => {
    snapshot.runtime = snapshot.runtime.replaceAll(
      'persistence: Arc<dyn OperationRuntimePersistence>',
      'store: IntegrationStore',
    )
  },
  'must not own a concrete IntegrationStore field',
)

expectFailure(
  'Operation Runtime loses port-only injection proof',
  snapshot => {
    snapshot.runtime = snapshot.runtime.replaceAll(
      'runtime_accepts_persistence_port_without_integration_store',
      'removed_port_injection_proof',
    )
  },
  'runtime_accepts_persistence_port_without_integration_store',
)

expectFailure(
  'PostgreSQL Operation Runtime loses serializable authority',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL claim loses SKIP LOCKED fencing',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replaceAll(
      'FOR UPDATE OF o,b,i SKIP LOCKED',
      'FOR UPDATE OF o,b,i',
    )
  },
  'SKIP LOCKED',
)

expectFailure(
  'PostgreSQL claim loses tenant predicate',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replaceAll(
      'WHERE o.tenant_id=$1',
      'WHERE TRUE',
    )
  },
  'WHERE o.tenant_id=$1',
)

expectFailure(
  'PostgreSQL Operation Runtime gains SQLite coupling',
  snapshot => {
    snapshot.postgres += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite',
)

expectFailure(
  'PostgreSQL Operation Runtime crosses into webhook persistence',
  snapshot => {
    snapshot.postgres += '\n// webhook_inbox\n'
  },
  'crossed persistence boundary',
)

expectFailure(
  'Operation Runtime migration loses half-open single-probe guard',
  snapshot => {
    snapshot.runtimeMigration = snapshot.runtimeMigration.replaceAll(
      'integration_half_open_single_probe',
      'removed_half_open_probe_guard',
    )
  },
  'integration_half_open_single_probe',
)

expectFailure(
  'Live proof stops checking restart recovery evidence',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'recovered_after_restart',
      'removed_recovery_event',
    )
  },
  'recovered_after_restart',
)

expectFailure(
  'Live proof stops checking stale binding fail-closed',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'IntegrationError::BindingRevisionStale',
      'IntegrationError::Persistence',
    )
  },
  'BindingRevisionStale',
)

expectFailure(
  'Live proof stops checking manual-resolution transition',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      '"manual_resolution_required"',
      '"ready"',
    )
  },
  'manual_resolution_required',
)

expectFailure(
  'IntegrationModule operation planning bypasses composed persistence',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'self.operations.persist_operation(&operation)?',
      'self.store.persist_operation(&operation)?',
    )
  },
  'must not bypass composed persistence',
)

expectFailure(
  'PostgreSQL operation admission loses metrics-aware constructor',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'PostgresOperationRuntimePersistence::new_with_metrics(',
      'PostgresOperationRuntimePersistence::new(',
    )
  },
  'PostgresOperationRuntimePersistence::new_with_metrics(',
)

expectFailure(
  'Production root stops selecting PostgreSQL IntegrationModule operation persistence',
  snapshot => {
    snapshot.main = snapshot.main.replaceAll(
      'IntegrationModule::new_with_postgres_persistence(',
      'IntegrationModule::new(integration_store.clone())',
    )
  },
  'does not select PostgreSQL IntegrationModule operation persistence',
)

expectFailure(
  'Governed worker stops selecting PostgreSQL operation persistence',
  snapshot => {
    snapshot.governedWorker = snapshot.governedWorker.replaceAll(
      'OperationRuntime::from_postgres_with_metrics(',
      'OperationRuntime::new_with_metrics(',
    )
  },
  'OperationRuntime::from_postgres_with_metrics(',
)

expectFailure(
  'Production root stops selecting PostgreSQL governed integration runtime',
  snapshot => {
    snapshot.main = snapshot.main.replaceAll(
      'GovernedIntegrationWorker::from_postgres_with_metrics(',
      'GovernedIntegrationWorker::new_with_metrics(',
    )
  },
  'does not select PostgreSQL Governed Integration runtime',
)

expectFailure(
  'Operation Runtime production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Operation Runtime gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Operation Runtime cutover mutation tests passed.')
