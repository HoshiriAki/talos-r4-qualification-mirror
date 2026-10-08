#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectExcelImportSnapshot,
  validateExcelImportSnapshot,
} from './check-r4-p8-excel-import-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectExcelImportSnapshot())
  mutate(snapshot)
  const errors = validateExcelImportSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateExcelImportSnapshot(collectExcelImportSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'factory restores SQLite-backed FeatureExcelImport',
  snapshot => {
    snapshot.factory += '\nfn regression() { let _ = with_pool!(FeatureExcelImport); }\n'
  },
  'with_pool!(FeatureExcelImport)',
)

expectFailure(
  'Excel import compatibility authority stops binding execution context',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      '.bind(ctx)',
      '.bind_removed(ctx)',
      'Excel import compatibility authority stops binding execution context',
    )
  },
  '.bind(ctx)',
)

expectFailure(
  'PostgreSQL model-name lookup loses tenant scope',
  snapshot => {
    snapshot.modelPostgres = replaceRequired(
      snapshot.modelPostgres,
      'WHERE dm.tenant_id = $1 AND dm.name = $2',
      'WHERE dm.name = $2',
      'PostgreSQL model-name lookup loses tenant scope',
    )
  },
  'WHERE dm.tenant_id = $1 AND dm.name = $2',
)

expectFailure(
  'PostgreSQL warehouse-name lookup loses tenant scope',
  snapshot => {
    snapshot.warehousePostgres = replaceRequired(
      snapshot.warehousePostgres,
      'FROM warehouses WHERE tenant_id=$1 AND name=$2 LIMIT 1',
      'FROM warehouses WHERE name=$2 LIMIT 1',
      'PostgreSQL warehouse-name lookup loses tenant scope',
    )
  },
  'FROM warehouses WHERE tenant_id=$1 AND name=$2 LIMIT 1',
)

expectFailure(
  'pure Excel parsing stops delegating to the established parser',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      'self.legacy_parser.execute(command, payload, ctx)',
      'Err("parser delegation removed".into())',
      'pure Excel parsing stops delegating to the established parser',
    )
  },
  'self.legacy_parser.execute(command, payload, ctx)',
)

expectFailure(
  'PG18 proof loses cross-tenant import fail-closed isolation',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert!(scoped_a.devices().get("REGISOLATE001")?.is_none())',
      'assert!(scoped_a.devices().get("REGISOLATE001")?.is_some())',
      'PG18 proof loses cross-tenant import fail-closed isolation',
    )
  },
  'assert!(scoped_a.devices().get("REGISOLATE001")?.is_none())',
)

console.log('R4-P8 Registry Excel import cutover mutation tests passed.')
