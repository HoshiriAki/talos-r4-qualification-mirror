#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDeviceCandidateCutoverSnapshot,
  validateDeviceCandidateCutoverSnapshot,
} from './check-r4-p8-device-candidate-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': missing mutation anchor ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDeviceCandidateCutoverSnapshot())
  mutate(snapshot)
  const errors = validateDeviceCandidateCutoverSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDeviceCandidateCutoverSnapshot(collectDeviceCandidateCutoverSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'order route regains raw SQLite pool',
  snapshot => {
    snapshot.orderRoute += '\nfn regression(state: &crate::AppState) { let _ = &state.pool; }\n'
  },
  'zero direct AppState SQLite pool dependencies',
)

expectFailure(
  'application stops binding repository to ExecutionContext',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      'repository_provider.bind(ctx)',
      'repository_provider.bind_unscoped()',
      'application stops binding repository to ExecutionContext',
    )
  },
  'repository_provider.bind(ctx)',
)

expectFailure(
  'PostgreSQL candidate query loses tenant predicate',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE tenant_id = $1',
      'WHERE 1 = 1',
      'PostgreSQL candidate query loses tenant predicate',
    )
  },
  'WHERE tenant_id = $1',
)

expectFailure(
  'candidate query stops probing max plus one',
  snapshot => {
    snapshot.dispatch = replaceRequired(
      snapshot.dispatch,
      'DEVICE_IMPORT_MATCH_CANDIDATES_MAX + 1',
      'DEVICE_IMPORT_MATCH_CANDIDATES_MAX',
      'candidate query stops probing max plus one',
    )
  },
  'DEVICE_IMPORT_MATCH_CANDIDATES_MAX + 1',
)

expectFailure(
  'legacy SQLite candidate loader returns to order route',
  snapshot => {
    snapshot.orderRoute += '\n// get_device_match_candidates(&state.pool, tenant)\n'
  },
  'get_device_match_candidates',
)

console.log('R4-P8 device candidate cutover mutation tests passed.')
