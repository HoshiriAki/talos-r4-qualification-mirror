#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/warehouses.rs',
  authority: 'backend/src/application/warehouse_authority.rs',
  services: 'backend/src/application/services.rs',
  module: 'backend/src/application/warehouse_compatibility.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/warehouse.rs',
  dispatch: 'backend/src/repositories/warehouse_dispatch.rs',
  postgres: 'backend/src/repositories/warehouse_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  sqliteMigration: 'backend/src/db/migrations/072_r4_tenant_scoped_catalog_names.sql',
  pgMigration: 'backend/src/db/migrations/postgres/072_r4_tenant_scoped_catalog_names.sql',
  r4Migrations: 'backend/src/db/r4_migrations.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/warehouse_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

export function collectWarehouseAuthoritySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateWarehouseAuthoritySnapshot(snapshot) {
  const errors = []
  const route = runtime(snapshot.route)

  for (const forbidden of [
    'warehouse_service::',
    'state.pool',
    'crate::repositories',
    '.repository_provider()',
  ]) {
    if (route.includes(forbidden)) {
      errors.push('warehouse route retains forbidden persistence authority: ' + forbidden)
    }
  }
  for (const token of [
    '.application_services()',
    '.warehouse_authority()',
    'WarehouseAuthorityError',
    '"get_warehouse_stats"',
  ]) {
    if (!route.includes(token)) errors.push('warehouse route cutover missing: ' + token)
  }

  for (const token of [
    'WarehouseAuthorityService',
    'repository_provider: Arc<dyn RepositoryProvider>',
    'self.repository_provider.bind(ctx)',
    '.warehouses()',
    'upsert_region_rule',
    'devices_paged',
  ]) {
    if (!snapshot.authority.includes(token)) {
      errors.push('warehouse application authority missing: ' + token)
    }
  }
  for (const token of [
    'pub fn warehouse_authority(&self) -> WarehouseAuthorityService',
    'WarehouseAuthorityService::new(self.repository_provider())',
  ]) {
    if (!snapshot.services.includes(token)) {
      errors.push('ApplicationServices warehouse authority factory missing: ' + token)
    }
  }

  for (const token of [
    'WarehouseCompatibilityModule',
    'self.repository_provider',
    '.bind(ctx)',
    '.warehouses()',
    'FeatureWarehouse::new().metadata()',
    'FeatureWarehouse::new().commands()',
    'FeatureWarehouse::new().schema()',
  ]) {
    if (!snapshot.module.includes(token)) {
      errors.push('warehouse compatibility module missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn warehouses(&self) -> ScopedWarehouseRepository<'_>")) {
    errors.push('ScopedRepositories must expose the scoped warehouse authority')
  }
  if (!snapshot.dispatch.includes('PostgresWarehouseRepository')
      || !snapshot.dispatch.includes('SqliteWarehouseRepository')) {
    errors.push('warehouse repository backend dispatch is incomplete')
  }

  for (const token of [
    'FROM warehouses WHERE tenant_id=?1 AND id=?2',
    'WHERE tenant_id=?1 AND (currentWarehouseId=?2 OR expectedWarehouseId=?2)',
    'WHERE r.warehouseId=?1 AND w.tenant_id=?2',
    'd.currentWarehouseId=?1 AND d.tenant_id=?2',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite warehouse authority missing tenant boundary: ' + token)
    }
  }

  for (const token of [
    'FROM warehouses WHERE tenant_id=$1 AND id=$2 LIMIT 1',
    'WHERE tenant_id=$1',
    'WHERE r.warehouseid=$1 AND w.tenant_id=$2',
    'd.currentwarehouseid=$1 AND d.tenant_id=$2',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL warehouse authority missing tenant boundary: ' + token)
    }
  }

  if (snapshot.factory.includes('with_pool!(FeatureWarehouse)')
      || snapshot.factory.includes('(FeatureWarehouse, Warehouse, "warehouse")')) {
    errors.push('ModuleFactory must not construct the SQLite-backed FeatureWarehouse authority')
  }
  for (const token of [
    '(WarehouseCompatibilityModule, Warehouse, "warehouse")',
    'WarehouseCompatibilityModule::new(repository_provider.clone())',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('ModuleFactory warehouse authority composition missing: ' + token)
    }
  }

  for (const token of [
    'UNIQUE(tenant_id, name)',
    'CREATE UNIQUE INDEX idx_device_models_id_tenant_unique',
    'CREATE TABLE device_models_new_072',
    'DROP TABLE device_models;',
    'ALTER TABLE device_models_new_072 RENAME TO device_models;',
    'CREATE TABLE warehouses_new_072',
    'DROP TABLE warehouses;',
    'ALTER TABLE warehouses_new_072 RENAME TO warehouses;',
  ]) {
    if (!snapshot.sqliteMigration.includes(token)) {
      errors.push('SQLite catalog namespace migration missing: ' + token)
    }
  }
  for (const token of [
    'pragma_update(None, "foreign_keys", 0_i64)',
    'PRAGMA foreign_key_check',
    'failed to restore PRAGMA foreign_keys',
  ]) {
    if (!snapshot.r4Migrations.includes(token)) {
      errors.push('SQLite 072 migration executor guard missing: ' + token)
    }
  }

  for (const token of [
    'DROP CONSTRAINT IF EXISTS device_models_name_key',
    'DROP CONSTRAINT IF EXISTS warehouses_name_key',
    'idx_device_models_tenant_name_unique',
    'idx_warehouses_tenant_name_unique',
    'ON warehouses(tenant_id, name)',
  ]) {
    if (!snapshot.pgMigration.includes(token)) {
      errors.push('PostgreSQL catalog namespace migration missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod warehouse_qualification_tests;')) {
    errors.push('PostgreSQL warehouse qualification module is not registered')
  }
  for (const token of [
    'live_pg18_warehouse_authority_preserves_scope_regions_devices_and_recomposition',
    'assert_eq!(created_a.name, created_b.name)',
    'scoped_b.warehouses().get("warehouse-a")?.is_none()',
    'WarehouseMutationError::Referenced(1)',
    'updated warehouse survives provider recomposition',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 warehouse authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-warehouse-authority-cutover.test.mjs',
    'node scripts/check-r4-p8-warehouse-authority-cutover.mjs',
    'live_pg18_warehouse_authority_preserves_scope_regions_devices_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-U qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateWarehouseAuthoritySnapshot(collectWarehouseAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 warehouse authority cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 warehouse authority cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
