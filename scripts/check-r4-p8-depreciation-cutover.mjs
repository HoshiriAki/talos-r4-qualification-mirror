#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  module: 'backend/src/application/depreciation_compatibility.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/depreciation.rs',
  dispatch: 'backend/src/repositories/depreciation_dispatch.rs',
  postgres: 'backend/src/repositories/depreciation_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  sqliteMigration: 'backend/src/db/migrations/075_r4_depreciation_tenant_invariant.sql',
  pgMigration: 'backend/src/db/migrations/postgres/075_r4_depreciation_tenant_invariant.sql',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/depreciation_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectDepreciationCutoverSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDepreciationCutoverSnapshot(snapshot) {
  const errors = []
  const module = compact(snapshot.module)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'DepreciationCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.depreciations()',
    'FeatureDepreciation::new().metadata()',
    'FeatureDepreciation::new().commands()',
    'FeatureDepreciation::new().schema()',
    'BIZ_DEPRECIATION_ALREADY_RUN',
    'BIZ_ASSET_NOT_FOUND',
  ]) {
    if (!module.includes(token)) {
      errors.push('depreciation compatibility authority missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn depreciations(&self) -> ScopedDepreciationRepository<'_>")) {
    errors.push('ScopedRepositories must expose depreciation authority')
  }

  if (!snapshot.dispatch.includes('PostgresDepreciationRepository')
      || !snapshot.dispatch.includes('SqliteDepreciationRepository')) {
    errors.push('depreciation backend dispatch is incomplete')
  }

  for (const token of [
    'WHERE ap.tenant_id=?1 AND ap.device_serial_no=?2',
    'WHERE tenant_id=?1 AND device_serial_no=?2',
    'WHERE tenant_id=?1 AND period=?2',
    'FROM asset_purchases',
    'INSERT INTO depreciation_log',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite depreciation scope missing: ' + token)
    }
  }

  for (const token of [
    'WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2',
    'WHERE tenant_id=$1 AND device_serial_no=$2',
    'WHERE tenant_id=$1 AND period=$2',
    'purchase_price::double precision',
    'SUM(depreciation_amount)::double precision',
    'pg_write_serializable_repository',
    'INSERT INTO depreciation_log',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL depreciation authority missing: ' + token)
    }
  }

  if (snapshot.factory.includes('with_pool!(FeatureDepreciation)')) {
    errors.push('Registry depreciation restored SQLite runtime construction')
  }
  if (!snapshot.factory.includes('DepreciationCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry depreciation provider composition missing')
  }

  if (!descriptors.includes('descriptor!("depreciation",Business,ModuleActivation::Always,NONE,Depreciation)')) {
    errors.push('depreciation Registry descriptor must not require SQLite')
  }

  for (const token of [
    'depreciation_log_tenant_id_not_null',
    'fk_depreciation_log_tenant_id_insert',
    'idx_depreciation_log_tenant_period',
  ]) {
    if (!snapshot.sqliteMigration.includes(token)) {
      errors.push('SQLite 075 depreciation tenant invariant missing: ' + token)
    }
  }

  for (const token of [
    'ALTER COLUMN tenant_id SET NOT NULL',
    'fk_depreciation_log_tenant',
    'idx_depreciation_log_tenant_period',
  ]) {
    if (!snapshot.pgMigration.includes(token)) {
      errors.push('PostgreSQL 075 depreciation tenant invariant missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod depreciation_qualification_tests;')) {
    errors.push('PostgreSQL depreciation qualification module is not registered')
  }

  for (const token of [
    'live_pg18_depreciation_authority_preserves_scope_monthly_idempotence_and_recomposition',
    'assert_eq!(initial_a.purchase_price, 360.0)',
    'assert_eq!(initial_b.purchase_price, 720.0)',
    'Err(DepreciationMutationError::AlreadyRun(2))',
    'scoped_b.depreciations().logs("DEP-A-ONLY")?.is_empty()',
    'depreciation survives provider recomposition',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 depreciation authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-depreciation-cutover.test.mjs',
    'node scripts/check-r4-p8-depreciation-cutover.mjs',
    'live_pg18_depreciation_authority_preserves_scope_monthly_idempotence_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-AF depreciation qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDepreciationCutoverSnapshot(collectDepreciationCutoverSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 depreciation cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 depreciation cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
