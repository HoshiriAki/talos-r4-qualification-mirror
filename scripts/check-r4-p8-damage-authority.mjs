#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  application: 'backend/src/application/damage_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/damage.rs',
  dispatch: 'backend/src/repositories/damage_dispatch.rs',
  pgReport: 'backend/src/repositories/damage_postgres_report.rs',
  pgTransition: 'backend/src/repositories/damage_postgres_transition.rs',
  pgRead: 'backend/src/repositories/damage_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/damage_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectDamageAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateDamageAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const descriptors = compact(snapshot.descriptors)
  const factory = compact(snapshot.factory)

  for (const token of [
    'DamageCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.damages()',
    '.report(',
    '.assess(',
    '.adjudicate(',
    '.get_by_order(',
    '.get_by_device(',
    '.list(',
    'FeatureDamage::new().metadata()',
    'FeatureDamage::new().commands()',
    'FeatureDamage::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Damage compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod damage;',
    'mod damage_dispatch;',
    'mod damage_postgres;',
    'mod damage_postgres_report;',
    'mod damage_postgres_transition;',
    'mod damage_postgres_read;',
    'pub use damage_dispatch::ScopedDamageRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Damage repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn damages(&self) -> ScopedDamageRepository<'_>")) {
    errors.push('ScopedRepositories must expose Damage authority')
  }
  if (!snapshot.dispatch.includes('PostgresDamageRepository')
      || !snapshot.dispatch.includes('SqliteDamageRepository')) {
    errors.push('Damage backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM orders o JOIN devices d',
    'INSERT INTO damage_reports',
    'status != "reported"',
    'status != "assessed"',
    'tenant_id',
    'sqlite_damage_authority_preserves_scope_and_state_machine',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Damage authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'FROM orders o JOIN devices d',
    'o.tenant_id=$2',
    'd.tenant_id=$2',
    'INSERT INTO damage_reports',
  ]) {
    if (!snapshot.pgReport.includes(token)) {
      errors.push('PostgreSQL Damage report invariant missing: ' + token)
    }
  }
  for (const token of [
    'pg_write_serializable_repository',
    'FROM damage_reports',
    'FOR UPDATE',
    'damage-not-reported:',
    'damage-not-assessed:',
  ]) {
    if (!snapshot.pgTransition.includes(token)) {
      errors.push('PostgreSQL Damage transition invariant missing: ' + token)
    }
  }
  if (!snapshot.pgRead.includes('QueryBuilder::<Postgres>')
      || !snapshot.pgRead.includes('tenant_id=')
      || !snapshot.pgRead.includes('(appearance_ok <> 0) AS appearance_ok')
      || !snapshot.pgRead.includes('estimated_damage_amount::double precision')) {
    errors.push('PostgreSQL Damage reads must remain tenant-scoped and type-normalized')
  }

  if (!factory.includes('DamageCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Damage provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureDamage)')) {
    errors.push('Registry Damage restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("damage",Business,ModuleActivation::Always,NONE,Damage)')) {
    errors.push('Damage Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod damage_qualification_tests;')) {
    errors.push('PostgreSQL Damage qualification module is not registered')
  }
  for (const token of [
    'live_pg18_damage_authority_preserves_scope_state_machine_and_recomposition',
    'Err(DamageMutationError::NotFound)',
    'Err(DamageMutationError::NotAssessed(status)) if status == "reported"',
    'Err(DamageMutationError::ResourceNotFound)',
    'persisted[0].status, "adjudicated"',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Damage authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-damage-authority.test.mjs',
    'node scripts/check-r4-p8-damage-authority.mjs',
    'live_pg18_damage_authority_preserves_scope_state_machine_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Damage qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDamageAuthoritySnapshot(collectDamageAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Damage authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Damage authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
