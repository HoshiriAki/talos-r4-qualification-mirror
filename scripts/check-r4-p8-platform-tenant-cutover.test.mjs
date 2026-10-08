#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectPlatformTenantSnapshot,
  validatePlatformTenantSnapshot,
} from './check-r4-p8-platform-tenant-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectPlatformTenantSnapshot())
  mutate(snapshot)
  const errors = validatePlatformTenantSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validatePlatformTenantSnapshot(collectPlatformTenantSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'tenant route regains direct SQLite access',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::AppState) { let _ = state.pool.get(); }\n'
  },
  'state.pool',
)

expectFailure(
  'tenant create loses owner membership atomic write',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'INSERT INTO tenant_memberships',
      'INSERT INTO removed_owner_memberships',
      'tenant create loses owner membership atomic write',
    )
  },
  'INSERT INTO tenant_memberships',
)

expectFailure(
  'tenant mutation loses platform audit',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'INSERT INTO audit_events',
      'INSERT INTO removed_audit_events',
      'tenant mutation loses platform audit',
    )
  },
  'INSERT INTO audit_events',
)

expectFailure(
  'platform tenant AppState bundle stops consuming selected authority',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,\s*platform_membership_repository,\s*platform_tenant_repository,/,
      'AppStateRepositories::new(audit_compatibility_repository, auth_security_repository, identity_authority_repository, machine_authority_repository, platform_membership_repository, PlatformTenantRepository::new(pool.clone()),',
    )
  },
  'AppState bundle must consume the selected PlatformTenant repository',
)

expectFailure(
  'production composition falls back to SQLite tenant authority',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'PlatformTenantRepository::postgres(pg.clone())',
      'PlatformTenantRepository::new(pool.clone())',
      'production composition falls back to SQLite tenant authority',
    )
  },
  'PlatformTenantRepository::postgres(pg.clone())',
)

console.log('R4-P8 platform tenant cutover mutation tests passed.')
