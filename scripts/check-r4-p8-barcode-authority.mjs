#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  official: 'backend/system/admin/src/barcode.rs',
  route: 'backend/src/routes/barcode.rs',
  frontend: 'frontend/src/api/barcode.ts',
  application: 'backend/src/application/barcode_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/barcode.rs',
  dispatch: 'backend/src/repositories/barcode_dispatch.rs',
  pgMutation: 'backend/src/repositories/barcode_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/barcode_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/barcode_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectBarcodeAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateBarcodeAuthoritySnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const route = compact(snapshot.route)
  const frontend = compact(snapshot.frontend)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'BarcodeCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.barcodes()',
    '.generate(',
    '.batch_generate(',
    '.lookup(',
    '.record_scan(',
    '.history(',
    '.stats(',
    'FeatureBarcode::new().metadata()',
    'FeatureBarcode::new().commands()',
    'FeatureBarcode::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Barcode compatibility authority missing: ' + token)
    }
  }

  if (!route.includes('state.registry.execute("barcode"')
      || route.includes('state.pool')) {
    errors.push('Barcode HTTP route must execute through Registry without direct SQLite pool access')
  }

  if (!frontend.includes('scannedBy:string')
      || !frontend.includes('warehouseId:string|null')) {
    errors.push('Barcode frontend scan identity fields must remain canonical text identities')
  }
  if (!compact(snapshot.official).includes('row.get::<_,String>(4)?')
      || !compact(snapshot.official).includes('row.get::<_,Option<String>>(5)?')) {
    errors.push('Legacy Barcode scan-history compatibility must decode text identity fields')
  }

  for (const token of [
    'mod barcode;',
    'mod barcode_dispatch;',
    'mod barcode_postgres;',
    'mod barcode_postgres_mutation;',
    'mod barcode_postgres_read;',
    'pub use barcode_dispatch::ScopedBarcodeRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Barcode repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn barcodes(&self) -> ScopedBarcodeRepository<'_>")) {
    errors.push('ScopedRepositories must expose Barcode authority')
  }
  if (!snapshot.dispatch.includes('PostgresBarcodeRepository')
      || !snapshot.dispatch.includes('SqliteBarcodeRepository')) {
    errors.push('Barcode backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'JOIN devices d ON d.serialNo=bl.device_serial_no',
    'WHERE d.tenant_id=?1 AND bl.device_serial_no=?2',
    'd.warning_status',
    'FROM tenant_memberships',
    "status='active'",
    'FROM warehouses',
    'INSERT INTO scan_events',
    'tenant_id',
    'sqlite_barcode_authority_preserves_scope_idempotence_scan_references_and_stats',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Barcode authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'WHERE tenant_id=$1 AND serialno=$2',
    'FOR UPDATE',
    'FROM tenant_memberships',
    "status='active'",
    'FOR KEY SHARE',
    'FROM warehouses',
    'INSERT INTO scan_events',
    'tenant_id',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Barcode mutation invariant missing: ' + token)
    }
  }

  for (const token of [
    'JOIN devices d ON d.serialno=bl.device_serial_no',
    'WHERE d.tenant_id=$1 AND bl.barcode_text=$2',
    'd.warning_status AS status',
    'QueryBuilder::<Postgres>',
    'tenant_id=',
    'COUNT(*)::bigint FROM scan_events',
  ]) {
    if (!snapshot.pgRead.includes(token)) {
      errors.push('PostgreSQL Barcode read invariant missing: ' + token)
    }
  }

  if (!factory.includes('BarcodeCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Barcode provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureBarcode)')) {
    errors.push('Registry Barcode restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("barcode",Business,ModuleActivation::Always,NONE,Barcode)')) {
    errors.push('Barcode Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod barcode_qualification_tests;')) {
    errors.push('PostgreSQL Barcode qualification module is not registered')
  }
  for (const token of [
    'live_pg18_barcode_authority_preserves_scope_idempotence_scan_references_stats_and_recomposition',
    'Err(BarcodeMutationError::ScanReferencesNotFound)',
    'scoped_b.barcodes().lookup(&generated.barcode_text)?.is_none()',
    'stats.by_type.inventory, 1',
    'lookup.device_info.status.as_deref(), Some("正常")',
    'persisted.device_serial_no, "BAR-A-001"',
  ]) {
    if (!compact(snapshot.pgTest).includes(compact(token))) {
      errors.push('PG18 Barcode authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-barcode-authority.test.mjs',
    'node scripts/check-r4-p8-barcode-authority.mjs',
    'live_pg18_barcode_authority_preserves_scope_idempotence_scan_references_stats_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Barcode qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateBarcodeAuthoritySnapshot(collectBarcodeAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Barcode authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Barcode authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
