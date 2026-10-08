#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDeviceImportSnapshot,
  validateDeviceImportSnapshot,
} from './check-r4-p8-device-import-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDeviceImportSnapshot())
  mutate(snapshot)
  const errors = validateDeviceImportSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDeviceImportSnapshot(collectDeviceImportSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'device import route restores raw SQLite pool',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'Excel device parser regains database writes',
  snapshot => {
    snapshot.parser += '\n// INSERT INTO devices\n'
  },
  'INSERT INTO devices',
)

expectFailure(
  'PostgreSQL migration keeps global device serial uniqueness',
  snapshot => {
    snapshot.pgMigration = replaceRequired(
      snapshot.pgMigration,
      'ALTER TABLE devices\n    DROP CONSTRAINT IF EXISTS devices_serialno_key;',
      '-- global device serial unique constraint retained',
      'PostgreSQL migration keeps global device serial uniqueness',
    )
  },
  'DROP CONSTRAINT IF EXISTS devices_serialno_key',
)

expectFailure(
  'SQLite migration loses one legacy composite serial FK',
  snapshot => {
    snapshot.sqliteMigration = replaceRequired(
      snapshot.sqliteMigration,
      'REFERENCES devices_new_073(serialNo, tenant_id) ON DELETE CASCADE',
      'REFERENCES devices_new_073(serialNo) ON DELETE CASCADE',
      'SQLite migration loses one legacy composite serial FK',
    )
  },
  'exactly three legacy serial child foreign keys',
)

expectFailure(
  'SQLite 073 stops restoring the tenant-delete device guard',
  snapshot => {
    snapshot.sqliteMigration = replaceRequired(
      snapshot.sqliteMigration,
      'CREATE TRIGGER prevent_tenant_delete_with_data',
      'CREATE TRIGGER removed_prevent_tenant_delete_with_data',
      'SQLite 073 stops restoring the tenant-delete device guard',
    )
  },
  'CREATE TRIGGER prevent_tenant_delete_with_data',
)

expectFailure(
  'SQLite 073 stops restoring the allocation device-model guard',
  snapshot => {
    snapshot.sqliteMigration = replaceRequired(
      snapshot.sqliteMigration,
      'CREATE TRIGGER trg_allocation_device_model_insert',
      'CREATE TRIGGER removed_allocation_device_model_insert',
      'SQLite 073 stops restoring the allocation device-model guard',
    )
  },
  'CREATE TRIGGER trg_allocation_device_model_insert',
)

expectFailure(
  'PG18 import proof stops using the same serial across tenants',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'serial_no: "SHAREDIMPORT001".into()',
      'serial_no: "TENANT_A_ONLY_IMPORT".into()',
      'PG18 import proof stops using the same serial across tenants',
    )
  },
  'cross-tenant same-serial insert',
)

console.log('R4-P8 device import cutover mutation tests passed.')
