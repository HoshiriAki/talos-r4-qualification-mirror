#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/devices.rs',
  parser: 'backend/src/services/excel_import_service.rs',
  mutationAuthority: 'backend/src/application/device_mutation_authority.rs',
  sqlite: 'backend/src/repositories/device.rs',
  postgres: 'backend/src/repositories/device_postgres.rs',
  sqliteMigration: 'backend/src/db/migrations/073_r4_tenant_scoped_device_serials.sql',
  pgMigration: 'backend/src/db/migrations/postgres/073_r4_tenant_scoped_device_serials.sql',
  r4Migrations: 'backend/src/db/r4_migrations.rs',
  dbMod: 'backend/src/db/mod.rs',
  pgFresh: 'backend/src/db/p8_pg18_qualification.rs',
  pgTest: 'backend/src/repositories/postgres/device_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function count(source, token) {
  return source.split(token).length - 1
}

export function collectDeviceImportSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDeviceImportSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'state.pool',
    'excel_import_service::import_devices_from_excel',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('device import route retains SQLite persistence authority: ' + forbidden)
    }
  }
  for (const token of [
    'excel_import_service::parse_device_import_rows',
    '.device_mutations()',
    '.import_devices(&ctx, candidates, parsed.failed_rows)',
  ]) {
    if (!snapshot.route.includes(token)) {
      errors.push('device import application cutover missing: ' + token)
    }
  }

  for (const token of [
    'Persistence ownership intentionally stays outside this parser',
    'pub fn parse_device_import_rows(',
    'ParsedDeviceImportBatch',
  ]) {
    if (!snapshot.parser.includes(token)) {
      errors.push('device Excel parser boundary missing: ' + token)
    }
  }
  for (const forbidden of [
    'pub fn import_devices_from_excel(',
    'INSERT INTO devices',
  ]) {
    if (snapshot.parser.includes(forbidden)) {
      errors.push('device Excel parser regained persistence ownership: ' + forbidden)
    }
  }

  for (const token of [
    'pub fn import_devices(',
    'ImportedDeviceDraft',
    'Uuid::new_v4().to_string()',
    'status: txt::STATUS_CHECKED_IN.to_owned()',
    'match scoped.devices().import_device(&draft)',
    '"设备已存在"',
    '"设备写入失败"',
  ]) {
    if (!snapshot.mutationAuthority.includes(token)) {
      errors.push('device import authority missing: ' + token)
    }
  }

  for (const token of [
    'WHERE tenant_id=?1 AND serialNo=?2',
    'INSERT INTO devices',
    '(id,serialNo,rentalStatus,notes,createdAt,tenant_id)',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite device import tenant scope missing: ' + token)
    }
  }
  for (const token of [
    'WHERE tenant_id=$1 AND serialno=$2',
    'INSERT INTO devices',
    '(id,serialno,rentalstatus,notes,createdat,tenant_id)',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL device import tenant scope missing: ' + token)
    }
  }

  for (const token of [
    'UNIQUE(serialNo, tenant_id)',
    'FOREIGN KEY(serialNo, tenant_id)',
    'FOREIGN KEY(device_serial_no, tenant_id)',
    'REFERENCES devices_new_073(serialNo, tenant_id)',
    'CREATE TRIGGER devices_tenant_id_not_null',
    'CREATE TRIGGER devices_tenant_id_update_not_null',
    'CREATE TRIGGER fk_devices_tenant_id_insert',
    'CREATE TRIGGER fk_devices_tenant_id_update',
    'DROP TRIGGER IF EXISTS prevent_tenant_delete_with_data',
    'DROP TRIGGER IF EXISTS trg_allocation_device_model_insert',
    'CREATE TRIGGER prevent_tenant_delete_with_data',
    'CREATE TRIGGER trg_allocation_device_model_insert',
  ]) {
    if (!snapshot.sqliteMigration.includes(token)) {
      errors.push('SQLite 073 tenant-scoped serial migration missing: ' + token)
    }
  }
  if (count(snapshot.sqliteMigration, 'REFERENCES devices_new_073(serialNo, tenant_id)') !== 3) {
    errors.push('SQLite 073 must rebuild exactly three legacy serial child foreign keys')
  }

  for (const token of [
    'DROP CONSTRAINT IF EXISTS devices_serialno_key',
    'fk_order_devices_device_tenant',
    'fk_damage_reports_device_tenant',
    'fk_repair_orders_device_tenant',
    'REFERENCES devices(serialNo, tenant_id)',
    'ALTER TABLE order_devices ALTER COLUMN tenant_id SET NOT NULL',
    'ALTER TABLE damage_reports ALTER COLUMN tenant_id SET NOT NULL',
    'ALTER TABLE repair_orders ALTER COLUMN tenant_id SET NOT NULL',
  ]) {
    if (!snapshot.pgMigration.includes(token)) {
      errors.push('PostgreSQL 073 tenant-scoped serial migration missing: ' + token)
    }
  }
  if (count(snapshot.pgMigration, 'REFERENCES devices(serialNo, tenant_id)') !== 3) {
    errors.push('PostgreSQL 073 must install exactly three tenant-scoped legacy serial FKs')
  }

  for (const token of [
    'MIGRATION_073_ID: &str = "073_r4_tenant_scoped_device_serials"',
    'migrations/073_r4_tenant_scoped_device_serials.sql',
    'pragma_update(None, "foreign_keys", 0_i64)',
    '073 foreign_key_check failed',
  ]) {
    if (!snapshot.r4Migrations.includes(token)) {
      errors.push('R4 migration 073 executor evidence missing: ' + token)
    }
  }
  for (const token of [
    'run_sqlite_extension_073(conn)?',
    'run_pg_extension_073(pool).await?',
  ]) {
    if (!snapshot.dbMod.includes(token)) {
      errors.push('production migration composition missing 073: ' + token)
    }
  }
  for (const token of [
    '073_r4_tenant_scoped_device_serials',
    'SHARED-SERIAL',
    'legacy order-device serial FK must include tenant identity',
  ]) {
    if (!snapshot.pgFresh.includes(token)) {
      errors.push('fresh PG18 serial namespace proof missing: ' + token)
    }
  }

  if (count(snapshot.pgTest, 'serial_no: "SHAREDIMPORT001".into()') !== 3) {
    errors.push('PG18 device import proof must cover first insert, same-tenant duplicate, and cross-tenant same-serial insert')
  }
  for (const token of [
    'tenant A imported device is visible',
    'tenant B same-serial import is independently visible',
    'Some("imported by tenant A")',
    'Some("imported by tenant B")',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 device import tenant-namespace evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-device-import-cutover.test.mjs',
    'node scripts/check-r4-p8-device-import-cutover.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-Y qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDeviceImportSnapshot(collectDeviceImportSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 device import cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 device import cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
