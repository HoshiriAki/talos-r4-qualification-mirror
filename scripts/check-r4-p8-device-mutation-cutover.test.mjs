#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDeviceMutationSnapshot,
  validateDeviceMutationSnapshot,
} from './check-r4-p8-device-mutation-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDeviceMutationSnapshot())
  mutate(snapshot)
  const errors = validateDeviceMutationSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDeviceMutationSnapshot(collectDeviceMutationSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'device route restores raw SQLite mutation authority',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'checkin drops legacy order auto-completion compatibility',
  snapshot => {
    snapshot.mutationAuthority = replaceRequired(
      snapshot.mutationAuthority,
      '.complete_legacy_orders_after_checkin(',
      '.removed_legacy_order_completion(',
      'checkin drops legacy order auto-completion compatibility',
    )
  },
  '.complete_legacy_orders_after_checkin(',
)

expectFailure(
  'PostgreSQL legacy completion loses tenant-scoped device join',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'ON d.serialno=od.serialno AND d.tenant_id=od.tenant_id',
      'ON d.serialno=od.serialno',
      'PostgreSQL legacy completion loses tenant-scoped device join',
    )
  },
  'ON d.serialno=od.serialno AND d.tenant_id=od.tenant_id',
)

expectFailure(
  'PG18 proof loses legacy order completion parity',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'vec!["legacy-order-a".to_string()]',
      'Vec::<String>::new()',
      'PG18 proof loses legacy order completion parity',
    )
  },
  'vec!["legacy-order-a".to_string()]',
)

console.log('R4-P8 device mutation cutover mutation tests passed.')
