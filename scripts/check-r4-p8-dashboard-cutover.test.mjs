#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDashboardSnapshot,
  validateDashboardSnapshot,
} from './check-r4-p8-dashboard-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDashboardSnapshot())
  mutate(snapshot)
  const errors = validateDashboardSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDashboardSnapshot(collectDashboardSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'dashboard route restores raw SQLite pool authority',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'dashboard route stops requiring resolved tenant extractor',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      'TenantUser',
      'AuthUser',
      'dashboard route stops requiring resolved tenant extractor',
    )
  },
  'AuthUser',
)

expectFailure(
  'PostgreSQL model ranking loses tenant-scoped device join',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'JOIN devices d ON d.serialno=od.serialno AND d.tenant_id=od.tenant_id',
      'JOIN devices d ON d.serialno=od.serialno',
      'PostgreSQL model ranking loses tenant-scoped device join',
    )
  },
  'd.tenant_id=od.tenant_id',
)

expectFailure(
  'SQLite cancel trend loses tenant predicate',
  snapshot => {
    snapshot.sqlite = replaceRequired(
      snapshot.sqlite,
      "WHERE tenant_id=?1 AND action='order_delete'",
      "WHERE action='order_delete'",
      'SQLite cancel trend loses tenant predicate',
    )
  },
  "WHERE tenant_id=?1 AND action='order_delete'",
)

expectFailure(
  'PostgreSQL cancel trend falls back to legacy audit view',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'FROM audit_events',
      'FROM audit_logs',
      'PostgreSQL cancel trend falls back to legacy audit view',
    )
  },
  'FROM audit_events',
)

expectFailure(
  'PG18 proof loses tenant-B dashboard isolation',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(overview_b.total_devices, 1)',
      'assert_eq!(overview_b.total_devices, 2)',
      'PG18 proof loses tenant-B dashboard isolation',
    )
  },
  'assert_eq!(overview_b.total_devices, 1)',
)

console.log('R4-P8 dashboard cutover mutation tests passed.')
