#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectModuleFactorySqliteOptionalSnapshot,
  validateModuleFactorySqliteOptionalSnapshot,
} from './check-r4-p8-module-factory-sqlite-optional.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectModuleFactorySqliteOptionalSnapshot())
  mutate(snapshot)
  const errors = validateModuleFactorySqliteOptionalSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateModuleFactorySqliteOptionalSnapshot(
  collectModuleFactorySqliteOptionalSnapshot(),
)
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'factory restores mandatory sqlite field',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'pool: Option<Pool<SqliteConnectionManager>>',
      'pool: Pool<SqliteConnectionManager>',
      'factory restores mandatory sqlite field',
    )
  },
  'pool:Option<Pool<SqliteConnectionManager>>',
)

expectFailure(
  'sqlite-free constructor disappears',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'pub(crate) fn new_without_sqlite(',
      'pub(crate) fn removed_without_sqlite(',
      'sqlite-free constructor disappears',
    )
  },
  'new_without_sqlite',
)

expectFailure(
  'factory regains ambient pool use',
  snapshot => {
    snapshot.factory += '\nfn forbidden(factory: &ModuleFactory) { let _ = factory.self.pool.clone(); }\n'
  },
  'only inside require_sqlite_pool',
)

expectFailure(
  'assembler drops sqlite-free branch',
  snapshot => {
    snapshot.assembler = replaceRequired(
      snapshot.assembler,
      'None => ModuleFactory::new_without_sqlite(http_client, provider_config)',
      'None => panic!("sqlite required")',
      'assembler drops sqlite-free branch',
    )
  },
  'new_without_sqlite',
)

expectFailure(
  'runtime stops threading selected sqlite capability',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      '            sqlite_pool.clone(),\n            http_client.clone(),',
      '            None,\n            http_client.clone(),',
      'runtime stops threading selected sqlite capability',
    )
  },
  'optional SQLite capability',
)

console.log('R4-P8 ModuleFactory optional-SQLite mutation tests passed.')
