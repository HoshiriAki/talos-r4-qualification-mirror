#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectOrderHttpResidualSnapshot,
  validateOrderHttpResidualSnapshot,
} from './check-r4-p8-order-http-residual-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectOrderHttpResidualSnapshot())
  mutate(snapshot)
  const errors = validateOrderHttpResidualSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateOrderHttpResidualSnapshot(collectOrderHttpResidualSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'order route regains a raw pool dependency',
  snapshot => {
    snapshot.orders += '\nfn regression(state: &crate::AppState) { let _ = &state.pool; }\n'
  },
  'must not retain any direct AppState SQLite pool dependency',
)

expectFailure(
  'order route restores the legacy audit writer',
  snapshot => {
    snapshot.orders += '\n// audit_service::write_audit_log(\n'
  },
  'audit_service::write_audit_log(',
)

expectFailure(
  'audit route bypasses Registry through AppState pool',
  snapshot => {
    snapshot.auditRoute += '\nfn regression(state: &crate::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'PostgreSQL authority audit loses canonical insert',
  snapshot => {
    snapshot.auditPostgres = replaceRequired(
      snapshot.auditPostgres,
      'INSERT INTO audit_events',
      'INSERT INTO removed_audit_events',
      'PostgreSQL authority audit loses canonical insert',
    )
  },
  'INSERT INTO audit_events',
)

expectFailure(
  'composition root stops retaining selected audit authority',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,/,
      'AppStateRepositories::new(AuditCompatibilityRepository::new(pool.clone()),',
    )
  },
  'AppState bundle must consume the selected audit authority',
)

expectFailure(
  'live PG18 proof loses tenant membership provenance',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(authority.3.as_deref(), Some("membership-tenant-audit"));',
      'assert_eq!(authority.3.as_deref(), Some("removed-membership-proof"));',
      'live PG18 proof loses tenant membership provenance',
    )
  },
  'membership-tenant-audit',
)

console.log('R4-P8 order HTTP residual cutover mutation tests passed.')
