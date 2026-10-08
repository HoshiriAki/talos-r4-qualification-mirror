#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectRegistryProviderDescriptorSnapshot,
  validateRegistryProviderDescriptorSnapshot,
} from './check-r4-p8-registry-provider-descriptors.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectRegistryProviderDescriptorSnapshot())
  mutate(snapshot)
  const errors = validateRegistryProviderDescriptorSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateRegistryProviderDescriptorSnapshot(
  collectRegistryProviderDescriptorSnapshot(),
)
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'customer descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      '        NONE,\n        Customer',
      '        SQLITE,\n        Customer',
      'customer descriptor restores SQLite requirement',
    )
  },
  'customer',
)

expectFailure(
  'reservation v2 descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      '        NONE,\n        ReservationV2',
      '        SQLITE,\n        ReservationV2',
      'reservation v2 descriptor restores SQLite requirement',
    )
  },
  'reservation_v2',
)

expectFailure(
  'r3 settlement descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      '        NONE,\n        R3Settlement',
      '        SQLITE,\n        R3Settlement',
      'r3 settlement descriptor restores SQLite requirement',
    )
  },
  'r3_settlement',
)

expectFailure(
  'order lifecycle runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureOrderLifecycleV2)\n'
  },
  'with_pool!(FeatureOrderLifecycleV2)',
)

console.log('R4-P8 provider-backed Registry descriptor mutation tests passed.')
