#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectRepairAuthoritySnapshot,
  validateRepairAuthoritySnapshot,
} from './check-r4-p8-repair-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectRepairAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateRepairAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateRepairAuthoritySnapshot(collectRepairAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Repair factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'RepairCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureRepair)',
      'Repair factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Repair descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("repair", Business, ModuleActivation::Always, NONE, Repair)',
      'descriptor!("repair", Business, ModuleActivation::Always, SQLITE, Repair)',
      'Repair descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Repair create loses damage row lock',
  snapshot => {
    snapshot.pgCreate = replaceRequired(
      snapshot.pgCreate,
      'FOR UPDATE',
      'NO_ROW_LOCK',
      'Repair create loses damage row lock',
    )
  },
  'create invariant missing',
)

expectFailure(
  'Repair transition loses serialization',
  snapshot => {
    snapshot.pgTransition = replaceRequired(
      snapshot.pgTransition,
      'pg_write_serializable_repository',
      'pg_write',
      'Repair transition loses serialization',
    )
  },
  'transition invariant missing',
)

expectFailure(
  'Repair return loses atomic device update',
  snapshot => {
    snapshot.pgTransition = replaceRequired(
      snapshot.pgTransition,
      "UPDATE devices SET rentalstatus='available'",
      "UPDATE unrelated_devices SET rentalstatus='available'",
      'Repair return loses atomic device update',
    )
  },
  'transition invariant missing',
)

expectFailure(
  'Repair reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id='), 'tenant mutation anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
  },
  'tenant-scoped',
)

expectFailure(
  'Repair live proof loses device return',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'device.status, "available"',
      'device.status, "repairing"',
      'Repair live proof loses device return',
    )
  },
  'device.status',
)

console.log('R4-P8 Repair authority mutation tests passed.')
