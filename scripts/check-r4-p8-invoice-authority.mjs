#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  application: 'backend/src/application/invoice_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/invoice.rs',
  dispatch: 'backend/src/repositories/invoice_dispatch.rs',
  pgMutation: 'backend/src/repositories/invoice_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/invoice_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  dbMod: 'backend/src/db/mod.rs',
  r4Migrations: 'backend/src/db/r4_migrations.rs',
  sqliteMigration: 'backend/src/db/migrations/076_r4_tenant_scoped_invoice_numbers.sql',
  pgMigration: 'backend/src/db/migrations/postgres/076_r4_tenant_scoped_invoice_numbers.sql',
  pgTaxMigration: 'backend/src/db/migrations/postgres/077_r4_tenant_scoped_tax_config.sql',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/invoice_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectInvoiceAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateInvoiceAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)
  const sqliteMigration = compact(snapshot.sqliteMigration)
  const pgMigration = compact(snapshot.pgMigration)
  const pgTaxMigration = compact(snapshot.pgTaxMigration)

  for (const token of [
    'InvoiceCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.invoices()',
    '.issue(',
    '.void(',
    '.red_flush(',
    '.get_by_id(',
    '.get_by_order(',
    '.list(',
    'FeatureInvoice::new().metadata()',
    'FeatureInvoice::new().commands()',
    'FeatureInvoice::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Invoice compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod invoice;',
    'mod invoice_dispatch;',
    'mod invoice_postgres;',
    'mod invoice_postgres_mutation;',
    'mod invoice_postgres_read;',
    'pub use invoice_dispatch::ScopedInvoiceRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Invoice repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn invoices(&self) -> ScopedInvoiceRepository<'_>")) {
    errors.push('ScopedRepositories must expose Invoice authority')
  }
  if (!snapshot.dispatch.includes('PostgresInvoiceRepository')
      || !snapshot.dispatch.includes('SqliteInvoiceRepository')) {
    errors.push('Invoice backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    "WHERE tenant_id=?1 AND tax_type='vat' AND is_active=1",
    'WHERE tenant_id=?1 AND invoice_no LIKE ?2',
    'INSERT INTO invoices',
    'INSERT INTO revenue_records',
    'INSERT INTO accounting_entries',
    'sqlite_invoice_authority_preserves_tenant_numbering_and_atomic_accounting',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Invoice authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'ROUND(rate::numeric, 8)::double precision',
    'ROUND(tax_rate::numeric, 8)::double precision AS tax_rate',
    "WHERE tenant_id=$1 AND tax_type='vat' AND is_active=1",
    'WHERE tenant_id=$1 AND invoice_no LIKE $2',
    'INSERT INTO invoices',
    'INSERT INTO revenue_records',
    'INSERT INTO accounting_entries',
    'FOR UPDATE',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Invoice mutation invariant missing: ' + token)
    }
  }

  if (!snapshot.pgRead.includes('QueryBuilder::<Postgres>')
      || !snapshot.pgRead.includes('tenant_id=')
      || !snapshot.pgRead.includes('amount::double precision')
      || !snapshot.pgRead.includes('ROUND(tax_rate::numeric, 8)::double precision AS tax_rate')
      || !snapshot.pgRead.includes('tax_amount::double precision')) {
    errors.push('PostgreSQL Invoice reads must remain tenant-scoped and type-normalized')
  }

  if (!snapshot.dbMod.includes('run_sqlite_extension_076(conn)?')
      || !snapshot.dbMod.includes('run_pg_extension_076(pool).await?')
      || !snapshot.dbMod.includes('run_pg_extension_077(pool).await?')) {
    errors.push('Invoice tenant-number and PostgreSQL tax-scope migrations must be in production composition')
  }
  for (const token of [
    'MIGRATION_076_ID',
    'run_sqlite_extension_076',
    'run_pg_extension_076',
    'MIGRATION_077_ID',
    'run_pg_extension_077',
    'sqlite_invoice_number_scope_is_tenant_local',
  ]) {
    if (!snapshot.r4Migrations.includes(token)) {
      errors.push('Invoice migration registration/evidence missing: ' + token)
    }
  }
  if (!sqliteMigration.includes('UNIQUE(tenant_id,invoice_no)')
      || sqliteMigration.includes('invoice_noTEXTNOTNULLUNIQUE')) {
    errors.push('SQLite invoice number uniqueness must be tenant-scoped')
  }
  if (!pgMigration.includes('DROPCONSTRAINTIFEXISTSinvoices_invoice_no_key')
      || !pgMigration.includes('ONinvoices(tenant_id,invoice_no)')) {
    errors.push('PostgreSQL invoice number uniqueness migration is incomplete')
  }
  if (!pgTaxMigration.includes('ADDCOLUMNIFNOTEXISTStenant_idTEXT')
      || !pgTaxMigration.includes('ALTERCOLUMNtenant_idSETNOTNULL')
      || !pgTaxMigration.includes('ONtax_config(tenant_id,tax_type,is_active,effective_from)')) {
    errors.push('PostgreSQL tax_config tenant-scope repair migration is incomplete')
  }

  if (!factory.includes('InvoiceCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Invoice provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureInvoice)')) {
    errors.push('Registry Invoice restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("invoice",Business,ModuleActivation::Always,NONE,Invoice)')) {
    errors.push('Invoice Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod invoice_qualification_tests;')) {
    errors.push('PostgreSQL Invoice qualification module is not registered')
  }
  for (const token of [
    'live_pg18_invoice_authority_preserves_tenant_numbering_atomic_accounting_and_recomposition',
    'a1.invoice_no, "INV-20260929-0001"',
    'b1.invoice_no, "INV-20260929-0001"',
    'Err(InvoiceMutationError::NotIssued(status)) if status == "voided"',
    'migration_applied',
    'tax_scope_migration_applied',
    'revenue_sum, 50.0',
    'accounting_count, 7',
    'next.invoice_no, "INV-20260929-0004"',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Invoice authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-invoice-authority.test.mjs',
    'node scripts/check-r4-p8-invoice-authority.mjs',
    'live_pg18_invoice_authority_preserves_tenant_numbering_atomic_accounting_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Invoice qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateInvoiceAuthoritySnapshot(collectInvoiceAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Invoice authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Invoice authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
