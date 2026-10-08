#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDeviceReadSnapshot,
  validateDeviceReadSnapshot,
} from './check-r4-p8-device-read-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDeviceReadSnapshot())
  mutate(snapshot)
  const errors = validateDeviceReadSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDeviceReadSnapshot(collectDeviceReadSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'paged route restores SQLite bounded helper',
  snapshot => {
    snapshot.route += '\n// device_bounded_read::query_devices_paged_bounded\n'
  },
  'query_devices_paged_bounded',
)

expectFailure(
  'PostgreSQL by-serial read loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'FROM devices d WHERE d.tenant_id = ',
      'FROM devices d WHERE ',
      'PostgreSQL by-serial read loses tenant scope',
    )
  },
  'FROM devices d WHERE d.tenant_id = ',
)

expectFailure(
  'device read authority drops dynamic warning calculation',
  snapshot => {
    snapshot.authority = replaceRequired(
      snapshot.authority,
      'fn calculate_warning(',
      'fn removed_calculate_warning(',
      'device read authority drops dynamic warning calculation',
    )
  },
  'fn calculate_warning(',
)

expectFailure(
  'PG18 proof loses tenant-filtered by-serial evidence',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'vec!["COREA001".to_string()]',
      'vec!["COREA001".to_string(), "COREB001".to_string()]',
      'PG18 proof loses tenant-filtered by-serial evidence',
    )
  },
  'vec!["COREA001".to_string()]',
)

console.log('R4-P8 device read cutover mutation tests passed.')
