#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8TenantMembershipSnapshot,
  validateP8TenantMembershipSnapshot,
} from './check-r4-p8-tenant-membership-authority.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8TenantMembershipSnapshot())
  mutate(snapshot)
  const errors = validateP8TenantMembershipSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8TenantMembershipSnapshot(
  collectP8TenantMembershipSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before tenant membership mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Production tenant membership authority falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'TenantMembershipAuthorityRepository::postgres(pg.clone())',
      'TenantMembershipAuthorityRepository::new(pool.clone())',
    )
  },
  'TenantMembershipAuthorityRepository::postgres(pg.clone())',
)

expectFailure(
  'Factory ignores injected staff authority',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'let staff_built = match self.staff_module.clone() {\n            Some(module) => constructed(module),',
      'let staff_built = match self.staff_module.clone() {\n            Some(_module) => constructed(with_pool!(FeatureStaff)),',
    )
  },
  'Some(module) => constructed(module)',
)

expectFailure(
  'Ownership transfer bypasses staff application boundary',
  snapshot => {
    snapshot.route = snapshot.route.replace(
      '.registry',
      '.pool',
    )
  },
  'Tenant ownership route application boundary',
)

expectFailure(
  'Tenant membership repository leaks back into AppState',
  snapshot => {
    snapshot.state = snapshot.state.replace(
      'application_services: Arc<ApplicationServices>,',
      'application_services: Arc<ApplicationServices>,\n    tenant_membership_authority_repository: TenantMembershipAuthorityRepository,',
    )
  },
  'must remain private to application composition',
)

expectFailure(
  'SQLite password reset stops counting sessions before credential rotation',
  snapshot => {
    snapshot.sqlite = snapshot.sqlite.replace(
      'SELECT COUNT(*) FROM auth_sessions',
      'SELECT COUNT_REMOVED(*) FROM auth_sessions',
    )
  },
  'SELECT COUNT(*) FROM auth_sessions',
)

expectFailure(
  'PostgreSQL password reset reintroduces soft session revocation',
  snapshot => {
    snapshot.postgres += '\nconst _REMOVED = "UPDATE auth_sessions";\n'
  },
  'migration-070 session revocation invariant',
)

expectFailure(
  'Live proof stops checking trigger-owned session deletion',
  snapshot => {
    snapshot.live = snapshot.live.replace(
      'assert_eq!(remaining_sessions, 0);',
      'assert_eq!(remaining_sessions, 1);',
    )
  },
  'assert_eq!(remaining_sessions, 0);',
)

expectFailure(
  'PostgreSQL governance loses SERIALIZABLE isolation',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replace(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL governance stops serializing by tenant row',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replace(
      "SELECT id FROM tenants WHERE id = $1 AND status != 'deleted' FOR UPDATE",
      "SELECT id FROM tenants WHERE id = $1 AND status != 'deleted'",
    )
  },
  'FOR UPDATE',
)

expectFailure(
  'Staff compatibility becomes simulation writable',
  snapshot => {
    snapshot.compatibility = snapshot.compatibility.replaceAll(
      'SimulationSupport::Blocked',
      'SimulationSupport::Supported',
    )
  },
  'SimulationSupport::Blocked',
)

expectFailure(
  'Live proof stops checking competing ownership successors',
  snapshot => {
    snapshot.live = snapshot.live.replace(
      'assert_eq!(successes, 1);',
      'assert!(successes > 0);',
    )
  },
  'assert_eq!(successes, 1);',
)

expectFailure(
  'Exact-head stops running tenant membership concurrency proof',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'live_pg18_tenant_ownership_transfer_serializes_competing_successors',
      'removed_tenant_ownership_concurrency_proof',
    )
  },
  'Exact-head tenant membership qualification',
)

expectFailure(
  'Tenant Membership production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Tenant Membership gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Tenant Membership authority cutover mutation tests passed.')
