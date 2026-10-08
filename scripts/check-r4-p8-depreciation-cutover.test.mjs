#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDepreciationCutoverSnapshot,
  validateDepreciationCutoverSnapshot,
} from './check-r4-p8-depreciation-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDepreciationCutoverSnapshot())
  mutate(snapshot)
  const errors = validateDepreciationCutoverSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDepreciationCutoverSnapshot(collectDepreciationCutoverSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'factory restores SQLite depreciation runtime',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'DepreciationCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureDepreciation)',
    )
  },
  'Registry depreciation',
)

expectFailure(
  'PostgreSQL snapshot loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2',
      'WHERE ap.device_serial_no=$2',
      'PostgreSQL snapshot loses tenant scope',
    )
  },
  'WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2',
)

expectFailure(
  'PostgreSQL monthly run loses serializable authority',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'pg_write_serializable_repository',
      'pg_write',
      'PostgreSQL monthly run loses serializable authority',
    )
  },
  'pg_write_serializable_repository',
)

expectFailure(
  'depreciation descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'NONE,\n        Depreciation',
      'SQLITE,\n        Depreciation',
      'depreciation descriptor restores SQLite requirement',
    )
  },
  'descriptor',
)

expectFailure(
  'PG18 proof loses duplicate-period rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'Err(DepreciationMutationError::AlreadyRun(2))',
      'Err(DepreciationMutationError::AlreadyRun(99))',
      'PG18 proof loses duplicate-period rejection',
    )
  },
  'AlreadyRun(2)',
)

console.log('R4-P8 depreciation cutover mutation tests passed.')
