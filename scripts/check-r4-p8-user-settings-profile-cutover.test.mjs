#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectUserSettingsProfileSnapshot,
  validateUserSettingsProfileSnapshot,
} from './check-r4-p8-user-settings-profile-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectUserSettingsProfileSnapshot())
  mutate(snapshot)
  const errors = validateUserSettingsProfileSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateUserSettingsProfileSnapshot(collectUserSettingsProfileSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'route regains raw SQLite access',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'module loses self-only identity check',
  snapshot => {
    snapshot.module = replaceRequired(
      snapshot.module,
      'actor != requested',
      'actor == requested',
      'module loses self-only identity check',
    )
  },
  'actor != requested',
)

expectFailure(
  'PostgreSQL adapter starts treating legacy tenant marker as authority',
  snapshot => {
    snapshot.postgres += '\n// regression tenant_id\n'
  },
  'legacy tenant_id',
)

expectFailure(
  'factory keeps user-settings capability token but severs repository binding',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'UserSettingsProfileRepository::new(\n                self.require_sqlite_pool("user settings compatibility")?,\n            )',
      'UserSettingsProfileRepository::new(legacy_pool.clone())\n            /* self.require_sqlite_pool("user settings compatibility")? */',
      'factory keeps user-settings capability token but severs repository binding',
    )
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'production profile silently falls back to SQLite',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'UserSettingsProfileRepository::postgres(pg.clone())',
      'UserSettingsProfileRepository::new(pool.clone())',
      'production profile silently falls back to SQLite',
    )
  },
  'UserSettingsProfileRepository::postgres(pg.clone())',
)

expectFailure(
  'PG18 proof loses identity-global tenant marker assertion',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'legacy_tenant_marker.is_none()',
      'legacy_tenant_marker.is_some()',
      'PG18 proof loses identity-global tenant marker assertion',
    )
  },
  'legacy_tenant_marker.is_none()',
)

console.log('R4-P8 user settings profile cutover mutation tests passed.')
