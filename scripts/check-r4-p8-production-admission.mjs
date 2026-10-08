#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  main: 'backend/src/main.rs',
  startupGate: 'scripts/check-r4-p8-startup-sqlite-optional.mjs',
  persistenceGate: 'scripts/check-r4-p8-production-persistence.mjs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectProductionAdmissionSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateProductionAdmissionSnapshot(snapshot) {
  const errors = []
  const main = compact(snapshot.main)

  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Transitional PostgreSQL runtime barrier is still present')
  }

  for (const token of [
    'letpg_pool:Option<sqlx::postgres::PgPool>=ifdatabase_profile==DatabaseProfile::Postgres18',
    'db::run_all_pg_migrations(&pool).await',
    'ifconfig.is_production&&pg_pool.is_none(){',
    'R4-P5productionwebhookEventLanerequiresPostgreSQL18durabledriver',
    'letsqlite_pool=ifdatabase_profile==DatabaseProfile::SqliteLocal{',
    'Some(create_pool(&config.db_path)?)',
    '}else{None};',
    "letrequire_sqlite_pool=|consumer:&'staticstr|",
    'CFG_SQLITE_CAPABILITY_REQUIRED',
    'Some(pg)=>(',
    'Arc::new(PostgresRepositoryProvider::new_with_metrics(',
    'WorkflowWorkerTenantSource::postgres(pg)',
  ]) {
    if (!main.includes(token)) {
      errors.push('Production admission invariant missing: ' + token)
    }
  }

  const createCalls = snapshot.main.match(/create_pool\(&config\.db_path\)\?/g) ?? []
  if (createCalls.length !== 1) {
    errors.push('Production admission must retain exactly one local SQLite pool creation site')
  }

  if (snapshot.main.includes('pg_pool.is_none().then(||')) {
    errors.push('Production admission must not infer SQLite fallback from PostgreSQL absence')
  }

  if (!snapshot.startupGate.includes(
    'Startup must not retain the transitional PostgreSQL admission barrier',
  )) {
    errors.push('Startup gate has not been inverted for production admission')
  }
  if (!snapshot.persistenceGate.includes(
    'transitional PostgreSQL admission barrier must be removed after P8-D closure',
  )) {
    errors.push('Persistence gate has not been inverted for production admission')
  }

  for (const token of [
    'node scripts/check-r4-p8-production-admission.test.mjs',
    'node scripts/check-r4-p8-production-admission.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head production admission qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateProductionAdmissionSnapshot(collectProductionAdmissionSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 production admission gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 production admission gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
