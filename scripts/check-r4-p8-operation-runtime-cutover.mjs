#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  integrationMod: 'backend/src/integration/mod.rs',
  integrationModule: 'backend/src/integration/module.rs',
  contract: 'backend/src/integration/operation_runtime_contract.rs',
  runtime: 'backend/src/integration/runtime.rs',
  governedWorker: 'backend/src/integration/governed_worker.rs',
  postgres: 'backend/src/integration/operation_runtime_postgres.rs',
  liveQualification: 'backend/src/integration/operation_runtime_postgres_tests.rs',
  runtimeMigration: 'backend/src/db/migrations/postgres/060_integration_runtime.sql',
  main: 'backend/src/main.rs',
}

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

export function collectP8OperationRuntimeSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8OperationRuntimeSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) mod operation_runtime_contract;',
    'pub(crate) mod operation_runtime_postgres;',
    'mod operation_runtime_postgres_tests;',
  ]) {
    if (!snapshot.integrationMod.includes(token)) {
      errors.push(`Operation Runtime module wiring missing: ${token}`)
    }
  }

  for (const token of [
    'pub trait OperationRuntimePersistence: Send + Sync',
    'fn persist_operation(',
    'fn claim_next_operation(',
    'fn record_runtime_outcome(',
    'fn recover_inflight_operations(',
    'fn operation_runtime_metrics(',
    'fn begin_reconciliation(',
    'fn resolve_reconciliation(',
    'pub struct OperationRuntimePolicy',
    'pub struct ClaimedOperation',
    'pub struct OperationRuntimeMetrics',
  ]) {
    if (!snapshot.contract.includes(token)) {
      errors.push(`Operation Runtime persistence contract missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.contract)) {
    errors.push('Operation Runtime persistence contract must remain backend-neutral')
  }

  for (const token of [
    'impl OperationRuntimePersistence for IntegrationStore',
    'persistence: Arc<dyn OperationRuntimePersistence>',
    'from_persistence(',
    'from_postgres(',
    'PostgresOperationRuntimePersistence::new(pool)',
    'runtime_accepts_persistence_port_without_integration_store',
    'pub(crate) use super::operation_runtime_contract::OperationRuntimePersistence;',
    '.claim_next_operation(',
    '.record_runtime_outcome(',
  ]) {
    if (!snapshot.runtime.includes(token)) {
      errors.push(`Operation Runtime dependency inversion missing: ${token}`)
    }
  }
  if (snapshot.runtime.includes(
    'pub use super::operation_runtime_contract::OperationRuntimePersistence',
  )) {
    errors.push('Operation Runtime persistence port must remain internal')
  }

  const runtimeStruct = between(
    snapshot.runtime,
    'pub struct OperationRuntime {',
    'impl OperationRuntime {',
  )
  if (runtimeStruct.includes('store: IntegrationStore')) {
    errors.push('Operation Runtime executor must not own a concrete IntegrationStore field')
  }

  for (const token of [
    'impl OperationRuntimePersistence for PostgresOperationRuntimePersistence',
    'metrics: Arc<dyn MetricsSink>',
    'pub(crate) fn new_with_metrics(',
    'if admitted {',
    'IntegrationEvent::Admitted',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'ON CONFLICT (tenant_id,binding_id,idempotency_key) DO NOTHING',
    'WHERE o.tenant_id=$1',
    'FOR UPDATE OF o,b,i SKIP LOCKED',
    'WHERE tenant_id=$1 AND binding_id=$2',
    "state='dispatching'",
    'BindingRevisionStale',
    'ConcurrencyLimited',
    'RateLimited',
    'integration_circuit_state',
    'external_operation_attempts',
    'external_operation_reconciliations',
    'external_operation_runtime_events',
    'worker_restarted_after_dispatch',
    'SERIALIZABLE READ ONLY',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push(`PostgreSQL Operation Runtime invariant missing: ${token}`)
    }
  }

  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL Operation Runtime must not depend on SQLite runtime types')
  }
  for (const forbidden of [
    'webhook_',
    'integration_scheduler_cursor',
    'integration_startup_recovery_snapshot',
    'integration_deposits',
    'refund_intents',
  ]) {
    if (snapshot.postgres.includes(forbidden)) {
      errors.push(`PostgreSQL Operation Runtime crossed persistence boundary: ${forbidden}`)
    }
  }

  for (const token of [
    'integration_instance_invalidates_queued_operations',
    'integration_binding_invalidates_queued_operations',
    'integration_half_open_single_probe',
    "state IN ('ready', 'retryable_failure')",
  ]) {
    if (!snapshot.runtimeMigration.includes(token)) {
      errors.push(`Operation Runtime migration authority missing: ${token}`)
    }
  }

  for (const token of [
    'operations: Arc<dyn OperationRuntimePersistence>',
    'new_with_postgres_persistence(',
    'PostgresOperationRuntimePersistence::new_with_metrics(',
    'self.operations.persist_operation(&operation)?',
  ]) {
    if (!snapshot.integrationModule.includes(token)) {
      errors.push(`IntegrationModule Operation persistence composition missing: ${token}`)
    }
  }
  if (snapshot.integrationModule.includes('self.store.persist_operation(')) {
    errors.push('IntegrationModule operation admission must not bypass composed persistence')
  }
  if (!snapshot.main.includes('IntegrationModule::new_with_postgres_persistence(')) {
    errors.push('Production root does not select PostgreSQL IntegrationModule operation persistence')
  }

  for (const token of [
    'live_pg18_operation_runtime_preserves_claim_outcome_recovery_and_reconciliation',
    'PostgresOperationRuntimePersistence::new',
    'persist_operation(&primary)?',
    '.claim_next_operation("tenant-b", &policy)?',
    'TransportFailure::after_dispatch',
    '"unknown_outcome"',
    'begin_reconciliation',
    'resolve_reconciliation',
    'attempt_number, 2',
    'DispatchResult::Succeeded',
    '.recover_inflight_operations("tenant-a")?',
    'recovered_after_restart',
    'BindingRevisionStale',
    '"manual_resolution_required"',
    'metrics.unknown_outcome, 1',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Operation Runtime PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'from_postgres_with_metrics(',
    'PostgresOperationRuntimePersistence::new(pool)',
  ]) {
    if (!snapshot.runtime.includes(token)) {
      errors.push(`Operation Runtime PostgreSQL composition seam missing: ${token}`)
    }
  }
  for (const token of [
    'scheduler: Arc<dyn IntegrationSchedulerPersistence>',
    'OperationRuntime::from_postgres_with_metrics(',
    'pub(crate) fn from_postgres_with_metrics(',
  ]) {
    if (!snapshot.governedWorker.includes(token)) {
      errors.push(`Governed Integration worker operation composition missing: ${token}`)
    }
  }
  if (!snapshot.main.includes(
    'GovernedIntegrationWorker::from_postgres_with_metrics(',
  )) {
    errors.push('Production composition root does not select PostgreSQL Governed Integration runtime')
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Operation Runtime production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Operation Runtime cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8OperationRuntimeSnapshot(
    collectP8OperationRuntimeSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Operation Runtime cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Operation Runtime PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
