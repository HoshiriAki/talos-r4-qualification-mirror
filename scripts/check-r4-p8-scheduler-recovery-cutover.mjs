#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  integrationMod: 'backend/src/integration/mod.rs',
  contract: 'backend/src/integration/scheduler_persistence_contract.rs',
  sqliteAdapter: 'backend/src/integration/scheduler_persistence_sqlite.rs',
  postgresAdapter: 'backend/src/integration/scheduler_persistence_postgres.rs',
  worker: 'backend/src/integration/worker.rs',
  governedWorker: 'backend/src/integration/governed_worker.rs',
  liveQualification: 'backend/src/integration/scheduler_persistence_postgres_tests.rs',
  cursorMigration: 'backend/src/db/migrations/postgres/061_integration_scheduler_cursor.sql',
  snapshotMigration: 'backend/src/db/migrations/postgres/062_integration_startup_recovery_snapshot.sql',
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

export function collectP8SchedulerRecoverySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8SchedulerRecoverySnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) mod scheduler_persistence_contract;',
    'pub(crate) mod scheduler_persistence_postgres;',
    'pub(crate) mod scheduler_persistence_sqlite;',
    'mod scheduler_persistence_postgres_tests;',
  ]) {
    if (!snapshot.integrationMod.includes(token)) {
      errors.push(`Scheduler persistence module wiring missing: ${token}`)
    }
  }

  for (const token of [
    'pub(crate) const INTEGRATION_WORKER_SCHEDULER_ID',
    'pub(crate) const INTEGRATION_TENANT_PAGE_SIZE',
    'pub(crate) struct StartupRecoveryPage',
    'pub(crate) trait IntegrationSchedulerPersistence: Send + Sync',
    'fn begin_startup_recovery_snapshot(',
    'fn recover_next_startup_snapshot_page(',
    'fn next_tenant_work_page(&self)',
  ]) {
    if (!snapshot.contract.includes(token)) {
      errors.push(`Scheduler persistence contract missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::|sqlx::/.test(snapshot.contract)) {
    errors.push('Scheduler persistence contract must remain backend-neutral')
  }

  if (!snapshot.sqliteAdapter.includes(
    'impl IntegrationSchedulerPersistence for IntegrationStore',
  )) {
    errors.push('SQLite Scheduler compatibility adapter missing')
  }

  for (const token of [
    'scheduler: Arc<dyn IntegrationSchedulerPersistence>',
    'from_components(',
    'from_postgres(',
    'PostgresIntegrationSchedulerPersistence::new',
    'OperationRuntime::from_postgres',
    'WebhookRuntime::from_postgres',
    '.begin_startup_recovery_snapshot(',
    '.recover_next_startup_snapshot_page(',
    '.next_tenant_work_page(',
  ]) {
    if (!snapshot.worker.includes(token)) {
      errors.push(`Integration Worker scheduler dependency inversion missing: ${token}`)
    }
  }
  const workerStruct = between(
    snapshot.worker,
    'pub struct FixtureIntegrationWorker {',
    'impl FixtureIntegrationWorker {',
  )
  if (workerStruct.includes('store: IntegrationStore')) {
    errors.push('Integration Worker must not own concrete IntegrationStore scheduler authority')
  }

  for (const token of [
    'impl IntegrationSchedulerPersistence for PostgresIntegrationSchedulerPersistence',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'DELETE FROM integration_startup_recovery_snapshot',
    "FROM external_operations\n                 WHERE state='dispatching'",
    "FROM webhook_inbox\n                 WHERE status='processing'",
    "work_kind='external_operation'",
    "work_kind='webhook'",
    'FOR UPDATE',
    "state='unknown_outcome'",
    'worker_restarted_after_dispatch',
    'recovered_after_restart',
    "status='verified'",
    'worker_restarted_during_processing',
    "state='retryable_failure'",
    'DELETE FROM integration_startup_recovery_snapshot',
    'INSERT INTO integration_scheduler_cursor',
    'FROM integration_scheduler_cursor',
    'FOR UPDATE',
    "state IN ('ready','retryable_failure','dispatching')",
    "status IN ('verified','processing')",
    'WHERE tenant_id>$1',
    'UPDATE integration_scheduler_cursor',
  ]) {
    if (!snapshot.postgresAdapter.includes(token)) {
      errors.push(`PostgreSQL Scheduler/Recovery invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresAdapter)) {
    errors.push('PostgreSQL Scheduler/Recovery must not depend on SQLite runtime types')
  }
  for (const forbidden of [
    'provider_bindings',
    'provider_instances',
    'provider_manifests',
    'integration_deposits',
    'refund_intents',
    'audit_events',
  ]) {
    if (snapshot.postgresAdapter.includes(forbidden)) {
      errors.push(`PostgreSQL Scheduler/Recovery crossed persistence boundary: ${forbidden}`)
    }
  }

  for (const token of [
    "scheduler_id TEXT PRIMARY KEY CHECK (scheduler_id = 'fixture_integration_worker')",
    'tenant_id TEXT',
  ]) {
    if (!snapshot.cursorMigration.includes(token)) {
      errors.push(`Scheduler cursor migration authority missing: ${token}`)
    }
  }
  for (const token of [
    "scheduler_id TEXT NOT NULL CHECK (scheduler_id = 'fixture_integration_worker')",
    "work_kind TEXT NOT NULL CHECK (work_kind IN ('external_operation', 'webhook'))",
    'PRIMARY KEY (scheduler_id, recovery_id, work_kind, tenant_id, work_id)',
    'idx_integration_startup_recovery_snapshot_page',
  ]) {
    if (!snapshot.snapshotMigration.includes(token)) {
      errors.push(`Startup recovery migration authority missing: ${token}`)
    }
  }

  for (const token of [
    'live_pg18_scheduler_recovery_preserves_snapshot_boundary_atomicity_and_cursor_fairness',
    'begin_startup_recovery_snapshot',
    'operation-before',
    'webhook-before',
    'operation-after',
    'webhook-after',
    'page.recovered_operations, 1',
    'page.recovered_webhooks, 1',
    '"unknown_outcome"',
    '"dispatching"',
    '"verified"',
    '"processing"',
    '"retryable_failure"',
    'recovered_after_restart',
    'remaining_snapshot_rows, 0',
    'vec!["tenant-b".to_owned(), "tenant-c".to_owned()]',
    'Some("tenant-c")',
    'FixtureIntegrationWorker::from_postgres',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Scheduler/Recovery PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'scheduler: Arc<dyn IntegrationSchedulerPersistence>',
    'PostgresIntegrationSchedulerPersistence::new(pool)',
    'pub(crate) fn from_postgres_with_metrics(',
  ]) {
    if (!snapshot.governedWorker.includes(token)) {
      errors.push(`Governed Integration scheduler composition missing: ${token}`)
    }
  }
  if (!snapshot.main.includes(
    'GovernedIntegrationWorker::from_postgres_with_metrics(',
  )) {
    errors.push('Production composition root does not select PostgreSQL integration scheduler')
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Scheduler Recovery production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Scheduler Recovery cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8SchedulerRecoverySnapshot(
    collectP8SchedulerRecoverySnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Scheduler/Recovery cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Scheduler/Recovery PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
