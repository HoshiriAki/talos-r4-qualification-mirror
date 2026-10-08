#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  factory: 'backend/src/registry/factory.rs',
  application: 'backend/src/application/warehouse_advanced_compatibility.rs',
  sqlite: 'backend/src/repositories/warehouse.rs',
  postgres: 'backend/src/repositories/warehouse_postgres.rs',
  dispatch: 'backend/src/repositories/warehouse_dispatch.rs',
  pgTest: 'backend/src/repositories/postgres/warehouse_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectWarehouseAdvancedSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateWarehouseAdvancedSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'with_pool!(FeatureWarehouseAdvanced)',
    'warehouse_advanced_concrete.warehouse_module',
    'warehouse_advanced_concrete.device_module',
  ]) {
    if (snapshot.factory.includes(forbidden)) {
      errors.push('warehouse advanced factory retains SQLite/module-handle authority: ' + forbidden)
    }
  }
  for (const token of [
    "WarehouseAdvancedCompatibilityModule,\n        WarehouseAdvanced,\n        \"warehouse_advanced\"",
    'WarehouseAdvancedCompatibilityModule::new(repository_provider.clone())',
    'let warehouse_advanced_built = constructed(warehouse_advanced_concrete);',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('warehouse advanced factory cutover missing: ' + token)
    }
  }

  for (const token of [
    'repository_provider: Arc<dyn RepositoryProvider>',
    'self.repository_provider',
    '.bind(ctx)',
    '.advanced_low_stock(',
    '.advanced_capacity_stats()',
    '.move_device_between_warehouses(',
    'FeatureWarehouseAdvanced::new().metadata()',
    'FeatureWarehouseAdvanced::new().commands()',
    'FeatureWarehouseAdvanced::new().schema()',
  ]) {
    if (!snapshot.application.includes(token)) {
      errors.push('warehouse advanced compatibility authority missing: ' + token)
    }
  }
  if (snapshot.application.includes('SqliteConnectionManager')
      || snapshot.application.includes('.pool')) {
    errors.push('warehouse advanced compatibility authority must be storage-backend neutral')
  }

  for (const token of [
    "d.currentWarehouseId=w.id AND d.tenant_id=w.tenant_id",
    "d.rentalStatus IN ('available','已入库')",
    "d.rentalStatus IN ('rented','租赁中')",
    'WHERE tenant_id=?2 AND serialNo=?3 AND currentWarehouseId=?4',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite warehouse advanced scoped/canonical contract missing: ' + token)
    }
  }

  for (const token of [
    'd.currentwarehouseid=w.id AND d.tenant_id=w.tenant_id',
    "d.rentalstatus IN ('available','已入库')",
    "d.rentalstatus IN ('rented','租赁中')",
    'WHERE tenant_id=$2 AND serialno=$3 AND currentwarehouseid=$4',
    'pg_write_serializable',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL warehouse advanced scoped/canonical contract missing: ' + token)
    }
  }

  for (const token of [
    'pub fn advanced_low_stock(',
    'pub fn advanced_capacity_stats(',
    'pub fn move_device_between_warehouses(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push('warehouse advanced backend dispatch missing: ' + token)
    }
  }

  for (const token of [
    'advanced_low_stock(None, 2)?',
    'let low_stock_b = scoped_b.warehouses().advanced_low_stock(None, 2)?;',
    'assert_eq!(low_stock_b[0].warehouse_id, "warehouse-b")',
    'assert_eq!(low_stock_b[0].available_count, 0)',
    'advanced_capacity_stats()?',
    'WarehouseDeviceMoveOutcome::TargetWarehouseNotFound',
    'WarehouseDeviceMoveOutcome::DeviceNotInSource',
    'WarehouseDeviceMoveOutcome::Moved',
    '.devices("warehouse-a-target", Some("已入库"))?',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 warehouse advanced evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-warehouse-advanced-cutover.test.mjs',
    'node scripts/check-r4-p8-warehouse-advanced-cutover.mjs',
    'live_pg18_warehouse_authority_preserves_scope_regions_devices_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-AA qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateWarehouseAdvancedSnapshot(collectWarehouseAdvancedSnapshot())
  if (errors.length) {
    console.error('R4-P8 warehouse advanced cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 warehouse advanced cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
