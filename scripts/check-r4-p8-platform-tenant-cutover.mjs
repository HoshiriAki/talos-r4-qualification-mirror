#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/tenants.rs',
  repository: 'backend/src/repositories/platform_tenant.rs',
  dispatch: 'backend/src/repositories/platform_tenant_dispatch.rs',
  postgres: 'backend/src/repositories/platform_tenant_postgres.rs',
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectPlatformTenantSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validatePlatformTenantSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'PlatformUser',
    'PlatformCapability::TenantList',
    'PlatformCapability::TenantRead',
    'PlatformCapability::TenantCreate',
    'PlatformCapability::TenantUpdate',
    'PlatformCapability::TenantSuspend',
    'PlatformCapability::TenantDelete',
    '.platform_tenant_repository()',
    'PlatformTenantAuditActor',
  ]) {
    if (!snapshot.route.includes(token)) errors.push('Platform tenant route cutover missing: ' + token)
  }
  for (const forbidden of [
    'state.pool',
    'rusqlite',
    'audit_service',
    'SELECT id, name, slug',
    'INSERT INTO tenants',
    'UPDATE tenants SET',
    'DELETE FROM tenants',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('Platform tenant route must not retain direct persistence coupling: ' + forbidden)
    }
  }

  for (const token of [
    'SqlitePlatformTenantRepository',
    'transaction_with_behavior(TransactionBehavior::Immediate)',
    'INSERT INTO tenant_memberships',
    '"tenant.created"',
    '"tenant.updated"',
    '"tenant.status_changed"',
    '"tenant.closed"',
  ]) {
    if (!snapshot.repository.includes(token)) errors.push('SQLite platform tenant authority missing: ' + token)
  }

  for (const token of [
    'PostgresPlatformTenantRepository',
    'INSERT INTO tenant_memberships',
    'INSERT INTO audit_events',
    'platform_membership_id',
    'RETURNING id, name, slug, status, plan, settings, created_at, updated_at',
  ]) {
    if (!snapshot.postgres.includes(token)) errors.push('PostgreSQL platform tenant authority missing: ' + token)
  }

  for (const token of [
    'PlatformTenantBackend',
    'PlatformTenantRepository',
    'PostgresPlatformTenantRepository',
  ]) {
    if (!snapshot.dispatch.includes(token)) errors.push('Platform tenant backend dispatch missing: ' + token)
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'platform_tenant_repository: PlatformTenantRepository',
    'pub(crate) fn platform_tenant_repository(&self) -> &PlatformTenantRepository',
  ]) {
    if (!snapshot.state.includes(token)) errors.push('AppState platform tenant composition missing: ' + token)
  }
  if (snapshot.state.includes('set_platform_tenant_repository')) {
    errors.push('Platform tenant AppState must not restore post-construction repository override')
  }

  for (const token of [
    'PlatformTenantRepository::postgres(pg.clone())',
    'PlatformTenantRepository::new(pool.clone())',
  ]) {
    if (!snapshot.main.includes(token)) errors.push('Composition-root platform tenant authority missing: ' + token)
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,\s*platform_membership_repository,\s*platform_tenant_repository,/.test(snapshot.main)) {
    errors.push('Platform tenant AppState bundle must consume the selected PlatformTenant repository')
  }

  if (!snapshot.postgresMod.includes('mod platform_tenant_qualification_tests;')) {
    errors.push('PostgreSQL platform tenant qualification module is not registered')
  }

  for (const token of [
    'node scripts/check-r4-p8-platform-tenant-cutover.test.mjs',
    'node scripts/check-r4-p8-platform-tenant-cutover.mjs',
    'live_pg18_platform_tenant_cutover_preserves_owner_audit_lifecycle_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push('Exact-head platform tenant qualification missing: ' + token)
  }

  return errors
}

function main() {
  const errors = validatePlatformTenantSnapshot(collectPlatformTenantSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 platform tenant cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 platform tenant cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
