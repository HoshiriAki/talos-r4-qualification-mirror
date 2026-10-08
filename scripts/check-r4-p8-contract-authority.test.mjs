#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectContractAuthoritySnapshot,
  validateContractAuthoritySnapshot,
} from './check-r4-p8-contract-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectContractAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateContractAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateContractAuthoritySnapshot(collectContractAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Contract command model restores integer order ids',
  snapshot => {
    snapshot.official = snapshot.official
      .replace('pub order_id: String,', 'pub order_id: i64,')
      .replace('pub order_id: Option<String>,', 'pub order_id: Option<i64>,')
  },
  'canonical text order identity',
)

expectFailure(
  'Contract factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'ContractCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureContract)',
      'Contract factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Contract descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "contract",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Contract\n    )',
      'descriptor!(\n        "contract",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Contract\n    )',
      'Contract descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Contract SQLite migration loses tenant-order composite foreign key',
  snapshot => {
    snapshot.migrationSqlite = replaceRequired(
      snapshot.migrationSqlite,
      'FOREIGN KEY(order_id, tenant_id)',
      'FOREIGN KEY(order_id)',
      'Contract SQLite migration loses tenant-order composite foreign key',
    )
  },
  'migration invariant missing',
)

expectFailure(
  'Contract PostgreSQL migration loses seeded template sequence repair',
  snapshot => {
    snapshot.migrationPg = replaceRequired(
      snapshot.migrationPg,
      "pg_get_serial_sequence('contract_templates', 'id')",
      "pg_get_serial_sequence('removed_contract_templates', 'id')",
      'Contract PostgreSQL migration loses seeded template sequence repair',
    )
  },
  'migration invariant missing',
)

expectFailure(
  'Contract PostgreSQL mutation loses serialization',
  snapshot => {
    assert.ok(
      snapshot.pgMutation.includes('pg_write_serializable_repository'),
      'serialization mutation anchor missing',
    )
    snapshot.pgMutation = snapshot.pgMutation.replaceAll(
      'pg_write_serializable_repository',
      'pg_write',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Contract PostgreSQL mutation loses row locks',
  snapshot => {
    assert.ok(snapshot.pgMutation.includes('FOR UPDATE'), 'row-lock mutation anchor missing')
    snapshot.pgMutation = snapshot.pgMutation.replaceAll('FOR UPDATE', 'NO_ROW_LOCK')
  },
  'mutation invariant missing',
)

expectFailure(
  'Contract PostgreSQL reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id='), 'tenant-read mutation anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
  },
  'tenant-scoped',
)

expectFailure(
  'Contract live proof loses cross-tenant signing rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'Err(ContractMutationError::ContractNotFound)',
      'Err(ContractMutationError::OrderNotFound)',
      'Contract live proof loses cross-tenant signing rejection',
    )
  },
  'ContractNotFound',
)

console.log('R4-P8 Contract authority mutation tests passed.')
