#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectWarehouseAdvancedSnapshot,
  validateWarehouseAdvancedSnapshot,
} from './check-r4-p8-warehouse-advanced-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectWarehouseAdvancedSnapshot())
  mutate(snapshot)
  const errors = validateWarehouseAdvancedSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateWarehouseAdvancedSnapshot(collectWarehouseAdvancedSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'factory drops warehouse advanced compatibility construction identity',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      "WarehouseAdvancedCompatibilityModule,\n        WarehouseAdvanced,\n        \"warehouse_advanced\"",
      "RemovedAdvancedModule,\n        WarehouseAdvanced,\n        \"warehouse_advanced\"",
      'factory drops warehouse advanced compatibility construction identity',
    )
  },
  'warehouse advanced factory cutover missing',
)

expectFailure(
  'factory restores SQLite FeatureWarehouseAdvanced',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'WarehouseAdvancedCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureWarehouseAdvanced)',
      'factory restores SQLite FeatureWarehouseAdvanced',
    )
  },
  'with_pool!(FeatureWarehouseAdvanced)',
)

expectFailure(
  'SQLite warehouse transfer loses tenant predicate',
  snapshot => {
    snapshot.sqlite = replaceRequired(
      snapshot.sqlite,
      'WHERE tenant_id=?2 AND serialNo=?3 AND currentWarehouseId=?4',
      'WHERE serialNo=?3 AND currentWarehouseId=?4',
      'SQLite warehouse transfer loses tenant predicate',
    )
  },
  'WHERE tenant_id=?2 AND serialNo=?3 AND currentWarehouseId=?4',
)

expectFailure(
  'PostgreSQL warehouse transfer loses tenant predicate',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE tenant_id=$2 AND serialno=$3 AND currentwarehouseid=$4',
      'WHERE serialno=$3 AND currentwarehouseid=$4',
      'PostgreSQL warehouse transfer loses tenant predicate',
    )
  },
  'WHERE tenant_id=$2 AND serialno=$3 AND currentwarehouseid=$4',
)

expectFailure(
  'compatibility module stops binding the scoped repository provider',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      '.bind(ctx)',
      '.bind_removed(ctx)',
      'compatibility module stops binding the scoped repository provider',
    )
  },
  '.bind(ctx)',
)

expectFailure(
  'PG18 low-stock proof loses tenant-local warehouse identity',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(low_stock_b[0].warehouse_id, "warehouse-b")',
      'assert_eq!(low_stock_b[0].warehouse_id, "warehouse-a")',
      'PG18 low-stock proof loses tenant-local warehouse identity',
    )
  },
  'assert_eq!(low_stock_b[0].warehouse_id, "warehouse-b")',
)

expectFailure(
  'PG18 proof loses cross-tenant transfer rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'WarehouseDeviceMoveOutcome::DeviceNotInSource',
      'WarehouseDeviceMoveOutcome::Moved',
      'PG18 proof loses cross-tenant transfer rejection',
    )
  },
  'WarehouseDeviceMoveOutcome::DeviceNotInSource',
)

console.log('R4-P8 warehouse advanced cutover mutation tests passed.')
