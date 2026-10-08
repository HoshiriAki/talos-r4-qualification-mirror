#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8IdentityAuthoritySnapshot,
  validateP8IdentityAuthoritySnapshot,
} from './check-r4-p8-identity-authority-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8IdentityAuthoritySnapshot());
  mutate(snapshot);
  const errors = validateP8IdentityAuthoritySnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8IdentityAuthoritySnapshot(
  collectP8IdentityAuthoritySnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Identity Authority mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'Identity authority dispatch loses PostgreSQL backend',
  (snapshot) => {
    snapshot.dispatch = snapshot.dispatch.replaceAll(
      'PostgresIdentityAuthorityRepository::new',
      'REMOVED_IDENTITY_PG_BACKEND',
    );
  },
  'PostgresIdentityAuthorityRepository::new',
);

expectFailure(
  'Tenant login lookup loses tenant predicate',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'tm.tenant_id = $2',
      'TRUE',
    );
  },
  'tm.tenant_id = $2',
);

expectFailure(
  'Session authentication loses bearer row lock',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'FOR UPDATE OF s',
      'REMOVED_SESSION_LOCK',
    );
  },
  'FOR UPDATE OF s',
);

expectFailure(
  'Session authentication loses revoked-state filter',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      's.revoked_at IS NULL',
      'TRUE',
    );
  },
  's.revoked_at IS NULL',
);

expectFailure(
  'Expired bearer is no longer deleted',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'DELETE FROM auth_sessions WHERE token_hash = $1',
      'REMOVED_EXPIRED_SESSION_DELETE',
    );
  },
  'DELETE FROM auth_sessions',
);

expectFailure(
  'Session touch is no longer durable',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'UPDATE auth_sessions SET last_seen_at = $1 WHERE token_hash = $2',
      'REMOVED_LAST_SEEN_TOUCH',
    );
  },
  'UPDATE auth_sessions SET last_seen_at',
);

expectFailure(
  'Identity PostgreSQL adapter gains SQLite coupling',
  (snapshot) => {
    snapshot.postgresRepository += '\nuse rusqlite::Connection;\n';
  },
  'must not depend on SQLite',
);

expectFailure(
  'Auth service bypasses identity repository seam',
  (snapshot) => {
    snapshot.authService = snapshot.authService.replaceAll(
      'get_auth_user_with_repository',
      'REMOVED_AUTH_USER_REPOSITORY_SEAM',
    );
  },
  'get_auth_user_with_repository',
);

expectFailure(
  'Identity successful-login mutation disappears from PostgreSQL',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'pub(crate) fn record_successful_login(',
      'fn REMOVED_RECORD_SUCCESSFUL_LOGIN(',
    );
  },
  'record_successful_login',
);

expectFailure(
  'Auth middleware bypasses injected Identity repository',
  (snapshot) => {
    snapshot.middlewareAuth = snapshot.middlewareAuth.replaceAll(
      'state.identity_authority_repository()',
      'REMOVED_IDENTITY_REPOSITORY',
    );
  },
  'middleware composition',
);

expectFailure(
  'Auth routes lose identity repository composition',
  (snapshot) => {
    snapshot.authRoute = snapshot.authRoute.replaceAll(
      'state.identity_authority_repository()',
      'REMOVED_IDENTITY_REPOSITORY',
    );
  },
  'route composition',
);

expectFailure(
  'Live proof stops checking successful-login persistence',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(recorded_last_login.as_deref(), Some("2026-09-20T05:55:00Z"));',
      'assert!(recorded_last_login.is_some());',
    );
  },
  'recorded_last_login',
);

expectFailure(
  'Live proof stops checking profile persistence',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(updated_profile.display_name, "Alice Updated");',
      'assert!(!updated_profile.display_name.is_empty());',
    );
  },
  'updated_profile.display_name',
);

expectFailure(
  'Live proof stops checking persisted profile tuple',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      '        persisted_profile,\n        (',
      '        updated_profile.display_name,\n        (',
    );
  },
  'persisted profile tuple assertion',
);

expectFailure(
  'Live proof stops checking cross-tenant bearer rejection',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'get_auth_user(tenant_token, Some("tenant-b"))?',
      'get_auth_user(tenant_token, Some("tenant-a"))?',
    );
  },
  'tenant-b',
);

expectFailure(
  'Live proof stops checking expired bearer deletion',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(expired_remaining, 0);',
      'assert!(true);',
    );
  },
  'expired_remaining',
);

expectFailure(
  'Identity AppState bundle stops consuming selected authority',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,/,
      'AppStateRepositories::new(audit_compatibility_repository, auth_security_repository, IdentityAuthorityRepository::new(pool.clone()),',
    );
  },
  'AppState bundle must consume the selected Identity repository',
);

expectFailure(
  'Composition root stops selecting PostgreSQL IdentityAuthority',
  (snapshot) => {
    snapshot.main = snapshot.main.replaceAll(
      'IdentityAuthorityRepository::postgres(pg.clone())',
      'IdentityAuthorityRepository::new(pool.clone())',
    );
  },
  'production composition',
);

expectFailure(
  'Identity production guard disappears',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
  },
  'fail-closed PostgreSQL authority guard',
);

expectFailure(
  'Identity gate restores transitional production barrier',
  (snapshot) => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n';
  },
  'must not restore the retired transitional production barrier',
);

console.log('R4-P8 Identity Authority cutover mutation tests passed.');
