#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectBootstrapAuthoritySnapshot,
  validateBootstrapAuthoritySnapshot,
} from './check-r4-p8-bootstrap-authority.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectBootstrapAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateBootstrapAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateBootstrapAuthoritySnapshot(collectBootstrapAuthoritySnapshot())
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before bootstrap mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Production bootstrap falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'BootstrapAuthorityRepository::postgres(pg.clone())',
      'BootstrapAuthorityRepository::new(pool.clone())',
    )
  },
  'BootstrapAuthorityRepository::postgres(pg.clone())',
)

expectFailure(
  'Main regains direct SQLite bootstrap binding',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'ensure_initial_admin_with_repository(',
      'ensure_initial_admin(&pool, ',
    )
  },
  'ensure_initial_admin_with_repository(',
)

expectFailure(
  'PostgreSQL bootstrap loses SERIALIZABLE isolation',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replace(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL bootstrap loses global startup serialization',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replace(
      'LOCK TABLE identities, platform_memberships, platform_role_grants',
      'LOCK TABLE identities',
    )
  },
  'LOCK TABLE identities, platform_memberships, platform_role_grants',
)

expectFailure(
  'Live proof stops checking one-winner bootstrap',
  snapshot => {
    snapshot.live = snapshot.live.replace('assert_eq!(created, 1);', 'assert!(created > 0);')
  },
  'assert_eq!(created, 1);',
)

expectFailure(
  'Bootstrap production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Bootstrap gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 bootstrap authority cutover mutation tests passed.')
