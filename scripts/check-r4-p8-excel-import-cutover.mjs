#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  factory: 'backend/src/registry/factory.rs',
  application: 'backend/src/application/excel_import_compatibility.rs',
  modelSqlite: 'backend/src/repositories/model.rs',
  modelPostgres: 'backend/src/repositories/model_postgres.rs',
  modelDispatch: 'backend/src/repositories/model_dispatch.rs',
  warehouseSqlite: 'backend/src/repositories/warehouse.rs',
  warehousePostgres: 'backend/src/repositories/warehouse_postgres.rs',
  warehouseDispatch: 'backend/src/repositories/warehouse_dispatch.rs',
  pgTest: 'backend/src/repositories/postgres/device_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectExcelImportSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateExcelImportSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'with_pool!(FeatureExcelImport)',
    '(FeatureExcelImport, ExcelImport, "excel_import")',
    'excel_import_concrete.device_module',
  ]) {
    if (snapshot.factory.includes(forbidden)) {
      errors.push('Registry Excel import retains SQLite/module-handle authority: ' + forbidden)
    }
  }
  for (const token of [
    '(ExcelImportCompatibilityModule, ExcelImport, "excel_import")',
    'ExcelImportCompatibilityModule::new(repository_provider.clone())',
    'let excel_import_built = constructed(excel_import_concrete);',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('Registry Excel import factory cutover missing: ' + token)
    }
  }

  for (const forbidden of [
    'SqliteConnectionManager',
    'rusqlite',
    'get_conn(',
    'SELECT id FROM device_models',
    'SELECT id FROM warehouses',
    '.pool',
  ]) {
    if (snapshot.application.includes(forbidden)) {
      errors.push('Excel import compatibility authority is not backend-neutral: ' + forbidden)
    }
  }
  for (const token of [
    'legacy_parser: FeatureExcelImport',
    'FeatureExcelImport::new()',
    'repository_provider: Arc<dyn RepositoryProvider>',
    'DeviceCompatibilityModule::new(repository_provider.clone())',
    '.bind(ctx)',
    '.models()',
    '.find_by_name(name)',
    '.warehouses()',
    '"get_device"',
    '"create_device"',
    '"parse_excel" | "validate_excel"',
    'self.legacy_parser.execute(command, payload, ctx)',
    'duplicate serialNo within file',
  ]) {
    if (!snapshot.application.includes(token)) {
      errors.push('Excel import compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'WHERE dm.tenant_id = ?1 AND dm.name = ?2',
    'LIMIT 1',
  ]) {
    if (!snapshot.modelSqlite.includes(token)) {
      errors.push('SQLite model name lookup missing tenant boundary: ' + token)
    }
  }
  for (const token of [
    'WHERE dm.tenant_id = $1 AND dm.name = $2',
    'LIMIT 1',
  ]) {
    if (!snapshot.modelPostgres.includes(token)) {
      errors.push('PostgreSQL model name lookup missing tenant boundary: ' + token)
    }
  }
  if (!snapshot.modelDispatch.includes('pub fn find_by_name(')) {
    errors.push('scoped model repository does not dispatch exact-name lookup')
  }

  for (const token of [
    'FROM warehouses WHERE tenant_id=?1 AND name=?2 LIMIT 1',
  ]) {
    if (!snapshot.warehouseSqlite.includes(token)) {
      errors.push('SQLite warehouse name lookup missing tenant boundary: ' + token)
    }
  }
  for (const token of [
    'FROM warehouses WHERE tenant_id=$1 AND name=$2 LIMIT 1',
  ]) {
    if (!snapshot.warehousePostgres.includes(token)) {
      errors.push('PostgreSQL warehouse name lookup missing tenant boundary: ' + token)
    }
  }
  if (!snapshot.warehouseDispatch.includes('pub fn find_by_name(')) {
    errors.push('scoped warehouse repository does not dispatch exact-name lookup')
  }

  for (const token of [
    'live_pg18_excel_import_registry_preserves_tenant_lookup_and_device_write_authority',
    '"serialNo": "REGIMPORT001"',
    '"modelName": "Model Core A"',
    '"modelName": "Model Core B"',
    'assert_eq!(device_a.model_id, "model-core-a")',
    'assert_eq!(device_b.model_id, "model-core-b")',
    '"serialNo": "REGISOLATE001"',
    'assert_eq!(isolated_lookup["created"], 0)',
    'assert_eq!(isolated_lookup["skipped"], 1)',
    'assert!(scoped_a.devices().get("REGISOLATE001")?.is_none())',
    'assert_eq!(duplicate["skipped"], 1)',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Registry Excel-import evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-excel-import-cutover.test.mjs',
    'node scripts/check-r4-p8-excel-import-cutover.mjs',
    'live_pg18_excel_import_registry_preserves_tenant_lookup_and_device_write_authority',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-AB qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateExcelImportSnapshot(collectExcelImportSnapshot())
  if (errors.length) {
    console.error('R4-P8 Registry Excel import cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Registry Excel import cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
