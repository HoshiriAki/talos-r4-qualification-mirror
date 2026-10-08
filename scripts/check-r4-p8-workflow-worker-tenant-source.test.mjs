#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8WorkflowWorkerSourceSnapshot,
  validateP8WorkflowWorkerSourceSnapshot,
} from './check-r4-p8-workflow-worker-tenant-source.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8WorkflowWorkerSourceSnapshot())
  mutate(snapshot)
  const errors = validateP8WorkflowWorkerSourceSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8WorkflowWorkerSourceSnapshot(
  collectP8WorkflowWorkerSourceSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before workflow-worker mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'PostgreSQL tenant source loses active workflow discovery',
  snapshot => {
    snapshot.tenantSource = snapshot.tenantSource.replaceAll(
      "WHERE status='active'",
      "WHERE status='completed'",
    )
  },
  "WHERE status='active'",
)

expectFailure(
  'PostgreSQL tenant source loses pending outbox discovery',
  snapshot => {
    snapshot.tenantSource = snapshot.tenantSource.replaceAll(
      "WHERE state='pending'",
      "WHERE state='delivered'",
    )
  },
  "WHERE state='pending'",
)

expectFailure(
  'PostgreSQL tenant source becomes writable',
  snapshot => {
    snapshot.tenantSource = snapshot.tenantSource.replace(
      'WorkflowWorkerTenantSourceBackend::Postgres(pool) => {',
      'WorkflowWorkerTenantSourceBackend::Postgres(pool) => {\n                // UPDATE workflow_instances',
    )
  },
  'must be read-only',
)

expectFailure(
  'Durable worker regains direct SQLite discovery',
  snapshot => {
    snapshot.workers = snapshot.workers.replace(
      'let tenants = self.tenant_source.discover_due_tenants(100)?;',
      'let tenants = self.pool.get()?.prepare("SELECT tenant_id FROM workflow_instances WHERE status=\'active\'")?;',
    )
  },
  'tenant-source seam missing',
)

expectFailure(
  'Durable worker loses injectable tenant source',
  snapshot => {
    snapshot.workers = snapshot.workers.replaceAll(
      'new_with_tenant_source(',
      'removed_tenant_source_constructor(',
    )
  },
  'new_with_tenant_source(',
)

expectFailure(
  'Live proof stops checking deterministic limit',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'source.discover_due_tenants(2)',
      'source.discover_due_tenants(3)',
    )
  },
  'discover_due_tenants(2)',
)

expectFailure(
  'Live proof must explicitly map String errors into anyhow',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      '.map_err(anyhow::Error::msg)?',
      '?',
    )
  },
  '.map_err(anyhow::Error::msg)?',
)

expectFailure(
  'Live proof stops checking recomposition durability',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'let recomposed = WorkflowWorkerTenantSource::postgres',
      'let recomposed = REMOVED_WORKER_SOURCE::postgres',
    )
  },
  'let recomposed = WorkflowWorkerTenantSource::postgres',
)

expectFailure(
  'Live proof stops checking completed-work filtering',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      "UPDATE workflow_instances SET status='completed'",
      "UPDATE workflow_instances SET status='active'",
    )
  },
  "UPDATE workflow_instances SET status='completed'",
)

expectFailure(
  'one local workflow worker path bypasses explicit sqlite capability',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'let pool = require_sqlite_pool("repository provider and workflow tenant source")?;',
      'let pool = create_pool(&config.db_path)?;',
    )
  },
  'exactly both compile paths',
)

expectFailure(
  'local workflow worker keeps capability token but severs tenant-source binding',
  snapshot => {
    const source = snapshot.main
    snapshot.main = snapshot.main.replace(
      'WorkflowWorkerTenantSource::new(pool)',
      'WorkflowWorkerTenantSource::new(legacy_pool.clone())',
    )
    assert.notEqual(snapshot.main, source, 'workflow worker binding mutation anchor missing')
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'Production workflow tenant source falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'WorkflowWorkerTenantSource::postgres(pg)',
      'WorkflowWorkerTenantSource::new(pool.clone())',
    )
  },
  'WorkflowWorkerTenantSource::postgres(pg)',
)

expectFailure(
  'Durable workflow composition restores implicit SQLite tenant source',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'DurableRentalWorkflowWorker::new_with_tenant_source_and_metrics(',
      'DurableRentalWorkflowWorker::new_with_metrics(',
    )
  },
  'new_with_tenant_source_and_metrics(',
)

expectFailure(
  'Workflow worker production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Workflow worker gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 workflow worker tenant-source mutation tests passed.')
