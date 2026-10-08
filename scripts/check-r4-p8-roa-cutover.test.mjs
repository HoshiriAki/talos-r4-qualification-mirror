#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectRoaCutoverSnapshot,
  validateRoaCutoverSnapshot,
} from './check-r4-p8-roa-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectRoaCutoverSnapshot())
  mutate(snapshot)
  const errors = validateRoaCutoverSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateRoaCutoverSnapshot(collectRoaCutoverSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'factory restores SQLite ROA runtime',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'RoaCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureRoa)',
    )
  },
  'Registry ROA',
)

expectFailure(
  'PostgreSQL ROA asset lookup loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2',
      'WHERE ap.device_serial_no=$2',
      'PostgreSQL ROA asset lookup loses tenant scope',
    )
  },
  'WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2',
)

expectFailure(
  'PostgreSQL ROA revenue join loses tenant equality',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'od.tenant_id=ap.tenant_id',
      '1=1',
      'PostgreSQL ROA revenue join loses tenant equality',
    )
  },
  'od.tenant_id=ap.tenant_id',
)

expectFailure(
  'PostgreSQL ROA order scope loses tenant equality',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'o.tenant_id=ap.tenant_id',
      '1=1',
      'PostgreSQL ROA order scope loses tenant equality',
    )
  },
  'o.tenant_id=ap.tenant_id',
)

expectFailure(
  'ROA descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("roa", Business, ModuleActivation::Always, NONE, Roa)',
      'descriptor!("roa", Business, ModuleActivation::Always, SQLITE, Roa)',
      'ROA descriptor restores SQLite requirement',
    )
  },
  'ROA Registry descriptor',
)

expectFailure(
  'PG18 ROA proof loses tenant-specific revenue',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(basis_b.total_revenue, 900.0)',
      'assert_eq!(basis_b.total_revenue, 20.0)',
      'PG18 ROA proof loses tenant-specific revenue',
    )
  },
  'basis_b.total_revenue, 900.0',
)

console.log('R4-P8 ROA cutover mutation tests passed.')
