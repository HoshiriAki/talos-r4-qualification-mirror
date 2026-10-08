#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  official: 'backend/system/admin/src/contract.rs',
  route: 'backend/src/routes/contract.rs',
  application: 'backend/src/application/contract_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/contract.rs',
  dispatch: 'backend/src/repositories/contract_dispatch.rs',
  pgMutation: 'backend/src/repositories/contract_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/contract_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  migrationSqlite: 'backend/src/db/migrations/080_r4_contract_tenant_invariant.sql',
  migrationPg: 'backend/src/db/migrations/postgres/080_r4_contract_tenant_invariant.sql',
  migrations: 'backend/src/db/r4_migrations.rs',
  db: 'backend/src/db/mod.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/contract_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectContractAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateContractAuthoritySnapshot(snapshot) {
  const errors = []
  const official = compact(snapshot.official)
  const route = compact(snapshot.route)
  const app = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)
  const migrationSqlite = compact(snapshot.migrationSqlite)
  const migrationPg = compact(snapshot.migrationPg)

  if (!official.includes('puborder_id:String')
      || !official.includes('puborder_id:Option<String>')) {
    errors.push('Contract command model must use canonical text order identity')
  }
  if (!route.includes('order_id:String')
      || !route.includes('order_id:Option<String>')) {
    errors.push('Contract HTTP boundary must use canonical text order identity')
  }

  for (const token of [
    'ContractCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.contracts()',
    '.template_create(',
    '.template_update(',
    '.template_list(',
    '.template_get(',
    '.generate(',
    '.sign(',
    '.verify(',
    '.list(',
    '.get(',
    'Unvalidated<TemplateCreateInput>',
    'Unvalidated<ContractGenerateInput>',
    'FeatureContract::new().metadata()',
    'FeatureContract::new().commands()',
    'FeatureContract::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Contract compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod contract;',
    'mod contract_dispatch;',
    'mod contract_postgres;',
    'mod contract_postgres_mutation;',
    'mod contract_postgres_read;',
    'pub use contract_dispatch::ScopedContractRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Contract repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn contracts(&self) -> ScopedContractRepository<'_>")) {
    errors.push('ScopedRepositories must expose Contract authority')
  }
  if (!snapshot.dispatch.includes('PostgresContractRepository')
      || !snapshot.dispatch.includes('SqliteContractRepository')) {
    errors.push('Contract backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM orders WHERE tenant_id=?1 AND id=?2',
    'INSERT INTO e_signatures',
    "SET status='signed'",
    'SELECT COUNT(*) FROM e_signatures',
    "SET status='verified'",
    'sqlite_contract_authority_preserves_scope_identity_and_signature_lifecycle',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Contract authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'FOR UPDATE',
    'FOR KEY SHARE',
    'INSERT INTO e_signatures',
    "SET status='signed'",
    'SELECT COUNT(*)::bigint FROM e_signatures',
    "SET status='verified'",
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Contract mutation invariant missing: ' + token)
    }
  }

  if (!snapshot.pgRead.includes('QueryBuilder::<Postgres>')
      || !snapshot.pgRead.includes('tenant_id=')
      || !snapshot.pgRead.includes('device_value::double precision')
      || !snapshot.pgRead.includes('(is_active <> 0) AS is_active')) {
    errors.push('PostgreSQL Contract reads must remain tenant-scoped and type-normalized')
  }

  for (const token of [
    'order_id TEXT NOT NULL',
    'updated_at TEXT NOT NULL',
    'FOREIGN KEY(order_id, tenant_id)',
    'REFERENCES orders(id, tenant_id)',
    'FOREIGN KEY(template_id, tenant_id)',
    'FOREIGN KEY(contract_id, tenant_id)',
  ]) {
    if (!migrationSqlite.includes(compact(token))) {
      errors.push('SQLite Contract migration invariant missing: ' + token)
    }
  }
  for (const token of [
    'ADD COLUMN IF NOT EXISTS updated_at TEXT',
    "pg_get_serial_sequence('contract_templates', 'id')",
    'FOREIGN KEY (order_id, tenant_id)',
    'REFERENCES orders(id, tenant_id)',
    'FOREIGN KEY (template_id, tenant_id)',
    'FOREIGN KEY (contract_id, tenant_id)',
  ]) {
    if (!migrationPg.includes(compact(token))) {
      errors.push('PostgreSQL Contract migration invariant missing: ' + token)
    }
  }
  if (!snapshot.migrations.includes('080_r4_contract_tenant_invariant')
      || !snapshot.db.includes('run_sqlite_extension_080')
      || !snapshot.db.includes('run_pg_extension_080')) {
    errors.push('Contract migration 080 is not registered on both database profiles')
  }

  if (!factory.includes('ContractCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Contract provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureContract)')) {
    errors.push('Registry Contract restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("contract",Business,ModuleActivation::Always,NONE,Contract)')) {
    errors.push('Contract Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod contract_qualification_tests;')) {
    errors.push('PostgreSQL Contract qualification module is not registered')
  }
  for (const token of [
    'live_pg18_contract_authority_preserves_scope_order_identity_signature_lifecycle_and_recomposition',
    'Err(ContractMutationError::ContractNotFound)',
    'Err(ContractMutationError::OrderNotFound)',
    'persisted.status, "verified"',
    'persisted.signatures.len(), 1',
    'persisted.items[0].order_id, "contract-order-alpha"',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Contract authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-contract-authority.test.mjs',
    'node scripts/check-r4-p8-contract-authority.mjs',
    'live_pg18_contract_authority_preserves_scope_order_identity_signature_lifecycle_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Contract qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateContractAuthoritySnapshot(collectContractAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Contract authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Contract authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
