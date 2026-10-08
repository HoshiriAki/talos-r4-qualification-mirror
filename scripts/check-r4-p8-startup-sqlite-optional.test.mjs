#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectStartupSqliteOptionalSnapshot,
  validateStartupSqliteOptionalSnapshot,
} from './check-r4-p8-startup-sqlite-optional.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectStartupSqliteOptionalSnapshot())
  mutate(snapshot)
  const errors = validateStartupSqliteOptionalSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateStartupSqliteOptionalSnapshot(collectStartupSqliteOptionalSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'startup restores ambient mandatory sqlite pool',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'let sqlite_pool = if database_profile == DatabaseProfile::SqliteLocal {',
      'let pool = create_pool(&config.db_path)?;\n    let sqlite_pool = if database_profile == DatabaseProfile::SqliteLocal {',
      'startup restores ambient mandatory sqlite pool',
    )
  },
  'ambient mandatory SQLite pool',
)

expectFailure(
  'postgres profile starts creating sqlite',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      '        Some(create_pool(&config.db_path)?)\n    } else {\n        None\n    };',
      '        Some(create_pool(&config.db_path)?)\n    } else {\n        Some(create_pool(&config.db_path)?)\n    };',
      'postgres profile starts creating sqlite',
    )
  },
  'exactly one SQLite pool creation site',
)

expectFailure(
  'registry assembly drops optional capability',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      '            sqlite_pool.clone(),',
      '            None,',
      'registry assembly drops optional capability',
    )
  },
  'Startup optional-SQLite invariant missing',
)

expectFailure(
  'startup regresses to pg absence inference',
  snapshot => {
    snapshot.main += '\nfn forbidden() { let _ = pg_pool.is_none().then(|| create_pool(&config.db_path)); }\n'
  },
  'must not infer SQLite capability',
)

expectFailure(
  'transitional postgres admission barrier is restored',
  snapshot => {
    snapshot.main +=
      '\nfn forbidden_barrier() { anyhow::bail!("R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback"); }\n'
  },
  'must not retain the transitional PostgreSQL admission barrier',
)

console.log('R4-P8 startup optional-SQLite mutation tests passed.')
