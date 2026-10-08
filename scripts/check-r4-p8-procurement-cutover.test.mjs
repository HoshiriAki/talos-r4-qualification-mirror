#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectProcurementCutoverSnapshot,
  validateProcurementCutoverSnapshot,
} from './check-r4-p8-procurement-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectProcurementCutoverSnapshot())
  mutate(snapshot)
  const errors = validateProcurementCutoverSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateProcurementCutoverSnapshot(collectProcurementCutoverSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Registry procurement restores raw SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'ProcurementCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureProcurement)',
      'Registry procurement restores raw SQLite construction',
    )
  },
  'restored SQLite runtime construction',
)

expectFailure(
  'PostgreSQL procurement point read loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE tenant_id=$1 AND device_serial_no=$2',
      'WHERE device_serial_no=$2',
      'PostgreSQL procurement point read loses tenant scope',
    )
  },
  'WHERE tenant_id=$1 AND device_serial_no=$2',
)

expectFailure(
  'procurement descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      '        NONE,\n        Procurement',
      '        SQLITE,\n        Procurement',
      'procurement descriptor restores SQLite storage',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'PG18 procurement proof loses cross-tenant same-serial evidence',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(created_a.device_serial_no, created_b.device_serial_no)',
      'assert_ne!(created_a.device_serial_no, created_b.device_serial_no)',
      'PG18 procurement proof loses cross-tenant same-serial evidence',
    )
  },
  'assert_eq!(created_a.device_serial_no, created_b.device_serial_no)',
)

console.log('R4-P8 procurement cutover mutation tests passed.')
