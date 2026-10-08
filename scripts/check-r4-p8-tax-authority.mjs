#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/tax_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/tax.rs',
  dispatch: 'backend/src/repositories/tax_dispatch.rs',
  pgMutation: 'backend/src/repositories/tax_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/tax_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  pgMigration077: 'backend/src/db/migrations/postgres/077_r4_tenant_scoped_tax_config.sql',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/tax_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectTaxAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateTaxAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'TaxCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.taxes()',
    '.get_configs(',
    '.upsert_config(',
    '.active_rate(',
    '.export_snapshot(',
    'FeatureTax::new().metadata()',
    'FeatureTax::new().commands()',
    'FeatureTax::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Tax compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod tax;',
    'mod tax_dispatch;',
    'mod tax_postgres;',
    'mod tax_postgres_mutation;',
    'mod tax_postgres_read;',
    'pub use tax_dispatch::ScopedTaxRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Tax repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn taxes(&self) -> ScopedTaxRepository<'_>")) {
    errors.push('ScopedRepositories must expose Tax authority')
  }
  if (!snapshot.dispatch.includes('PostgresTaxRepository')
      || !snapshot.dispatch.includes('SqliteTaxRepository')) {
    errors.push('Tax backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'UPDATE tax_config',
    'tenant_id=?1',
    'INSERT INTO tax_config',
    'FROM invoices',
    'WHERE tenant_id=?1 AND issued_at LIKE ?2',
    'sqlite_tax_authority_preserves_scope_active_rate_and_export_snapshot',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Tax authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'UPDATE tax_config',
    'WHERE tenant_id=$1 AND tax_type=$2',
    'INSERT INTO tax_config',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Tax mutation invariant missing: ' + token)
    }
  }

  for (const token of [
    'ROUND(rate::numeric, 8)::double precision',
    'ROUND(tax_rate::numeric, 8)::double precision AS tax_rate',
    '(is_active <> 0) AS is_active',
    'WHERE tenant_id=$1 AND tax_type=$2',
    'FROM invoices',
    'WHERE tenant_id=$1 AND issued_at LIKE $2',
  ]) {
    if (!snapshot.pgRead.includes(token)) {
      errors.push('PostgreSQL Tax read invariant missing: ' + token)
    }
  }

  if (!snapshot.pgMigration077.includes('ADD COLUMN IF NOT EXISTS tenant_id TEXT')
      || !snapshot.pgMigration077.includes('ALTER COLUMN tenant_id SET NOT NULL')
      || !snapshot.pgMigration077.includes('idx_tax_config_tenant_active')) {
    errors.push('PostgreSQL Tax tenant-scope migration 077 is incomplete')
  }

  if (!factory.includes('TaxCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Tax provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureTax)')) {
    errors.push('Registry Tax restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("tax",Business,ModuleActivation::Always,NONE,Tax)')) {
    errors.push('Tax Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod tax_qualification_tests;')) {
    errors.push('PostgreSQL Tax qualification module is not registered')
  }
  for (const token of [
    'live_pg18_tax_authority_preserves_scope_active_rate_export_and_recomposition',
    'configs_a.iter().filter(|row| row.is_active).count(), 1',
    'scoped_a.taxes().active_rate("vat")?, Some(0.09)',
    'scoped_b.taxes().active_rate("vat")?, Some(0.13)',
    'export_a.invoices[0].invoice_no, "TAX-A"',
    'row.invoice_no != "TAX-B"',
    'active_count, 1',
    'persisted.configs[0].rate, 0.09',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Tax authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-tax-authority.test.mjs',
    'node scripts/check-r4-p8-tax-authority.mjs',
    'live_pg18_tax_authority_preserves_scope_active_rate_export_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Tax qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateTaxAuthoritySnapshot(collectTaxAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Tax authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Tax authority gate passed.')
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
