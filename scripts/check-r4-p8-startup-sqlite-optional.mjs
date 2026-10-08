#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  main: 'backend/src/main.rs',
  config: 'backend/src/config.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectStartupSqliteOptionalSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateStartupSqliteOptionalSnapshot(snapshot) {
  const errors = []
  const main = compact(snapshot.main)
  const config = compact(snapshot.config)

  for (const token of [
    'enumDatabaseProfile{SqliteLocal,Postgres18,}',
    'ifis_production&&profile!=DatabaseProfile::Postgres18',
  ]) {
    if (!config.includes(token)) {
      errors.push('Database-profile admission invariant missing: ' + token)
    }
  }

  for (const token of [
    'letsqlite_pool=ifdatabase_profile==DatabaseProfile::SqliteLocal{',
    'Some(create_pool(&config.db_path)?)',
    '}else{None};',
    "letrequire_sqlite_pool=|consumer:&'staticstr|",
    'CFG_SQLITE_CAPABILITY_REQUIRED',
    'ifletSome(pool)=sqlite_pool.as_ref()',
    'letintegration_store=sqlite_pool.clone().map(|pool|',
    'ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(sqlite_pool.clone(),',
  ]) {
    if (!main.includes(token)) {
      errors.push('Startup optional-SQLite invariant missing: ' + token)
    }
  }

  if (snapshot.main.includes('let pool = create_pool(&config.db_path)?;')) {
    errors.push('Startup must not construct an ambient mandatory SQLite pool')
  }

  const createCalls = snapshot.main.match(/create_pool\(&config\.db_path\)\?/g) ?? []
  if (createCalls.length !== 1) {
    errors.push('Startup must have exactly one SQLite pool creation site inside the local profile branch')
  }

  if (snapshot.main.includes('pg_pool.is_none().then(||')) {
    errors.push('Startup must not infer SQLite capability indirectly from PostgreSQL pool absence')
  }

  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Startup must not retain the transitional PostgreSQL admission barrier')
  }

  for (const token of [
    'ifconfig.is_production&&pg_pool.is_none(){anyhow::bail!("R4-P5productionwebhookEventLanerequiresPostgreSQL18durabledriver");}',
    'None=>MaintenanceCompatibilityRepository::new(require_sqlite_pool(',
    'None=>{letpool=require_sqlite_pool("repositoryproviderandworkflowtenantsource")?;',
    'None=>TenantSimulationCompatibilityModule::with_pool(require_sqlite_pool(',
  ]) {
    if (!main.includes(token)) {
      errors.push('Startup fail-closed/local fallback invariant missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-startup-sqlite-optional.test.mjs',
    'node scripts/check-r4-p8-startup-sqlite-optional.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head startup optional-SQLite qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateStartupSqliteOptionalSnapshot(collectStartupSqliteOptionalSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 startup optional-SQLite gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 startup optional-SQLite gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
