#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/devices.rs',
  readAuthority: 'backend/src/application/device_read_authority.rs',
  mutationAuthority: 'backend/src/application/device_mutation_authority.rs',
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

export function collectDeviceMutationSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDeviceMutationSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'state.pool',
    'device_bounded_read::list_legacy_devices_bounded',
    'device_service::checkin_by_scan',
    'device_service::undo_checkin_scan',
    'device_service::bulk_update_devices',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('device P8-X route retains legacy SQLite authority: ' + forbidden)
    }
  }

  for (const token of [
    '.device_reads()',
    '.legacy_unpaged(',
    '.device_mutations()',
    '.checkin(&ctx, &serial_no)',
    '.undo_checkin(&ctx, &serial_no, &previous_status)',
    '.bulk_update(&ctx, &serial_nos, &updates)',
  ]) {
    if (!snapshot.route.includes(token)) {
      errors.push('device P8-X application cutover missing: ' + token)
    }
  }

  for (const token of [
    'DEVICE_LEGACY_LIST_ROWS_MAX: usize = 500',
    'pub fn legacy_unpaged(',
  ]) {
    if (!snapshot.readAuthority.includes(token)) {
      errors.push('legacy device read compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'DeviceMutationAuthorityService',
    'repository_provider: Arc<dyn RepositoryProvider>',
    'self.repository_provider.bind(ctx)',
    '.checkin_status(serial_no, txt::STATUS_CHECKED_IN)?',
    '.complete_legacy_orders_after_checkin(',
    '.restore_status(serial_no, previous_status)?',
    '.update_status_and_notes(serial_no, merged_status, merged_notes)?',
    '"autoCompletedOrders"',
    '"all_devices_checked_in"',
  ]) {
    if (!snapshot.mutationAuthority.includes(token)) {
      errors.push('device mutation application authority missing: ' + token)
    }
  }

  for (const token of [
    'pub fn device_mutations(&self) -> DeviceMutationAuthorityService',
    'DeviceMutationAuthorityService::new(self.repository_provider())',
  ]) {
    if (!snapshot.services.includes(token)) {
      errors.push('ApplicationServices device mutation factory missing: ' + token)
    }
  }

  for (const token of [
    'WHERE tenant_id=?1 AND serialNo=?2 LIMIT 1',
    'WHERE tenant_id=?2 AND serialNo=?3',
    'WHERE od.tenant_id=?1 AND od.serialNo=?2',
    'ON d.serialNo=od.serialNo AND d.tenant_id=od.tenant_id',
    "SET status='completed'",
    'WHERE tenant_id=?3 AND serialNo=?4',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite P8-X device mutation scope missing: ' + token)
    }
  }

  for (const token of [
    'WHERE tenant_id=$1 AND serialno=$2',
    'WHERE tenant_id=$2 AND serialno=$3',
    'WHERE od.tenant_id=$1 AND od.serialno=$2',
    'ON d.serialno=od.serialno AND d.tenant_id=od.tenant_id',
    "SET status='completed'",
    'WHERE tenant_id=$3 AND serialno=$4',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL P8-X device mutation scope missing: ' + token)
    }
  }

  for (const token of [
    'pub fn checkin_status(',
    'pub fn complete_legacy_orders_after_checkin(',
    'pub fn restore_status(',
    'pub fn update_status_and_notes(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push('scoped P8-X device mutation dispatch missing: ' + token)
    }
  }

  for (const token of [
    'assert!(\n            scoped_b\n                .devices()\n                .checkin_status("COREA001", "已入库")?\n                .is_none()\n        );',
    'scoped_b',
    '.complete_legacy_orders_after_checkin(',
    'vec!["legacy-order-a".to_string()]',
    'assert_eq!(legacy_status_a, "completed")',
    'assert_eq!(legacy_status_b, "active")',
    'scoped_b.devices().restore_status("COREA001", "rented")?',
    'assert!(!scoped_b.devices().update_status_and_notes(\n            "COREA002",\n            "rented",\n            "foreign mutation"\n        )?);',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 P8-X mutation evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-device-mutation-cutover.test.mjs',
    'node scripts/check-r4-p8-device-mutation-cutover.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-X qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDeviceMutationSnapshot(collectDeviceMutationSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 device mutation cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 device mutation cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
