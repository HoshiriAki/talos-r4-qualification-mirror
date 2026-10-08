#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  repository: 'backend/src/repositories/tenant_resolution.rs',
  dispatch: 'backend/src/repositories/tenant_resolution_dispatch.rs',
  postgres: 'backend/src/repositories/tenant_resolution_postgres.rs',
  middleware: 'backend/src/middleware/tenant.rs',
  state: 'backend/src/state.rs',
  routes: 'backend/src/routes/mod.rs',
  main: 'backend/src/main.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification: 'backend/src/repositories/postgres/tenant_resolution_qualification_tests.rs',
  exactHead: '.github/workflows/exact-head-qualification.yml',
}

function read(pathname) {
  return readFileSync(path.resolve(process.cwd(), pathname), 'utf8')
}

export function collectP8TenantResolutionSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, pathname]) => [key, read(pathname)]))
}

export function validateP8TenantResolutionSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) struct TenantResolutionRecord',
    'SqliteTenantResolutionRepository',
    'resolve_by_slug(',
    'FROM tenants WHERE slug=?1',
  ]) {
    if (!snapshot.repository.includes(token)) {
      errors.push(`SQLite tenant resolution invariant missing: ${token}`)
    }
  }

  for (const token of [
    'enum TenantResolutionBackend',
    'Sqlite(SqliteTenantResolutionRepository)',
    'Postgres(PostgresTenantResolutionRepository)',
    'pub(crate) struct TenantResolutionRepository',
    'pub(crate) fn postgres(',
    'resolve_by_slug(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Tenant resolution dispatch invariant missing: ${token}`)
    }
  }

  for (const token of [
    'PostgresTenantResolutionRepository',
    'FROM tenants WHERE slug=$1',
    'fetch_optional(&pool)',
    'TenantResolutionRecord',
    'tenant resolution requires the multi-thread runtime',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push(`PostgreSQL tenant resolution invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL tenant resolution repository must not depend on SQLite types')
  }

  for (const token of [
    'State(repository): State<TenantResolutionRepository>',
    'repository.resolve_by_slug(&tenant_slug)',
    'record.status == "active"',
    'Tenant::from(record)',
    'tenant_slug != "default"',
  ]) {
    if (!snapshot.middleware.includes(token)) {
      errors.push(`Tenant middleware repository composition missing: ${token}`)
    }
  }
  for (const forbidden of [
    'SqliteConnectionManager',
    'rusqlite::',
    'SELECT id, name, slug, status, plan, settings, created_at, updated_at',
    'get_tenant_by_slug(',
  ]) {
    if (snapshot.middleware.includes(forbidden)) {
      errors.push(`Tenant middleware still owns SQLite persistence: ${forbidden}`)
    }
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'tenant_resolution_repository: TenantResolutionRepository',
    'pub(crate) fn tenant_resolution_repository(&self) -> &TenantResolutionRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push(`Tenant resolution AppState composition missing: ${token}`)
    }
  }
  if (snapshot.state.includes('set_tenant_resolution_repository')) {
    errors.push('Tenant resolution AppState must not restore post-construction repository override')
  }

  if (!snapshot.routes.includes('state.tenant_resolution_repository().clone()')) {
    errors.push('Tenant middleware router composition must consume the AppState tenant resolution repository')
  }
  if (snapshot.routes.includes('state.pool.clone(),\n        crate::middleware::tenant::extract_tenant_middleware')) {
    errors.push('Tenant middleware router composition regressed to SQLite pool authority')
  }

  for (const token of [
    'TenantResolutionRepository::postgres(pg)',
    'TenantResolutionRepository::new(pool.clone())',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Tenant resolution production composition missing: ${token}`)
    }
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,\s*platform_membership_repository,\s*platform_tenant_repository,\s*tenant_resolution_repository,/.test(snapshot.main)) {
    errors.push('Tenant resolution AppState bundle must consume the selected TenantResolution repository')
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Tenant Resolution production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Tenant Resolution cutover must not restore the retired transitional production barrier')
  }

  if (!snapshot.pgMod.includes('mod tenant_resolution_qualification_tests;')) {
    errors.push('Tenant resolution PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_tenant_resolution_preserves_slug_status_projection_and_recomposition',
    'TenantResolutionRepository::postgres',
    'assert_eq!(active.id, "tenant-a");',
    'assert_eq!(active.status, "active");',
    'assert_eq!(suspended.status, "suspended");',
    'resolve_by_slug("missing")?.is_none()',
    'let recomposed = TenantResolutionRepository::postgres',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Tenant resolution PostgreSQL live proof missing: ${token}`)
    }
  }
  if (!snapshot.exactHead.includes(
    'live_pg18_tenant_resolution_preserves_slug_status_projection_and_recomposition',
  )) {
    errors.push('Exact-head qualification does not execute tenant resolution PostgreSQL live proof')
  }

  return errors
}

function main() {
  const errors = validateP8TenantResolutionSnapshot(collectP8TenantResolutionSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Tenant Resolution cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Tenant Resolution PostgreSQL cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
