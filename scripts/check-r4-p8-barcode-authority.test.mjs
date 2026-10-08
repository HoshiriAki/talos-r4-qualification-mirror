#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectBarcodeAuthoritySnapshot,
  validateBarcodeAuthoritySnapshot,
} from './check-r4-p8-barcode-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectBarcodeAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateBarcodeAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateBarcodeAuthoritySnapshot(collectBarcodeAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Barcode factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'BarcodeCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureBarcode)',
      'Barcode factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Barcode descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("barcode", Business, ModuleActivation::Always, NONE, Barcode)',
      'descriptor!("barcode", Business, ModuleActivation::Always, SQLITE, Barcode)',
      'Barcode descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Barcode PostgreSQL mutation loses serialization',
  snapshot => {
    assert.ok(
      snapshot.pgMutation.includes('pg_write_serializable_repository'),
      'serialization mutation anchor missing',
    )
    snapshot.pgMutation = snapshot.pgMutation.replaceAll(
      'pg_write_serializable_repository',
      'pg_write',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Barcode PostgreSQL generation loses device row lock',
  snapshot => {
    assert.ok(snapshot.pgMutation.includes('FOR UPDATE'), 'row-lock mutation anchor missing')
    snapshot.pgMutation = snapshot.pgMutation.replaceAll('FOR UPDATE', 'NO_ROW_LOCK')
  },
  'mutation invariant missing',
)

expectFailure(
  'Barcode PostgreSQL reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id='), 'tenant-read mutation anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
    snapshot.pgRead = snapshot.pgRead.replaceAll('d.tenant_id=$1', 'd.tenant_scope_removed=$1')
  },
  'read invariant missing',
)

expectFailure(
  'Barcode PostgreSQL status mapping restores nonexistent device status',
  snapshot => {
    snapshot.pgRead = replaceRequired(
      snapshot.pgRead,
      'd.warning_status AS status',
      'd.status',
      'Barcode PostgreSQL status mapping restores nonexistent device status',
    )
  },
  'read invariant missing',
)

expectFailure(
  'Barcode HTTP route restores direct pool access',
  snapshot => {
    snapshot.route = snapshot.route + '\n// state.pool.get()\n'
  },
  'without direct SQLite pool access',
)

expectFailure(
  'Barcode frontend restores numeric scan identities',
  snapshot => {
    snapshot.frontend = replaceRequired(
      snapshot.frontend,
      'scannedBy: string',
      'scannedBy: number',
      'Barcode frontend restores numeric scan identities',
    )
  },
  'canonical text identities',
)

expectFailure(
  'Barcode live proof loses cross-tenant reference rejection',
  snapshot => {
    assert.ok(
      snapshot.pgTest.includes('Err(BarcodeMutationError::ScanReferencesNotFound)'),
      'live-proof mutation anchor missing',
    )
    snapshot.pgTest = snapshot.pgTest.replaceAll(
      'Err(BarcodeMutationError::ScanReferencesNotFound)',
      'Err(BarcodeMutationError::DeviceNotFound)',
    )
  },
  'ScanReferencesNotFound',
)

console.log('R4-P8 Barcode authority mutation tests passed.')
