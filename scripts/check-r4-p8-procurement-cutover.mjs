#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  module: 'backend/src/application/procurement_compatibility.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/procurement.rs',
  dispatch: 'backend/src/repositories/procurement_dispatch.rs',
  postgres: 'backend/src/repositories/procurement_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  sqliteMigration: 'backend/src/db/migrations/074_r4_tenant_scoped_asset_purchases.sql',
  pgMigration: 'backend/src/db/migrations/postgres/074_r4_tenant_scoped_asset_purchases.sql',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/procurement_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectProcurementCutoverSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateProcurementCutoverSnapshot(snapshot) {
  const errors = []
  const descriptors = compact(snapshot.descriptors)
  const module = compact(snapshot.module)

  for (const token of [
    'ProcurementCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.procurements()',
    'FeatureProcurement::new().metadata()',
    'FeatureProcurement::new().commands()',
    'FeatureProcurement::new().schema()',
  ]) {
    if (!module.includes(token)) {
      errors.push('procurement compatibility authority missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn procurements(&self) -> ScopedProcurementRepository<'_>")) {
    errors.push('ScopedRepositories must expose procurement authority')
  }

  if (!snapshot.dispatch.includes('PostgresProcurementRepository')
      || !snapshot.dispatch.includes('SqliteProcurementRepository')) {
    errors.push('procurement backend dispatch is incomplete')
  }

  for (const token of [
    'WHERE tenant_id=?1 AND device_serial_no=?2',
    'INSERT INTO asset_purchases',
    'WHERE tenant_id=?2 AND device_serial_no=?3',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite procurement scope missing: ' + token)
    }
  }

  for (const token of [
    'WHERE tenant_id=$1 AND device_serial_no=$2',
    'INSERT INTO asset_purchases',
    'WHERE tenant_id=$2 AND device_serial_no=$3',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL procurement scope missing: ' + token)
    }
  }

  if (snapshot.factory.includes('with_pool!(FeatureProcurement)')) {
    errors.push('Registry procurement restored SQLite runtime construction')
  }
  if (!snapshot.factory.includes('ProcurementCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry procurement provider composition missing')
  }

  if (!descriptors.includes('descriptor!("procurement",Business,ModuleActivation::Always,NONE,Procurement)')) {
    errors.push('procurement Registry descriptor must not require SQLite')
  }

  for (const token of [
    'tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT',
    'UNIQUE(tenant_id, device_serial_no)',
  ]) {
    if (!snapshot.sqliteMigration.includes(token)) {
      errors.push('SQLite 074 procurement identity missing: ' + token)
    }
  }

  for (const token of [
    'DROP CONSTRAINT IF EXISTS asset_purchases_device_serial_no_key',
    'ALTER COLUMN tenant_id SET NOT NULL',
    'fk_asset_purchases_tenant',
    'idx_asset_purchases_tenant_serial_unique',
  ]) {
    if (!snapshot.pgMigration.includes(token)) {
      errors.push('PostgreSQL 074 procurement identity missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod procurement_qualification_tests;')) {
    errors.push('PostgreSQL procurement qualification module is not registered')
  }

  for (const token of [
    'live_pg18_procurement_authority_preserves_scope_identity_and_recomposition',
    'assert_eq!(created_a.device_serial_no, created_b.device_serial_no)',
    'Err(ProcurementMutationError::Duplicate)',
    'scoped_b.procurements().get("PROC-A-ONLY")?.is_none()',
    'procurement record survives provider recomposition',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 procurement authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-procurement-cutover.test.mjs',
    'node scripts/check-r4-p8-procurement-cutover.mjs',
    'live_pg18_procurement_authority_preserves_scope_identity_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-AE procurement qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateProcurementCutoverSnapshot(collectProcurementCutoverSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 procurement cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 procurement cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
