#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  repositories: 'backend/src/repositories/mod.rs',
  tenantSource: 'backend/src/repositories/workflow_worker_tenant_source.rs',
  workers: 'backend/src/application/workers.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/workflow_worker_tenant_source_qualification_tests.rs',
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

export function collectP8WorkflowWorkerSourceSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8WorkflowWorkerSourceSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'mod workflow_worker_tenant_source;',
    'workflow_worker_tenant_source::WorkflowWorkerTenantSource',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push(`Workflow worker tenant-source wiring missing: ${token}`)
    }
  }

  for (const token of [
    'enum WorkflowWorkerTenantSourceBackend',
    'Sqlite(Pool<SqliteConnectionManager>)',
    'Postgres(sqlx::PgPool)',
    'pub(crate) fn postgres(pool: sqlx::PgPool)',
    'pub(crate) fn discover_due_tenants',
    "FROM workflow_instances",
    "WHERE status='active'",
    'FROM domain_outbox',
    "WHERE state='pending'",
    'ORDER BY tenant_id',
    'LIMIT $1',
    'WORKFLOW_WORKER_TENANT_LIMIT_INVALID',
    'WORKFLOW_WORKER_TENANT_SOURCE_UNAVAILABLE',
  ]) {
    if (!snapshot.tenantSource.includes(token)) {
      errors.push(`Workflow worker tenant-source invariant missing: ${token}`)
    }
  }

  const postgresBranch = between(
    snapshot.tenantSource,
    'WorkflowWorkerTenantSourceBackend::Postgres(pool) =>',
    '\n            }\n        }',
  )
  for (const forbidden of ['INSERT ', 'UPDATE ', 'DELETE ']) {
    if (postgresBranch.includes(forbidden)) {
      errors.push(`Workflow worker PostgreSQL tenant source must be read-only: ${forbidden.trim()}`)
    }
  }

  const workerBlock = between(
    snapshot.workers,
    'pub struct DurableRentalWorkflowWorker',
    'pub struct LegacyMaintenanceAdapter',
  )
  for (const token of [
    'tenant_source: WorkflowWorkerTenantSource',
    'new_with_tenant_source(',
    'new_with_tenant_source_and_metrics(',
    'WorkflowWorkerTenantSource::new(pool)',
    'self.tenant_source.discover_due_tenants(100)?',
  ]) {
    if (!workerBlock.includes(token)) {
      errors.push(`Durable rental worker tenant-source seam missing: ${token}`)
    }
  }
  const processDue = between(
    workerBlock,
    'fn process_due(&self)',
    '    }\n}\n',
  )
  for (const forbidden of [
    'self.pool.get()',
    '.prepare(',
    'SELECT tenant_id',
    'workflow_instances WHERE',
    'domain_outbox WHERE',
  ]) {
    if (processDue.includes(forbidden)) {
      errors.push(`Durable rental worker must not own tenant-discovery SQL: ${forbidden}`)
    }
  }

  if (!snapshot.pgMod.includes('mod workflow_worker_tenant_source_qualification_tests;')) {
    errors.push('Workflow worker tenant-source live PG18 qualification wiring missing')
  }
  for (const token of [
    'live_pg18_workflow_worker_tenant_source_discovers_deduplicates_limits_and_recomposes',
    'WorkflowWorkerTenantSource::postgres',
    'discover_due_tenants(2)',
    'discover_due_tenants(100)',
    '.map_err(anyhow::Error::msg)?',
    'source.discover_due_tenants(0).is_err()',
    'source.discover_due_tenants(1_001).is_err()',
    'let recomposed = WorkflowWorkerTenantSource::postgres',
    'UPDATE workflow_instances SET status=\'completed\'',
    'UPDATE domain_outbox SET state=\'delivered\'',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Workflow worker tenant-source live proof missing: ${token}`)
    }
  }

  for (const token of [
    'WorkflowWorkerTenantSource::postgres(pg)',
    'WorkflowWorkerTenantSource::new(pool)',
    'DurableRentalWorkflowWorker::new_with_tenant_source_and_metrics(',
    'workflow_worker_tenant_source',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Workflow worker production composition missing: ${token}`)
    }
  }

  const localWorkerBindingPattern =
    /let pool = require_sqlite_pool\("repository provider and workflow tenant source"\)\?;[\s\S]*?WorkflowWorkerTenantSource::new\(pool\)/g
  const localWorkerBindingCount = snapshot.main.match(localWorkerBindingPattern)?.length ?? 0
  if (localWorkerBindingCount !== 2) {
    errors.push(
      `Workflow worker local composition must bind the explicit SQLite capability directly in exactly both compile paths (found ${localWorkerBindingCount}, expected 2)`,
    )
  }
  if (snapshot.main.includes('DurableRentalWorkflowWorker::new_with_metrics(')) {
    errors.push('Workflow worker composition regressed to implicit SQLite tenant source')
  }

  if (!snapshot.workers.includes('pub struct LegacyMaintenanceAdapter')) {
    errors.push('Legacy maintenance boundary must remain explicit until its own PostgreSQL cutover')
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Workflow worker production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Workflow worker cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8WorkflowWorkerSourceSnapshot(
    collectP8WorkflowWorkerSourceSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 workflow worker tenant-source cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 workflow worker tenant-source PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
