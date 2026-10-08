#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectWorkTaskSnapshot,
  validateWorkTaskSnapshot,
} from './check-r4-p8-work-task-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectWorkTaskSnapshot())
  mutate(snapshot)
  const errors = validateWorkTaskSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateWorkTaskSnapshot(collectWorkTaskSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'task route loses trusted tenant extraction',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      'TrustedTenantUser',
      'AuthUser',
      'task route loses trusted tenant extraction',
    )
  },
  'TrustedTenantUser',
)

expectFailure(
  'task route regains direct SQLite access',
  snapshot => {
    snapshot.route += '\nfn regression(state: &AppState) { let _ = state.pool.get(); }\n'
  },
  'state.pool',
)

expectFailure(
  'PostgreSQL point read loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE tenant_id = $1 AND id = $2',
      'WHERE id = $2',
      'PostgreSQL point read loses tenant scope',
    )
  },
  'WHERE tenant_id = $1 AND id = $2',
)

expectFailure(
  'PostgreSQL optimistic update loses version CAS',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE tenant_id = $3 AND id = $4 AND version = $5',
      'WHERE tenant_id = $3 AND id = $4',
      'PostgreSQL optimistic update loses version CAS',
    )
  },
  'WHERE tenant_id = $3 AND id = $4 AND version = $5',
)

expectFailure(
  'task errors drift into new public coded responses',
  snapshot => {
    snapshot.error = replaceRequired(
      snapshot.error,
      '"BIZ_WORK_TASK_NOT_FOUND" => AppError::NotFound(message)',
      '"BIZ_WORK_TASK_NOT_FOUND" => AppError::CodedNotFound { code: code.to_string(), message }',
      'task errors drift into new public coded responses',
    )
  },
  'legacy public HTTP error codes',
)

expectFailure(
  'factory stops composing the Registry module',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'insert(work_task_compatibility_built)',
      'drop(work_task_compatibility_built)',
      'factory stops composing the Registry module',
    )
  },
  'insert(work_task_compatibility_built)',
)

console.log('R4-P8 work task cutover mutation tests passed.')
