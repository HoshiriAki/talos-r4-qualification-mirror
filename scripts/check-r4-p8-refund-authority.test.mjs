#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectRefundAuthoritySnapshot,
  validateRefundAuthoritySnapshot,
} from './check-r4-p8-refund-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectRefundAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateRefundAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateRefundAuthoritySnapshot(collectRefundAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Refund factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'RefundCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureRefund)',
      'Refund factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Refund descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("refund", Business, ModuleActivation::Always, NONE, Refund)',
      'descriptor!("refund", Business, ModuleActivation::Always, SQLITE, Refund)',
      'Refund descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Refund request loses serialization',
  snapshot => {
    snapshot.pgRequest = replaceRequired(
      snapshot.pgRequest,
      'pg_write_serializable_repository',
      'pg_write',
      'Refund request loses serialization',
    )
  },
  'request invariant missing',
)

expectFailure(
  'Refund transition loses row lock',
  snapshot => {
    snapshot.pgTransition = replaceRequired(
      snapshot.pgTransition,
      'FOR UPDATE',
      'NO_ROW_LOCK',
      'Refund transition loses row lock',
    )
  },
  'transition invariant missing',
)

expectFailure(
  'Refund execute loses ledger write',
  snapshot => {
    snapshot.pgExecute = replaceRequired(
      snapshot.pgExecute,
      'INSERT INTO deposit_ledger',
      'INSERT INTO unrelated_ledger',
      'Refund execute loses ledger write',
    )
  },
  'execute invariant missing',
)

expectFailure(
  'Refund live proof loses cross tenant rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'Err(RefundMutationError::NotFound)',
      'Err(RefundMutationError::DepositNotFound)',
      'Refund live proof loses cross tenant rejection',
    )
  },
  'NotFound',
)

console.log('R4-P8 Refund authority mutation tests passed.')
