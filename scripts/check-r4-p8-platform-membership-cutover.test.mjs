#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8PlatformMembershipSnapshot,
  validateP8PlatformMembershipSnapshot,
} from './check-r4-p8-platform-membership-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8PlatformMembershipSnapshot())
  mutate(snapshot)
  const errors = validateP8PlatformMembershipSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8PlatformMembershipSnapshot(
  collectP8PlatformMembershipSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Platform Membership mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Platform membership AppState bundle stops consuming selected authority',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,\s*platform_membership_repository,/,
      'AppStateRepositories::new(audit_compatibility_repository, auth_security_repository, identity_authority_repository, machine_authority_repository, PlatformMembershipRepository::new(pool.clone()),',
    )
  },
  'AppState bundle must consume the selected PlatformMembership repository',
)

expectFailure(
  'Production platform membership authority falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'PlatformMembershipRepository::postgres(pg.clone())',
      'PlatformMembershipRepository::new(pool.clone())',
    )
  },
  'PlatformMembershipRepository::postgres(pg.clone())',
)

expectFailure(
  'Platform membership route loses composed AppState repository',
  snapshot => {
    snapshot.route = snapshot.route.replaceAll(
      'platform_membership_repository()',
      'removed_platform_membership_repository()',
    )
  },
  'composed AppState repository',
)

expectFailure(
  'Platform membership route bypasses composed repository',
  snapshot => {
    snapshot.route += '\nfn regression(state: &AppState) { let _ = state.pool.get(); }\n'
  },
  'route still owns persistence',
)

expectFailure(
  'PostgreSQL adapter regains SQLite dependency',
  snapshot => {
    snapshot.postgres += '\nuse r2d2_sqlite::SqliteConnectionManager;\n'
  },
  'must not depend on SQLite types',
)

expectFailure(
  'PostgreSQL membership writes lose SERIALIZABLE isolation',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replace(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'SQLite membership writes lose atomic authority audit',
  snapshot => {
    snapshot.repository = snapshot.repository.replaceAll(
      'append_platform_audit_sqlite(',
      'removed_platform_audit_sqlite(',
    )
  },
  'append_platform_audit_sqlite(',
)

expectFailure(
  'Live proof stops checking rollback after audit failure',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'auditor_status_after_failed_audit',
      'removed_failed_audit_rollback_assertion',
    )
  },
  'auditor_status_after_failed_audit',
)

expectFailure(
  'Exact-head stops running platform membership mutation proof',
  snapshot => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'live_pg18_platform_membership_mutations_preserve_owner_and_audit_atomicity',
      'removed_platform_membership_mutation_proof',
    )
  },
  'Exact-head platform membership qualification',
)

expectFailure(
  'Platform Membership production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Platform Membership gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Platform Membership cutover mutation tests passed.')
