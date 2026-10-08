#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDeviceAuthoritySnapshot,
  validateDeviceAuthoritySnapshot,
} from './check-r4-p8-device-authority-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDeviceAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateDeviceAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDeviceAuthoritySnapshot(collectDeviceAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'device route adds another raw pool path',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::AppState) { let _ = &state.pool; }\n'
  },
  'must not retain raw SQLite pool authority',
)

expectFailure(
  'bulk delete falls back to legacy SQLite service',
  snapshot => {
    snapshot.route += '\n// device_service::delete_device(&state.pool, "tenant", "serial")\n'
  },
  'device_service::delete_device(&state.pool',
)

expectFailure(
  'factory restores FeatureDevice SQLite authority',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'DeviceCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureDevice)',
    )
  },
  'DeviceCompatibilityModule::new(repository_provider.clone())',
)

expectFailure(
  'provider hides scoped device authority',
  snapshot => {
    snapshot.provider = replaceRequired(
      snapshot.provider,
      "pub fn devices(&self) -> ScopedDeviceRepository<'_>",
      "fn removed_devices(&self) -> ScopedDeviceRepository<'_>",
      'provider hides scoped device authority',
    )
  },
  'ScopedRepositories must expose',
)

expectFailure(
  'PostgreSQL point read loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE d.tenant_id = $1 AND d.serialno = $2',
      'WHERE d.serialno = $2',
      'PostgreSQL point read loses tenant scope',
    )
  },
  'WHERE d.tenant_id = $1 AND d.serialno = $2',
)

expectFailure(
  'PG18 proof loses cross-tenant delete rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'scoped_b.devices().delete("COREA003")?',
      'scoped_a.devices().delete("COREA003")?',
      'PG18 proof loses cross-tenant delete rejection',
    )
  },
  'scoped_b.devices().delete("COREA003")?',
)

console.log('R4-P8 device authority cutover mutation tests passed.')
