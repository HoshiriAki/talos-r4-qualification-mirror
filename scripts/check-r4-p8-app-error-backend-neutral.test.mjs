#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectAppErrorSnapshot,
  validateAppErrorSnapshot,
} from './check-r4-p8-app-error-backend-neutral.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectAppErrorSnapshot())
  mutate(snapshot)
  const errors = validateAppErrorSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateAppErrorSnapshot(collectAppErrorSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'sqlite backend variant is restored',
  snapshot => {
    snapshot.error = replaceRequired(
      snapshot.error,
      '    #[error("storage operation failed")]\n    Storage,',
      '    #[error(transparent)]\n    Sqlite(#[from] rusqlite::Error),',
      'sqlite backend variant is restored',
    )
  },
  'Backend-specific AppError variant restored',
)

expectFailure(
  'pool conversion is removed',
  snapshot => {
    const start = snapshot.error.indexOf('impl From<r2d2::Error> for AppError')
    assert.ok(start >= 0, 'pool conversion mutation anchor missing')
    const end = snapshot.error.indexOf('\n}\n', start)
    assert.ok(end >= 0, 'pool conversion mutation end missing')
    snapshot.error = snapshot.error.slice(0, start) + snapshot.error.slice(end + 3)
  },
  'impl From<r2d2::Error> for AppError',
)

expectFailure(
  'explicit sqlite app error usage is restored',
  snapshot => {
    const key = 'services/api_key_service.rs'
    const fullKey = Object.keys(snapshot.sources).find(value => value.endsWith(key))
    assert.ok(fullKey, 'api key service source missing')
    snapshot.sources[fullKey] += '\nfn mutation_only(error: rusqlite::Error) { let _ = AppError::Sqlite(error); }\n'
  },
  'AppError::Sqlite usage restored',
)

expectFailure(
  'storage log class is backend-specific again',
  snapshot => {
    snapshot.error = replaceRequired(
      snapshot.error,
      'AppError::Storage => "storage"',
      'AppError::Storage => "sqlite"',
      'storage log class is backend-specific again',
    )
  },
  'AppError::Storage => "storage"',
)

console.log('R4-P8 AppError backend-neutral mutation tests passed.')
