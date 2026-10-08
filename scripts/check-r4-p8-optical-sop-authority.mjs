#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  route: 'backend/src/routes/optical_sop.rs',
  frontendStore: 'frontend/src/stores/optical-sop.ts',
  frontendPage: 'frontend/src/pages/OpticalSopPage.vue',
  application: 'backend/src/application/optical_sop_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/optical_sop.rs',
  dispatch: 'backend/src/repositories/optical_sop_dispatch.rs',
  pgMutation: 'backend/src/repositories/optical_sop_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/optical_sop_postgres_read.rs',
  migrationSqlite: 'backend/src/db/migrations/081_r4_optical_sop_tenant_invariant.sql',
  migrationPg: 'backend/src/db/migrations/postgres/081_r4_optical_sop_tenant_invariant.sql',
  migrationRegistry: 'backend/src/db/r4_migrations.rs',
  migrationComposition: 'backend/src/db/mod.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/optical_sop_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectOpticalSopAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateOpticalSopAuthoritySnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const route = compact(snapshot.route)
  const store = compact(snapshot.frontendStore)
  const page = compact(snapshot.frontendPage)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)
  const sqliteMigration = compact(snapshot.migrationSqlite)
  const pgMigration = compact(snapshot.migrationPg)

  for (const token of [
    'OpticalSopCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.optical_sops()',
    '.create(',
    '.update_step(',
    '.complete(',
    '.get(',
    '.list(',
    '.stats(',
    'FeatureOpticalSop::new().metadata()',
    'FeatureOpticalSop::new().commands()',
    'FeatureOpticalSop::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Optical SOP compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'order_id:String',
    'alias="order_id"',
    'alias="device_serial_no"',
    'alias="inspection_id"',
    'alias="passed"',
    'order_id:Option<String>',
    'alias="overall_grade"',
    'alias="page_size"',
  ]) {
    if (!route.includes(compact(token))) {
      errors.push('Optical SOP HTTP compatibility missing: ' + token)
    }
  }
  if (route.includes('order_id:i64')) {
    errors.push('Optical SOP HTTP order identity regressed to numeric')
  }

  for (const token of [
    'data.record||data.inspection||data.data||data',
    "body.step.endsWith('_ok')?body.step.slice(0,-3):body.step",
    'awaitfetchOne(String(id))',
  ]) {
    if (!store.includes(compact(token))) {
      errors.push('Optical SOP frontend store compatibility missing: ' + token)
    }
  }
  for (const token of [
    "key:'body',field:'bodyOk',noteField:'bodyNote'",
    "key:'function',field:'functionOk',noteField:'functionNote'",
    "case'minor_damage'",
    "case'major_damage'",
    'result.overallGrade',
  ]) {
    if (!page.includes(compact(token))) {
      errors.push('Optical SOP frontend wizard compatibility missing: ' + token)
    }
  }

  for (const token of [
    'mod optical_sop;',
    'mod optical_sop_dispatch;',
    'mod optical_sop_postgres;',
    'mod optical_sop_postgres_mutation;',
    'mod optical_sop_postgres_read;',
    'pub use optical_sop_dispatch::ScopedOpticalSopRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Optical SOP repository wiring missing: ' + token)
    }
  }
  if (!snapshot.provider.includes("pub fn optical_sops(&self) -> ScopedOpticalSopRepository<'_>")) {
    errors.push('ScopedRepositories must expose Optical SOP authority')
  }
  if (!snapshot.dispatch.includes('PostgresOpticalSopRepository')
      || !snapshot.dispatch.includes('SqliteOpticalSopRepository')) {
    errors.push('Optical SOP backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM orders',
    'FROM order_devices od',
    'JOIN devices d',
    'completed_at',
    'INSERT INTO damage_reports',
    'appearance_ok',
    'reported_by',
    'sqlite_optical_sop_authority_preserves_scope_completion_and_damage_link',
  ]) {
    if (!compact(snapshot.sqlite).includes(compact(token))) {
      errors.push('SQLite Optical SOP invariant missing: ' + token)
    }
  }
  for (const forbidden of ['reporter_id', 'severity', 'detected_at']) {
    if (snapshot.sqlite.includes(forbidden)) {
      errors.push('SQLite Optical SOP restored obsolete Damage schema: ' + forbidden)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'FROM orders',
    'FOR UPDATE',
    'FROM inspection_checklists',
    'FOR KEY SHARE OF d',
    'completed_at',
    'INSERT INTO damage_reports',
    'appearance_ok',
    'reported_by',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Optical SOP mutation invariant missing: ' + token)
    }
  }
  for (const forbidden of ['reporter_id', 'severity', 'detected_at']) {
    if (snapshot.pgMutation.includes(forbidden)) {
      errors.push('PostgreSQL Optical SOP restored obsolete Damage schema: ' + forbidden)
    }
  }

  for (const token of [
    'QueryBuilder::<Postgres>',
    'tenant_id=',
    '(body_ok<>0) AS body_ok',
    'COUNT(*)::bigint',
    'completed_at',
  ]) {
    if (!snapshot.pgRead.includes(token)) {
      errors.push('PostgreSQL Optical SOP read invariant missing: ' + token)
    }
  }

  for (const token of [
    'order_id TEXT NOT NULL',
    'inspector_id TEXT NOT NULL',
    'damage_report_id TEXT',
    'completed_at TEXT',
    'tenant_id TEXT NOT NULL',
    'UNIQUE(tenant_id, order_id, device_serial_no)',
    'FOREIGN KEY(order_id, tenant_id)',
    'FOREIGN KEY(device_serial_no, tenant_id)',
  ]) {
    if (!sqliteMigration.includes(compact(token))) {
      errors.push('SQLite Optical SOP migration invariant missing: ' + token)
    }
  }
  for (const token of [
    'ADD COLUMN IF NOT EXISTS completed_at TEXT',
    'ALTER COLUMN tenant_id SET NOT NULL',
    'UNIQUE (tenant_id, order_id, device_serial_no)',
    'FOREIGN KEY (order_id, tenant_id)',
    'FOREIGN KEY (device_serial_no, tenant_id)',
  ]) {
    if (!pgMigration.includes(compact(token))) {
      errors.push('PostgreSQL Optical SOP migration invariant missing: ' + token)
    }
  }
  if (!snapshot.migrationRegistry.includes('MIGRATION_081_ID')
      || !snapshot.migrationRegistry.includes('run_sqlite_extension_081')
      || !snapshot.migrationRegistry.includes('run_pg_extension_081')) {
    errors.push('Optical SOP migration 081 is not registered')
  }
  if (!snapshot.migrationComposition.includes('run_sqlite_extension_081(conn)?')
      || !snapshot.migrationComposition.includes('run_pg_extension_081(pool).await?')) {
    errors.push('Optical SOP migration 081 is not in production migration composition')
  }

  if (!factory.includes('OpticalSopCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Optical SOP provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureOpticalSop)')) {
    errors.push('Registry Optical SOP restored direct SQLite construction')
  }
  if (!descriptors.includes(
    'descriptor!("optical_sop",Business,ModuleActivation::Always,NONE,OpticalSop)',
  )) {
    errors.push('Optical SOP Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod optical_sop_qualification_tests;')) {
    errors.push('PostgreSQL Optical SOP qualification module is not registered')
  }
  for (const token of [
    'live_pg18_optical_sop_authority_preserves_scope_completion_damage_link_and_recomposition',
    'Err(OpticalSopMutationError::Duplicate)',
    'Err(OpticalSopMutationError::DeviceNotInOrder)',
    'Err(OpticalSopMutationError::Completed)',
    'reports[0].reported_by, "optical-actor-a"',
    'assert_eq!(damage_count, 1);',
    'persisted.damage_report_id.as_deref()',
  ]) {
    if (!compact(snapshot.pgTest).includes(compact(token))) {
      errors.push('PG18 Optical SOP evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-optical-sop-authority.test.mjs',
    'node scripts/check-r4-p8-optical-sop-authority.mjs',
    'live_pg18_optical_sop_authority_preserves_scope_completion_damage_link_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Optical SOP qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateOpticalSopAuthoritySnapshot(collectOpticalSopAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Optical SOP authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Optical SOP authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
