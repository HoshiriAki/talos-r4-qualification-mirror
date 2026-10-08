#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  sqlite: 'backend/src/repositories/audit_compatibility.rs',
  dispatch: 'backend/src/repositories/audit_compatibility_dispatch.rs',
  postgres: 'backend/src/repositories/audit_compatibility_postgres.rs',
  application: 'backend/src/application/audit_compatibility.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  state: 'backend/src/state.rs',
  live: 'backend/src/repositories/postgres/audit_compatibility_qualification_tests.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  systemAudit: 'backend/system/admin/src/audit.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.resolve(process.cwd(), relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

export function collectAuditCompatibilitySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateAuditCompatibilitySnapshot(snapshot) {
  const errors = []
  for (const token of [
    'SqliteAuditCompatibilityRepository',
    'INSERT INTO audit_logs',
    'SELECT COUNT(1) FROM audit_logs',
    'tenant_id = ?',
    'sqlite_sort_column(',
  ]) {
    if (!snapshot.sqlite.includes(token)) errors.push(`SQLite audit compatibility missing: ${token}`)
  }
  for (const token of [
    'AuditCompatibilityBackend',
    'Postgres(PostgresAuditCompatibilityRepository)',
    'pub(crate) struct AuditCompatibilityRepository',
  ]) {
    if (!snapshot.dispatch.includes(token)) errors.push(`Audit dispatch missing: ${token}`)
  }
  for (const token of [
    'PostgresAuditCompatibilityRepository',
    'INSERT INTO audit_logs',
    'QueryBuilder::<Postgres>',
    '\"actorIdentityId\"',
    'tenant_id = ',
  ]) {
    if (!snapshot.postgres.includes(token)) errors.push(`PostgreSQL audit compatibility missing: ${token}`)
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL audit compatibility must not depend on SQLite runtime types')
  }
  for (const token of [
    'pub(crate) struct AuditCompatibilityModule',
    'safe_audit_detail_json(&detail)',
    '.append_audit_log(&entry)',
    '.list_audit_logs(ctx.data_scope().tenant_id().as_str(), &input)',
    'name: "feature-audit"',
    '"write_audit_log"',
    '"list_audit_logs"',
  ]) {
    if (!snapshot.application.includes(token)) errors.push(`Audit compatibility module missing: ${token}`)
  }
  if (!snapshot.systemAudit.includes('pub fn safe_audit_detail_json(')) {
    errors.push('Legacy and compatibility audit paths must share the redaction helper')
  }
  for (const token of [
    'audit_module: Option<AuditCompatibilityModule>',
    '(AuditCompatibilityModule, Audit, "audit")',
    'pub(crate) fn with_audit_module(',
  ]) {
    if (!snapshot.factory.includes(token)) errors.push(`Audit factory composition missing: ${token}`)
  }
  const auditFactoryBlock = between(
    snapshot.factory,
    'let audit_built = match self.audit_module.clone() {',
    '\n        };',
  )
  for (const token of [
    'Some(module) => constructed(module)',
    'None => constructed(AuditCompatibilityModule::new(',
    'AuditCompatibilityRepository::new(self.require_sqlite_pool("audit compatibility")?)',
  ]) {
    if (!auditFactoryBlock.includes(token)) {
      errors.push(`Audit factory runtime selection missing: ${token}`)
    }
  }
  for (const token of [
    'audit_module: AuditCompatibilityModule',
    '.with_audit_module(audit_module)',
  ]) {
    if (!snapshot.assembler.includes(token)) errors.push(`Audit assembler composition missing: ${token}`)
  }
  for (const token of [
    'AuditCompatibilityRepository::postgres(pg.clone())',
    'AuditCompatibilityRepository::new(pool.clone())',
    'AuditCompatibilityModule::new(audit_compatibility_repository.clone())',
    'AppStateRepositories::new(',
    'audit_module,',
  ]) {
    if (!snapshot.main.includes(token)) errors.push(`Audit production composition missing: ${token}`)
  }
  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'audit_compatibility_repository: AuditCompatibilityRepository',
    'pub(crate) fn audit_compatibility_repository(&self) -> &AuditCompatibilityRepository',
  ]) {
    if (!snapshot.state.includes(token)) errors.push(`Audit AppState authority missing: ${token}`)
  }
  if (snapshot.state.includes('set_audit_compatibility_repository')) {
    errors.push('Audit AppState must not restore post-construction repository override')
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,/.test(snapshot.main)) {
    errors.push('Audit AppState bundle must consume the selected Audit repository')
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Audit production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Audit cutover must not restore the retired transitional production barrier')
  }
  if (!snapshot.pgMod.includes('mod audit_compatibility_qualification_tests;')) {
    errors.push('Audit PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_audit_compatibility_preserves_legacy_view_and_canonical_store',
    'SELECT authority_kind, action, resource_type',
    'assert_eq!(result.total, 1);',
    'assert_eq!(foreign.total, 0);',
  ]) {
    if (!snapshot.live.includes(token)) errors.push(`Audit PostgreSQL live proof missing: ${token}`)
  }
  for (const token of [
    'node scripts/check-r4-p8-audit-compatibility-cutover.test.mjs',
    'node scripts/check-r4-p8-audit-compatibility-cutover.mjs',
    'live_pg18_audit_compatibility_preserves_legacy_view_and_canonical_store',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push(`Exact-head audit qualification missing: ${token}`)
  }
  return errors
}

function main() {
  const errors = validateAuditCompatibilitySnapshot(collectAuditCompatibilitySnapshot())
  if (errors.length) {
    console.error('R4-P8 audit compatibility cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 audit compatibility cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
