#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectTenantPreviewSnapshot,
  validateTenantPreviewSnapshot,
} from './check-r4-p8-tenant-preview-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectTenantPreviewSnapshot())
  mutate(snapshot)
  const errors = validateTenantPreviewSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateTenantPreviewSnapshot(collectTenantPreviewSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Tenant Preview loses platform authority gate',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      'Self::require_platform_authority(ctx)?;',
      '// platform gate removed',
      'Tenant Preview loses platform authority gate',
    )
  },
  'require_platform_authority(ctx)?',
)

expectFailure(
  'Tenant Preview factory restores direct Feature SQLite construction',
  snapshot => {
    snapshot.factory += '\n// FeatureTenantPreview::with_pool(self.pool.clone())\n'
  },
  'direct Feature SQLite construction',
)

expectFailure(
  'Tenant Preview descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tenant_preview" => "feature-tenant-preview", Core, ModuleActivation::Always, NONE, TenantPreview)',
      'descriptor!("tenant_preview" => "feature-tenant-preview", Core, ModuleActivation::Always, SQLITE, TenantPreview)',
      'Tenant Preview descriptor restores SQLite storage',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'production composition loses PostgreSQL Preview backend',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'TenantPreviewRepository::postgres(pg.clone())',
      'TenantPreviewRepository::new(pool.clone())',
      'production composition loses PostgreSQL Preview backend',
    )
  },
  'backend selection missing',
)

expectFailure(
  'PostgreSQL Preview loses serialized session creation',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
      'PostgreSQL Preview loses serialized session creation',
    )
  },
  'PostgreSQL Tenant Preview invariant missing',
)

expectFailure(
  'PostgreSQL Preview loses actor-owned simulation check',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'FROM simulation_sessions',
      'FROM unrelated_simulations',
      'PostgreSQL Preview loses actor-owned simulation check',
    )
  },
  'PostgreSQL Tenant Preview invariant missing',
)

expectFailure(
  'Preview end loses paired workspace update',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'UPDATE tenant_workspace_sessions',
      'UPDATE unrelated_workspace_sessions',
      'Preview end loses paired workspace update',
    )
  },
  'PostgreSQL Tenant Preview invariant missing',
)

expectFailure(
  'Preview dashboard loses tenant scope',
  snapshot => {
    assert.ok(snapshot.postgres.includes('WHERE tenant_id=$1'), 'tenant scope anchor missing')
    snapshot.postgres = snapshot.postgres.replaceAll('WHERE tenant_id=$1', 'WHERE 1=1')
  },
  'PostgreSQL Tenant Preview invariant missing',
)

expectFailure(
  'Preview route trusts caller tenant instead of resolver',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      'let resolved = state\n        .registry\n        .execute(\n            "tenant_preview",\n            "session.resolve",',
      'let resolved = state\n        .registry\n        .execute(\n            "tenant_preview",\n            "session.get",',
      'Preview route trusts caller tenant instead of resolver',
    )
  },
  'server-owned resolution invariant missing',
)

expectFailure(
  'Preview route loses read-only execution mode',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      'ExecutionMode::ReadOnlyPreview(',
      'ExecutionMode::Normal /* preview removed */(',
      'Preview route loses read-only execution mode',
    )
  },
  'server-owned resolution invariant missing',
)

expectFailure(
  'PG18 Preview proof loses actor isolation',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'SIMULATION_NOT_FOUND',
      'SIMULATION_ACCEPTED',
      'PG18 Preview proof loses actor isolation',
    )
  },
  'PG18 Tenant Preview evidence missing',
)

console.log('R4-P8 Tenant Preview authority mutation tests passed.')
