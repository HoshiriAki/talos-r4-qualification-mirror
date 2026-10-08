#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8TenantResolutionSnapshot,
  validateP8TenantResolutionSnapshot,
} from './check-r4-p8-tenant-resolution-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8TenantResolutionSnapshot())
  mutate(snapshot)
  const errors = validateP8TenantResolutionSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8TenantResolutionSnapshot(collectP8TenantResolutionSnapshot())
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Tenant Resolution mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Tenant resolution AppState bundle stops consuming selected authority',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,\s*platform_membership_repository,\s*platform_tenant_repository,\s*tenant_resolution_repository,/,
      'AppStateRepositories::new(audit_compatibility_repository, auth_security_repository, identity_authority_repository, machine_authority_repository, platform_membership_repository, platform_tenant_repository, TenantResolutionRepository::new(pool.clone()),',
    )
  },
  'AppState bundle must consume the selected TenantResolution repository',
)

expectFailure(
  'Production tenant resolution falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'TenantResolutionRepository::postgres(pg)',
      'TenantResolutionRepository::new(pool.clone())',
    )
  },
  'TenantResolutionRepository::postgres(pg)',
)

expectFailure(
  'Router tenant middleware bypasses composed repository',
  snapshot => {
    snapshot.routes = snapshot.routes.replace(
      'state.tenant_resolution_repository().clone()',
      'state.pool.clone()',
    )
  },
  'AppState tenant resolution repository',
)

expectFailure(
  'Tenant middleware restores direct SQLite lookup',
  snapshot => {
    snapshot.middleware += '\nuse r2d2_sqlite::SqliteConnectionManager;\n'
  },
  'still owns SQLite persistence',
)

expectFailure(
  'Tenant middleware stops checking active status',
  snapshot => {
    snapshot.middleware = snapshot.middleware.replace(
      'record.status == "active"',
      'true',
    )
  },
  'record.status == "active"',
)

expectFailure(
  'Live proof stops checking missing tenant',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      'assert!(repository.resolve_by_slug("missing")?.is_none());',
      'assert!(true);',
    )
  },
  'resolve_by_slug("missing")?.is_none()',
)

expectFailure(
  'Exact-head stops running tenant resolution qualification',
  snapshot => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'live_pg18_tenant_resolution_preserves_slug_status_projection_and_recomposition',
      'removed_tenant_resolution_pg18_qualification',
    )
  },
  'Exact-head qualification',
)

expectFailure(
  'Tenant Resolution production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Tenant Resolution gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Tenant Resolution cutover mutation tests passed.')
