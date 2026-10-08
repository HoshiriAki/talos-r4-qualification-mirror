#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectAuthAuditResidualSnapshot,
  validateAuthAuditResidualSnapshot,
} from './check-r4-p8-auth-audit-residual-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectAuthAuditResidualSnapshot())
  mutate(snapshot)
  const errors = validateAuthAuditResidualSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateAuthAuditResidualSnapshot(collectAuthAuditResidualSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'auth route regains raw pool access',
  snapshot => {
    snapshot.authRoute = replaceRequired(
      snapshot.authRoute,
      'pub fn auth_routes()',
      'fn regression(state: &crate::AppState) { let _ = &state.pool; }\n\npub fn auth_routes()',
      'auth route regains raw pool access',
    )
  },
  'direct AppState SQLite pool dependency',
)

expectFailure(
  'auth route restores legacy audit writer',
  snapshot => {
    snapshot.authRoute = replaceRequired(
      snapshot.authRoute,
      'audit_service::write_audit_log_with_repository',
      'audit_service::write_audit_log',
      'auth route restores legacy audit writer',
    )
  },
  'legacy SQLite audit writer',
)

expectFailure(
  'auth route drops one semantic audit call',
  snapshot => {
    snapshot.authRoute = replaceRequired(
      snapshot.authRoute,
      '"profile_update"',
      '"profile_update_removed"',
      'auth route drops one semantic audit call',
    )
  },
  '"profile_update"',
)

expectFailure(
  'auth route stops using selected audit authority',
  snapshot => {
    snapshot.authRoute = replaceRequired(
      snapshot.authRoute,
      'state.audit_compatibility_repository()',
      'state.identity_authority_repository()',
      'auth route stops using selected audit authority',
    )
  },
  'selected audit authority handle',
)

console.log('R4-P8 auth audit residual cutover mutation tests passed.')
