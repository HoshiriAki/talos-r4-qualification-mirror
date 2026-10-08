#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectTaxAuthoritySnapshot,
  validateTaxAuthoritySnapshot,
} from './check-r4-p8-tax-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectTaxAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateTaxAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateTaxAuthoritySnapshot(collectTaxAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Tax factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'TaxCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureTax)',
      'Tax factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Tax descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tax", Business, ModuleActivation::Always, NONE, Tax)',
      'descriptor!("tax", Business, ModuleActivation::Always, SQLITE, Tax)',
      'Tax descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Tax upsert loses advisory lock',
  snapshot => {
    snapshot.pgMutation = replaceRequired(
      snapshot.pgMutation,
      'pg_advisory_xact_lock(hashtextextended($1,0))',
      'tax_lock_removed($1)',
      'Tax upsert loses advisory lock',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Tax upsert loses serializable transaction',
  snapshot => {
    snapshot.pgMutation = replaceRequired(
      snapshot.pgMutation,
      'pg_write_serializable_repository',
      'pg_write',
      'Tax upsert loses serializable transaction',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Tax reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id=$1'), 'Tax reads lose tenant scope: anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=$1', 'tenant_scope_removed=$1')
  },
  'read invariant missing',
)

expectFailure(
  'Tax reads lose precision normalization',
  snapshot => {
    assert.ok(
      snapshot.pgRead.includes('ROUND(rate::numeric, 8)::double precision'),
      'Tax reads lose precision normalization: anchor missing',
    )
    snapshot.pgRead = snapshot.pgRead.replaceAll(
      'ROUND(rate::numeric, 8)::double precision',
      'rate::double precision',
    )
  },
  'read invariant missing',
)

expectFailure(
  'Tax migration loses not-null ownership',
  snapshot => {
    snapshot.pgMigration077 = replaceRequired(
      snapshot.pgMigration077,
      'ALTER COLUMN tenant_id SET NOT NULL',
      'ALTER COLUMN tenant_id DROP NOT NULL',
      'Tax migration loses not-null ownership',
    )
  },
  'migration 077 is incomplete',
)

expectFailure(
  'Tax live proof loses cross-tenant invoice exclusion',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'row.invoice_no != "TAX-B"',
      'row.invoice_no != "TAX-A"',
      'Tax live proof loses cross-tenant invoice exclusion',
    )
  },
  'TAX-B',
)

console.log('R4-P8 Tax authority mutation tests passed.')
