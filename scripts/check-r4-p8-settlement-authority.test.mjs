#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectSettlementAuthoritySnapshot,
  validateSettlementAuthoritySnapshot,
} from './check-r4-p8-settlement-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectSettlementAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateSettlementAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateSettlementAuthoritySnapshot(collectSettlementAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Settlement factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'SettlementCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureSettlement)',
      'Settlement factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Settlement descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "settlement",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Settlement\n    )',
      'descriptor!(\n        "settlement",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Settlement\n    )',
      'Settlement descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Settlement generate loses advisory lock',
  snapshot => {
    snapshot.pgMutation = replaceRequired(
      snapshot.pgMutation,
      'pg_advisory_xact_lock(hashtextextended($1,0))',
      'settlement_lock_removed($1)',
      'Settlement generate loses advisory lock',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Settlement mutations lose serialization',
  snapshot => {
    assert.ok(
      snapshot.pgMutation.includes('pg_write_serializable_repository'),
      'Settlement mutations lose serialization: mutation anchor missing',
    )
    snapshot.pgMutation = snapshot.pgMutation.replaceAll(
      'pg_write_serializable_repository',
      'pg_write',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Settlement aggregation loses tenant scope',
  snapshot => {
    assert.ok(snapshot.pgMutation.includes('tenant_id=$1'), 'tenant mutation anchor missing')
    snapshot.pgMutation = snapshot.pgMutation.replaceAll('tenant_id=$1', 'tenant_scope_removed=$1')
  },
  'mutation invariant missing',
)

expectFailure(
  'Settlement confirmation loses row lock',
  snapshot => {
    snapshot.pgMutation = snapshot.pgMutation.replaceAll('FOR UPDATE', 'NO_ROW_LOCK')
  },
  'mutation invariant missing',
)

expectFailure(
  'Settlement reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id='), 'read tenant mutation anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
  },
  'tenant-scoped',
)

expectFailure(
  'Settlement live proof loses confirmation freeze',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'frozen.total_revenue, None',
      'frozen.total_revenue, Some(200.0)',
      'Settlement live proof loses confirmation freeze',
    )
  },
  'frozen.total_revenue',
)

console.log('R4-P8 Settlement authority mutation tests passed.')
