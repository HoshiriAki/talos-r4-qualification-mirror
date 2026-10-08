#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  repositories: 'backend/src/repositories/mod.rs',
  sqliteRepository: 'backend/src/repositories/maintenance_compatibility.rs',
  dispatch: 'backend/src/repositories/maintenance_compatibility_dispatch.rs',
  postgresRepository: 'backend/src/repositories/maintenance_compatibility_postgres.rs',
  workers: 'backend/src/application/workers.rs',
  service: 'backend/src/services/order_compatibility_support.rs',
  main: 'backend/src/main.rs',
  liveQualification:
    'backend/src/repositories/postgres/maintenance_compatibility_qualification_tests.rs',
  exactHead: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP8MaintenanceSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8MaintenanceSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'mod maintenance_compatibility;',
    'mod maintenance_compatibility_dispatch;',
    'mod maintenance_compatibility_postgres;',
    'MaintenanceCompatibilityRepository',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push(`Maintenance compatibility repository wiring missing: ${token}`)
    }
  }

  for (const token of [
    'SqliteMaintenanceCompatibilityRepository',
    'sync_order_statuses(',
    'seed_overdue_tasks(',
    'INSERT OR IGNORE INTO work_tasks',
    'o.tenant_id=od.tenant_id',
    'tenant_id)',
  ]) {
    if (!snapshot.sqliteRepository.includes(token)) {
      errors.push(`SQLite maintenance compatibility invariant missing: ${token}`)
    }
  }

  for (const token of [
    'pub(crate) struct MaintenanceCompatibilityRepository',
    'MaintenanceCompatibilityBackend::Sqlite',
    'MaintenanceCompatibilityBackend::Postgres',
    'pub(crate) fn postgres(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Maintenance compatibility dispatch missing: ${token}`)
    }
  }

  for (const token of [
    'PostgresMaintenanceCompatibilityRepository',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'ON CONFLICT (id) DO NOTHING',
    'o.tenant_id=od.tenant_id',
    'CAST($1 AS date) - $2::integer',
    'SYS_MAINTENANCE_RUNTIME',
  ]) {
    if (!snapshot.postgresRepository.includes(token)) {
      errors.push(`PostgreSQL maintenance compatibility invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresRepository)) {
    errors.push('PostgreSQL maintenance compatibility repository must not depend on SQLite types')
  }

  for (const token of [
    'maintenance_repository: MaintenanceCompatibilityRepository',
    'new_with_repositories(',
    '.maintenance_repository',
    '.sync_order_statuses(&today)',
    '.seed_overdue_tasks(&today, &now)',
  ]) {
    if (!snapshot.workers.includes(token)) {
      errors.push(`Maintenance worker composition missing: ${token}`)
    }
  }
  const adapterStart = snapshot.workers.indexOf('pub struct LegacyMaintenanceAdapter')
  const adapterStructEnd = snapshot.workers.indexOf(
    '\n}\n\nimpl LegacyMaintenanceAdapter',
    adapterStart,
  )
  const adapterEnd = snapshot.workers.indexOf('pub trait WorkerRunner', adapterStart)
  const adapterStruct =
    adapterStart >= 0 && adapterStructEnd > adapterStart
      ? snapshot.workers.slice(adapterStart, adapterStructEnd)
      : ''
  const adapterSlice =
    adapterStart >= 0 && adapterEnd > adapterStart
      ? snapshot.workers.slice(adapterStart, adapterEnd)
      : ''
  if (adapterStruct.includes('pool: Pool<SqliteConnectionManager>')) {
    errors.push('LegacyMaintenanceAdapter must not own a SQLite persistence pool')
  }
  for (const forbidden of [
    'order_compatibility_support::sync_order_statuses(&self.pool)',
    'order_compatibility_support::seed_overdue_tasks(&self.pool)',
  ]) {
    if (adapterSlice.includes(forbidden)) {
      errors.push(`Maintenance worker bypasses repository authority: ${forbidden}`)
    }
  }

  for (const token of [
    'MaintenanceCompatibilityRepository::new(pool.clone())',
    '.sync_order_statuses(&today)?',
    '.seed_overdue_tasks(&today, &now)',
  ]) {
    if (!snapshot.service.includes(token)) {
      errors.push(`Legacy maintenance service delegation missing: ${token}`)
    }
  }
  for (const forbidden of [
    '"UPDATE orders SET status = \'reserved\'',
    '"INSERT INTO work_tasks (id,kind,status,risk',
  ]) {
    if (snapshot.service.includes(forbidden)) {
      errors.push(`Legacy maintenance service still owns persistence SQL: ${forbidden}`)
    }
  }

  for (const token of [
    'MaintenanceCompatibilityRepository::postgres(pg)',
    'LegacyMaintenanceAdapter::new_with_repositories(',
    'maintenance_compatibility_repository',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Maintenance production composition missing: ${token}`)
    }
  }

  const localMaintenanceBindingPattern =
    /MaintenanceCompatibilityRepository::new\(\s*require_sqlite_pool\(\s*"maintenance compatibility"\s*,?\s*\)\?\s*,?\s*\)/g
  const localMaintenanceBindingCount =
    snapshot.main.match(localMaintenanceBindingPattern)?.length ?? 0
  if (localMaintenanceBindingCount !== 2) {
    errors.push(
      'Maintenance local fallback must bind the explicit SQLite capability directly in exactly both compile paths',
    )
  }
  if (snapshot.main.includes('MaintenanceCompatibilityRepository::new(pool.clone())')) {
    errors.push('Maintenance composition restored implicit shared SQLite pool fallback')
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Maintenance production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Maintenance cutover must not restore the retired transitional production barrier')
  }

  for (const token of [
    'live_pg18_maintenance_compatibility_preserves_status_sync_overdue_tenant_and_idempotency',
    'assert_eq!(synced.reserved, 1);',
    'assert_eq!(synced.activated, 1);',
    'assert_eq!(row_a.get::<String, _>(1), "tenant-a");',
    'assert_eq!(row_b.get::<String, _>(1), "tenant-b");',
    'assert_eq!(row_a.get::<String, _>(2), "high");',
    'assert_eq!(row_b.get::<String, _>(2), "medium");',
    'let recomposed = MaintenanceCompatibilityRepository::postgres',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Maintenance PostgreSQL live proof missing: ${token}`)
    }
  }
  if (!snapshot.exactHead.includes(
    'live_pg18_maintenance_compatibility_preserves_status_sync_overdue_tenant_and_idempotency',
  )) {
    errors.push('Exact-head qualification does not execute maintenance PostgreSQL live proof')
  }

  return errors
}

function main() {
  const errors = validateP8MaintenanceSnapshot(collectP8MaintenanceSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Maintenance compatibility cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Maintenance compatibility PostgreSQL cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
