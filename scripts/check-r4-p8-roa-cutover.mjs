#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  module: 'backend/src/application/roa_compatibility.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/roa.rs',
  dispatch: 'backend/src/repositories/roa_dispatch.rs',
  postgres: 'backend/src/repositories/roa_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/roa_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectRoaCutoverSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateRoaCutoverSnapshot(snapshot) {
  const errors = []
  const module = compact(snapshot.module)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'RoaCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.roa()',
    'FeatureRoa::new().metadata()',
    'FeatureRoa::new().commands()',
    'FeatureRoa::new().schema()',
    'BIZ_ASSET_NOT_FOUND',
    'BIZ_ASSET_ZERO_PRICE',
    'assessment(',
  ]) {
    if (!module.includes(token)) {
      errors.push('ROA compatibility authority missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn roa(&self) -> ScopedRoaRepository<'_>")) {
    errors.push('ScopedRepositories must expose ROA authority')
  }

  if (!snapshot.dispatch.includes('PostgresRoaRepository')
      || !snapshot.dispatch.includes('SqliteRoaRepository')) {
    errors.push('ROA backend dispatch is incomplete')
  }

  for (const token of [
    'WHERE ap.tenant_id=?1 AND ap.device_serial_no=?2',
    'SUM(opd.amount)',
    'MIN(opd.dateKey)',
    'od.orderId=opd.orderId',
    'od.tenant_id=ap.tenant_id',
    'od.serialNo=ap.device_serial_no',
    'o.tenant_id=ap.tenant_id',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite ROA authority missing canonical scoped revenue read: ' + token)
    }
  }

  for (const token of [
    'WHERE ap.tenant_id=$1 AND ap.device_serial_no=$2',
    'SUM(opd.amount)::double precision',
    'MIN(opd.datekey)',
    'od.orderid=opd.orderid',
    'od.tenant_id=ap.tenant_id',
    'od.serialno=ap.device_serial_no',
    'o.tenant_id=ap.tenant_id',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL ROA authority missing canonical scoped revenue read: ' + token)
    }
  }

  if (snapshot.factory.includes('with_pool!(FeatureRoa)')) {
    errors.push('Registry ROA restored SQLite runtime construction')
  }
  if (!snapshot.factory.includes('RoaCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry ROA provider composition missing')
  }

  if (!descriptors.includes('descriptor!("roa",Business,ModuleActivation::Always,NONE,Roa)')) {
    errors.push('ROA Registry descriptor must not require SQLite')
  }

  if (!snapshot.pgMod.includes('mod roa_qualification_tests;')) {
    errors.push('PostgreSQL ROA qualification module is not registered')
  }

  for (const token of [
    'live_pg18_roa_authority_preserves_scope_revenue_schema_and_recomposition',
    'assert_eq!(basis_a.total_revenue, 20.0)',
    'assert_eq!(basis_b.total_revenue, 900.0)',
    'scoped_a.roa().get("ROA-B-ONLY")?.is_none()',
    'ROA basis survives provider recomposition',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 ROA authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-roa-cutover.test.mjs',
    'node scripts/check-r4-p8-roa-cutover.mjs',
    'live_pg18_roa_authority_preserves_scope_revenue_schema_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head ROA qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateRoaCutoverSnapshot(collectRoaCutoverSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 ROA cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 ROA cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
