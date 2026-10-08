#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/devices.rs',
  module: 'backend/src/application/device_compatibility.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/device.rs',
  dispatch: 'backend/src/repositories/device_dispatch.rs',
  postgres: 'backend/src/repositories/device_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/device_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

function count(source, token) {
  return source.split(token).length - 1
}

export function collectDeviceAuthoritySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDeviceAuthoritySnapshot(snapshot) {
  const errors = []
  const route = runtime(snapshot.route)

  if (count(route, 'state.pool') !== 0) {
    errors.push('device route must not retain raw SQLite pool authority after P8-X/P8-Y')
  }
  for (const forbidden of [
    'device_bounded_read::list_legacy_devices_bounded(',
    'device_service::checkin_by_scan(&state.pool',
    'device_service::undo_checkin_scan(&state.pool',
    'device_service::bulk_update_devices(&state.pool',
    'excel_import_service::import_devices_from_excel(',
    'device_bounded_read::query_devices_paged_bounded',
    'device_bounded_read::enforce_warning_scan_budget',
    'device_bounded_read::export_devices_bounded',
    'device_bounded_read::export_devices_by_serial_nos_bounded',
  ]) {
    if (route.includes(forbidden)) {
      errors.push('device route restored raw SQLite authority: ' + forbidden)
    }
  }
  for (const forbidden of [
    'audit_service::write_audit_log(',
    'device_service::delete_device(&state.pool',
  ]) {
    if (route.includes(forbidden)) {
      errors.push('device route P8-S closure regressed: ' + forbidden)
    }
  }
  for (const token of [
    'audit_service::write_audit_log_with_repository',
    'state.audit_compatibility_repository()',
    '"device"',
    '"delete_device"',
  ]) {
    if (!route.includes(token)) errors.push('device route P8-S cutover missing: ' + token)
  }

  for (const token of [
    'DeviceCompatibilityModule',
    'self.repository_provider',
    '.bind(ctx)',
    '.devices()',
    'FeatureDevice::new().metadata()',
    'FeatureDevice::new().commands()',
    'FeatureDevice::new().schema()',
  ]) {
    if (!snapshot.module.includes(token)) {
      errors.push('device compatibility module missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn devices(&self) -> ScopedDeviceRepository<'_>")) {
    errors.push('ScopedRepositories must expose the scoped device authority')
  }

  for (const token of [
    'WHERE d.tenant_id = ?1 AND d.serialNo = ?2',
    'DELETE FROM devices WHERE tenant_id = ?1 AND serialNo = ?2',
    'tenant_id = ?',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite scoped device authority missing tenant boundary: ' + token)
    }
  }

  for (const token of [
    'WHERE d.tenant_id = $1 AND d.serialno = $2',
    'DELETE FROM devices WHERE tenant_id = $1 AND serialno = $2',
    'd.tenant_id = ',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL scoped device authority missing tenant boundary: ' + token)
    }
  }
  if (!snapshot.dispatch.includes('PostgresDeviceRepository')
      || !snapshot.dispatch.includes('SqliteDeviceRepository')) {
    errors.push('device repository backend dispatch is incomplete')
  }

  if (snapshot.factory.includes('with_pool!(FeatureDevice)')
      || snapshot.factory.includes('(FeatureDevice, Device, "device")')) {
    errors.push('ModuleFactory must not construct the SQLite-backed FeatureDevice authority')
  }
  for (const token of [
    '(DeviceCompatibilityModule, Device, "device")',
    'DeviceCompatibilityModule::new(repository_provider.clone())',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('ModuleFactory device authority composition missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod device_qualification_tests;')) {
    errors.push('PostgreSQL device qualification module is not registered')
  }
  for (const token of [
    'live_pg18_device_authority_preserves_scope_crud_paging_and_recomposition',
    'scoped_b.devices().get("COREA001")?.is_none()',
    'created device survives provider recomposition',
    'scoped_b.devices().delete("COREA003")?',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 device authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-device-authority-cutover.test.mjs',
    'node scripts/check-r4-p8-device-authority-cutover.mjs',
    'live_pg18_device_authority_preserves_scope_crud_paging_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-S qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDeviceAuthoritySnapshot(collectDeviceAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 device authority cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 device authority cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
