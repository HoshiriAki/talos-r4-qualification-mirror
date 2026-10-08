#!/usr/bin/env node
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  sqlite: 'backend/src/repositories/deletion_compatibility.rs',
  dispatch: 'backend/src/repositories/deletion_compatibility_dispatch.rs',
  postgres: 'backend/src/repositories/deletion_compatibility_postgres.rs',
  application: 'backend/src/application/deletion_compatibility.rs',
  systemDeletion: 'backend/system/admin/src/deletion.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  live: 'backend/src/repositories/postgres/deletion_compatibility_qualification_tests.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}
function read(relative) { return readFileSync(path.resolve(process.cwd(), relative), 'utf8') }
function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}
export function collectDeletionCompatibilitySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}
export function validateDeletionCompatibilitySnapshot(snapshot) {
  const errors = []
  for (const token of [
    'SqliteDeletionCompatibilityRepository',
    'TransactionBehavior::Immediate',
    "status IN ('pending', 'processing')",
    'UPDATE tenant_memberships',
    'DeletionCompatibilityError::LastOwner',
    'NOT EXISTS (',
  ]) if (!snapshot.sqlite.includes(token)) errors.push(`SQLite deletion compatibility missing: ${token}`)

  for (const token of [
    'enum DeletionCompatibilityBackend',
    'Postgres(PostgresDeletionCompatibilityRepository)',
    'pub(crate) struct DeletionCompatibilityRepository',
  ]) if (!snapshot.dispatch.includes(token)) errors.push(`Deletion dispatch missing: ${token}`)

  for (const token of [
    'PostgresDeletionCompatibilityRepository',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    "status IN ('pending', 'processing')",
    "status != 'deleted' FOR UPDATE",
    'UPDATE tenant_memberships',
    'totp_enabled = FALSE',
    'DeletionCompatibilityError::LastOwner',
  ]) if (!snapshot.postgres.includes(token)) errors.push(`PostgreSQL deletion compatibility missing: ${token}`)
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL deletion compatibility must not depend on SQLite runtime types')
  }

  for (const token of [
    'pub(crate) struct DeletionCompatibilityModule',
    'require_self_service_actor(ctx.actor().id(), &input.user_id)',
    '"request"', '"list"', '"process"', '"complete"', '"reject"',
  ]) if (!snapshot.application.includes(token)) errors.push(`Deletion compatibility module missing: ${token}`)

  const metadata = between(
    snapshot.application,
    'fn commands(&self) -> Vec<CommandMetadata> {',
    '\n    fn schema(&self) -> ModuleSchema',
  )
  for (const command of ['request', 'process', 'complete', 'reject']) {
    const start = metadata.indexOf(`CommandMetadata::new(\n                "${command}",`)
    const block = start >= 0 ? metadata.slice(start, start + 280) : ''
    if (!block.includes('SimulationSupport::Blocked')) {
      errors.push(`Deletion ${command} mutation must remain blocked in Simulation`)
    }
  }
  if (!snapshot.systemDeletion.includes('pub fn require_self_service_actor(')) {
    errors.push('Legacy and compatibility deletion paths must share the actor self-service guard')
  }

  for (const token of [
    'deletion_module: Option<DeletionCompatibilityModule>',
    '(DeletionCompatibilityModule, Deletion, "deletion")',
    'pub(crate) fn with_deletion_module(',
  ]) if (!snapshot.factory.includes(token)) errors.push(`Deletion factory composition missing: ${token}`)
  const factoryBlock = between(
    snapshot.factory,
    'let deletion_built = match self.deletion_module.clone() {',
    '\n        let two_fa_built',
  )
  for (const token of [
    'Some(module) => constructed(module)',
    'None => constructed(DeletionCompatibilityModule::new(',
  ]) if (!factoryBlock.includes(token)) errors.push(`Deletion factory runtime selection missing: ${token}`)
  if (!/DeletionCompatibilityRepository::new\(\s*self\.require_sqlite_pool\(\s*"deletion compatibility"\s*\)\?\s*,?\s*\)/s.test(factoryBlock)) {
    errors.push('Deletion factory must bind the explicit SQLite capability directly to DeletionCompatibilityRepository')
  }

  for (const token of [
    'deletion_module: DeletionCompatibilityModule',
    '.with_deletion_module(deletion_module)',
  ]) if (!snapshot.assembler.includes(token)) errors.push(`Deletion assembler composition missing: ${token}`)

  for (const token of [
    'DeletionCompatibilityRepository::postgres(pg.clone())',
    'DeletionCompatibilityRepository::new(pool.clone())',
    'DeletionCompatibilityModule::new(deletion_compatibility_repository)',
    'deletion_module,',
  ]) if (!snapshot.main.includes(token)) errors.push(`Deletion production composition missing: ${token}`)
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Deletion production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Deletion cutover must not restore the retired transitional production barrier')
  }
  if (!snapshot.pgMod.includes('mod deletion_compatibility_qualification_tests;')) {
    errors.push('Deletion PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_deletion_compatibility_preserves_scope_dedup_and_owner_safety',
    'assert_eq!(created, 1);',
    'assert_eq!(existing, 1);',
    'assert!(tenant_b.is_empty());',
    'assert!(completed.identity_anonymized);',
    'Err(DeletionCompatibilityError::LastOwner)',
  ]) if (!snapshot.live.includes(token)) errors.push(`Deletion PostgreSQL live proof missing: ${token}`)
  for (const token of [
    'node scripts/check-r4-p8-deletion-compatibility-cutover.test.mjs',
    'node scripts/check-r4-p8-deletion-compatibility-cutover.mjs',
    'live_pg18_deletion_compatibility_preserves_scope_dedup_and_owner_safety',
  ]) if (!snapshot.workflow.includes(token)) errors.push(`Exact-head deletion qualification missing: ${token}`)
  return errors
}
function main() {
  const errors = validateDeletionCompatibilitySnapshot(collectDeletionCompatibilitySnapshot())
  if (errors.length) {
    console.error('R4-P8 deletion compatibility cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 deletion compatibility cutover gate passed.')
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
