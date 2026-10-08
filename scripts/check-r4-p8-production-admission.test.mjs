#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectProductionAdmissionSnapshot,
  validateProductionAdmissionSnapshot,
} from './check-r4-p8-production-admission.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectProductionAdmissionSnapshot())
  mutate(snapshot)
  const errors = validateProductionAdmissionSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateProductionAdmissionSnapshot(collectProductionAdmissionSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'transitional barrier returns',
  snapshot => {
    snapshot.main +=
      '\nconst FORBIDDEN: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'runtime barrier is still present',
)

expectFailure(
  'production pg authority guard disappears',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'if config.is_production && pg_pool.is_none()',
      'if false',
      'production pg authority guard disappears',
    )
  },
  'Production admission invariant missing',
)

expectFailure(
  'postgres profile creates sqlite fallback',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      '    } else {\n        None\n    };\n    let require_sqlite_pool',
      '    } else {\n        Some(create_pool(&config.db_path)?)\n    };\n    let require_sqlite_pool',
      'postgres profile creates sqlite fallback',
    )
  },
  'exactly one local SQLite pool creation site',
)

expectFailure(
  'repository provider loses postgres branch',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'Arc::new(PostgresRepositoryProvider::new_with_metrics(',
      'Arc::new(SqliteRepositoryProvider::new_with_metrics(',
      'repository provider loses postgres branch',
    )
  },
  'Production admission invariant missing',
)

expectFailure(
  'startup gate still expects old barrier',
  snapshot => {
    snapshot.startupGate = snapshot.startupGate.replace(
      'Startup must not retain the transitional PostgreSQL admission barrier',
      'Startup accepts transitional barrier',
    )
  },
  'Startup gate has not been inverted',
)

console.log('R4-P8 production admission mutation tests passed.')
