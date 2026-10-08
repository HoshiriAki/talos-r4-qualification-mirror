#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectInvoiceAuthoritySnapshot,
  validateInvoiceAuthoritySnapshot,
} from './check-r4-p8-invoice-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectInvoiceAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateInvoiceAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateInvoiceAuthoritySnapshot(collectInvoiceAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Invoice factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'InvoiceCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureInvoice)',
      'Invoice factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Invoice descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("invoice", Business, ModuleActivation::Always, NONE, Invoice)',
      'descriptor!("invoice", Business, ModuleActivation::Always, SQLITE, Invoice)',
      'Invoice descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Invoice PostgreSQL numbering loses advisory lock',
  snapshot => {
    snapshot.pgMutation = replaceRequired(
      snapshot.pgMutation,
      'pg_advisory_xact_lock(hashtextextended($1,0))',
      'invoice_number_lock_removed($1)',
      'Invoice PostgreSQL numbering loses advisory lock',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Invoice PostgreSQL tax rate loses precision normalization',
  snapshot => {
    snapshot.pgMutation = replaceRequired(
      snapshot.pgMutation,
      'ROUND(rate::numeric, 8)::double precision',
      'rate::double precision',
      'Invoice PostgreSQL tax rate loses precision normalization',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Invoice PostgreSQL numbering loses tenant scope',
  snapshot => {
    snapshot.pgMutation = replaceRequired(
      snapshot.pgMutation,
      'WHERE tenant_id=$1 AND invoice_no LIKE $2',
      'WHERE invoice_no LIKE $2',
      'Invoice PostgreSQL numbering loses tenant scope',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Invoice accounting side effect is removed',
  snapshot => {
    snapshot.pgMutation = snapshot.pgMutation.replaceAll(
      'INSERT INTO accounting_entries',
      'INSERT INTO removed_accounting_entries',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Invoice SQLite migration restores global uniqueness',
  snapshot => {
    snapshot.sqliteMigration = replaceRequired(
      snapshot.sqliteMigration,
      'UNIQUE(tenant_id, invoice_no)',
      'UNIQUE(invoice_no)',
      'Invoice SQLite migration restores global uniqueness',
    )
  },
  'tenant-scoped',
)

expectFailure(
  'Invoice PostgreSQL migration drops tenant composite unique',
  snapshot => {
    snapshot.pgMigration = replaceRequired(
      snapshot.pgMigration,
      'ON invoices(tenant_id, invoice_no)',
      'ON invoices(invoice_no)',
      'Invoice PostgreSQL migration drops tenant composite unique',
    )
  },
  'uniqueness migration',
)

expectFailure(
  'Invoice PostgreSQL tax scope migration drops tenant ownership',
  snapshot => {
    snapshot.pgTaxMigration = replaceRequired(
      snapshot.pgTaxMigration,
      'ALTER TABLE tax_config\n    ALTER COLUMN tenant_id SET NOT NULL;',
      'ALTER TABLE tax_config\n    ALTER COLUMN tenant_id DROP NOT NULL;',
      'Invoice PostgreSQL tax scope migration drops tenant ownership',
    )
  },
  'tax_config tenant-scope repair migration',
)

expectFailure(
  'Invoice reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id='), 'tenant mutation anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
  },
  'tenant-scoped',
)

expectFailure(
  'Invoice live proof loses cross-tenant same number',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'b1.invoice_no, "INV-20260929-0001"',
      'b1.invoice_no, "INV-20260929-9999"',
      'Invoice live proof loses cross-tenant same number',
    )
  },
  'b1.invoice_no',
)

console.log('R4-P8 Invoice authority mutation tests passed.')
