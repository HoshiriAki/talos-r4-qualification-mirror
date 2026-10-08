#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/devices.rs',
  authority: 'backend/src/application/device_read_authority.rs',
  services: 'backend/src/application/services.rs',
  sqlite: 'backend/src/repositories/device.rs',
  postgres: 'backend/src/repositories/device_postgres.rs',
  dispatch: 'backend/src/repositories/device_dispatch.rs',
  pgTest: 'backend/src/repositories/postgres/device_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectDeviceReadSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDeviceReadSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'device_bounded_read::enforce_warning_scan_budget',
    'device_bounded_read::query_devices_paged_bounded',
    'device_bounded_read::export_devices_bounded',
    'device_bounded_read::export_devices_by_serial_nos_bounded',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('device paged/export route retains SQLite read authority: ' + forbidden)
    }
  }

  for (const token of [
    '.device_reads()',
    '.paged(',
    '.export_filtered(',
    '.export_by_serials(',
    '.build_export_workbook(',
  ]) {
    if (!snapshot.route.includes(token)) {
      errors.push('device read application cutover missing: ' + token)
    }
  }

  for (const token of [
    'pub fn device_reads(&self) -> DeviceReadAuthorityService',
    'DeviceReadAuthorityService::new(self.repository_provider())',
  ]) {
    if (!snapshot.services.includes(token)) {
      errors.push('ApplicationServices device read factory missing: ' + token)
    }
  }

  for (const token of [
    'DEVICE_EXPORT_ROWS_MAX: usize = 500',
    'DEVICE_WARNING_SCAN_ROWS_MAX: usize = 5_000',
    '.compatibility_count(&request)',
    '.compatibility_rows(&request',
    '.compatibility_rows_by_serials(&unique)',
    'fn calculate_warning(',
    'fn enrich_row(',
    'build_export_workbook',
  ]) {
    if (!snapshot.authority.includes(token)) {
      errors.push('device read authority missing: ' + token)
    }
  }

  for (const token of [
    'WHERE od.tenant_id=d.tenant_id AND od.serialNo=d.serialNo',
    'JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id',
    'WHERE d.tenant_id=? AND d.serialNo IN',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite device compatibility read scope missing: ' + token)
    }
  }

  for (const token of [
    'WHERE od.tenant_id=d.tenant_id AND od.serialno=d.serialno',
    'JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id',
    'FROM devices d WHERE d.tenant_id = ',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL device compatibility read scope missing: ' + token)
    }
  }

  for (const token of [
    'pub fn compatibility_count(',
    'pub fn compatibility_rows(',
    'pub fn compatibility_rows_by_serials(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push('scoped device compatibility dispatch missing: ' + token)
    }
  }

  for (const token of [
    'DeviceCompatibilityReadRequest',
    '.compatibility_count(&compatibility_request)?',
    '.compatibility_rows(',
    '.compatibility_rows_by_serials(&[',
    'vec!["COREA001".to_string()]',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 device compatibility evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-device-read-cutover.test.mjs',
    'node scripts/check-r4-p8-device-read-cutover.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-W qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDeviceReadSnapshot(collectDeviceReadSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 device read cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 device read cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
