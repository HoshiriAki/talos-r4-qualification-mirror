#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectAppStateSqliteOptionalSnapshot,
  validateAppStateSqliteOptionalSnapshot,
} from './check-r4-p8-appstate-sqlite-optional.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectAppStateSqliteOptionalSnapshot())
  mutate(snapshot)
  const errors = validateAppStateSqliteOptionalSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateAppStateSqliteOptionalSnapshot(collectAppStateSqliteOptionalSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'AppState restores raw SQLite field',
  snapshot => {
    snapshot.state = replaceRequired(
      snapshot.state,
      'pub struct AppState {',
      'pub struct AppState {\n    pub pool: Pool<SqliteConnectionManager>,',
      'AppState restores raw SQLite field',
    )
  },
  'raw SQLite pool field',
)

expectFailure(
  'backend-neutral constructor accepts SQLite pool',
  snapshot => {
    snapshot.state = replaceRequired(
      snapshot.state,
      'pub(crate) fn new(\n        config: AppConfig,',
      'pub(crate) fn new(\n        pool: Pool<SqliteConnectionManager>,\n        config: AppConfig,',
      'backend-neutral constructor accepts SQLite pool',
    )
  },
  'must not accept a SQLite pool',
)

expectFailure(
  'main uses local AppState constructor',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'let mut state = AppState::new(',
      'let mut state = AppState::new_local(',
      'main uses local AppState constructor',
    )
  },
  'main must not use the SQLite-local',
)

expectFailure(
  'metrics fixture loses explicit local composition',
  snapshot => {
    snapshot.metrics = replaceRequired(
      snapshot.metrics,
      'AppState::new_local(',
      'AppState::new(',
      'metrics fixture loses explicit local composition',
    )
  },
  'fixture must use the explicit AppState local constructor',
)

expectFailure(
  'tenant preview restores raw AppState pool access',
  snapshot => {
    snapshot.tenantPreview += '\nfn raw_pool_regression(state) { let _ = &state.pool; }\n'
  },
  'tenant-preview route must not access a raw SQLite pool',
)

expectFailure(
  'tenant preview stops resolving simulation authority',
  snapshot => {
    snapshot.tenantPreview = replaceRequired(
      snapshot.tenantPreview,
      '"tenant_simulation",',
      '"tenant_preview",',
      'tenant preview stops resolving simulation authority',
    )
  },
  'simulation authority derivation missing',
)

expectFailure(
  'post-construction backend override returns',
  snapshot => {
    snapshot.state += '\nfn set_auth_repositories() {}\n'
  },
  'post-construction backend override',
)

console.log('R4-P8 AppState optional-SQLite mutation tests passed.')
