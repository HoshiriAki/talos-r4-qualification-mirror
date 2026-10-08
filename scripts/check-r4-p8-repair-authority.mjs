#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/repair_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/repair.rs',
  dispatch: 'backend/src/repositories/repair_dispatch.rs',
  pgCreate: 'backend/src/repositories/repair_postgres_create.rs',
  pgTransition: 'backend/src/repositories/repair_postgres_transition.rs',
  pgRead: 'backend/src/repositories/repair_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/repair_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectRepairAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateRepairAuthoritySnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'RepairCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.repairs()',
    '.create(',
    '.start(',
    '.complete(',
    '.return_to_stock(',
    '.get(',
    '.list(',
    '.device_stats(',
    '.procurements()',
    'FeatureRepair::new().metadata()',
    'FeatureRepair::new().commands()',
    'FeatureRepair::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Repair compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod repair;',
    'mod repair_dispatch;',
    'mod repair_postgres;',
    'mod repair_postgres_create;',
    'mod repair_postgres_transition;',
    'mod repair_postgres_read;',
    'pub use repair_dispatch::ScopedRepairRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Repair repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn repairs(&self) -> ScopedRepairRepository<'_>")) {
    errors.push('ScopedRepositories must expose Repair authority')
  }
  if (!snapshot.dispatch.includes('PostgresRepairRepository')
      || !snapshot.dispatch.includes('SqliteRepairRepository')) {
    errors.push('Repair backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM damage_reports',
    'INSERT INTO repair_orders',
    'UPDATE devices SET rentalStatus',
    "status='returned'",
    'tenant_id',
    'sqlite_repair_authority_preserves_scope_lifecycle_device_return_and_stats',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Repair authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'FROM damage_reports',
    'FOR UPDATE',
    'SELECT id FROM repair_orders',
    'INSERT INTO repair_orders',
  ]) {
    if (!snapshot.pgCreate.includes(token)) {
      errors.push('PostgreSQL Repair create invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'FROM repair_orders',
    'FOR UPDATE',
    "UPDATE devices SET rentalstatus='available'",
    "SET status='returned'",
    'repair-not-completed:',
  ]) {
    if (!snapshot.pgTransition.includes(token)) {
      errors.push('PostgreSQL Repair transition invariant missing: ' + token)
    }
  }

  if (!snapshot.pgRead.includes('QueryBuilder::<Postgres>')
      || !snapshot.pgRead.includes('tenant_id=')
      || !snapshot.pgRead.includes('repair_cost::double precision')
      || !snapshot.pgRead.includes('SUM(repair_cost),0)::double precision')) {
    errors.push('PostgreSQL Repair reads must remain tenant-scoped and type-normalized')
  }

  if (!factory.includes('RepairCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Repair provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureRepair)')) {
    errors.push('Registry Repair restored direct SQLite construction')
  }
  if (snapshot.factory.includes('repair_concrete.device_module')
      || snapshot.factory.includes('repair_concrete.deposit_module')) {
    errors.push('Registry Repair restored legacy module-handle injection')
  }
  if (!descriptors.includes('descriptor!("repair",Business,ModuleActivation::Always,NONE,Repair)')) {
    errors.push('Repair Registry descriptor must not require SQLite or legacy module handles')
  }

  if (!snapshot.postgresMod.includes('mod repair_qualification_tests;')) {
    errors.push('PostgreSQL Repair qualification module is not registered')
  }
  for (const token of [
    'live_pg18_repair_authority_preserves_scope_lifecycle_device_return_and_recomposition',
    'Err(RepairMutationError::NotFound)',
    'Err(RepairMutationError::AlreadyExists(_))',
    'Err(RepairMutationError::DamageNotAdjudicated(status)) if status == "reported"',
    'Err(RepairMutationError::NotInProgress(status)) if status == "pending"',
    'device.status, "available"',
    'persisted.total, 1',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Repair authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-repair-authority.test.mjs',
    'node scripts/check-r4-p8-repair-authority.mjs',
    'live_pg18_repair_authority_preserves_scope_lifecycle_device_return_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Repair qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateRepairAuthoritySnapshot(collectRepairAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Repair authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Repair authority gate passed.')
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
