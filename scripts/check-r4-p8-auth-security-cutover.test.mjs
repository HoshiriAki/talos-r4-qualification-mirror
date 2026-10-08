#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8AuthSecuritySnapshot,
  validateP8AuthSecuritySnapshot,
} from './check-r4-p8-auth-security-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8AuthSecuritySnapshot());
  mutate(snapshot);
  const errors = validateP8AuthSecuritySnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8AuthSecuritySnapshot(collectP8AuthSecuritySnapshot());
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Auth/Security mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'Auth security dispatch loses PostgreSQL backend',
  (snapshot) => {
    snapshot.dispatch = snapshot.dispatch.replaceAll(
      'PostgresAuthSecurityRepository::new(pool)',
      'REMOVED_AUTH_PG_BACKEND',
    );
  },
  'PostgresAuthSecurityRepository::new(pool)',
);

expectFailure(
  'Auth rate mutations lose serialized ownership',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    );
  },
  'SERIALIZABLE',
);

expectFailure(
  'Auth rate state loses conflict-safe upsert',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'ON CONFLICT (key_hash) DO UPDATE SET',
      'REMOVED_RATE_UPSERT',
    );
  },
  'ON CONFLICT (key_hash) DO UPDATE SET',
);

expectFailure(
  'Password rotation stops locking prior sessions',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'SELECT id FROM auth_sessions WHERE identity_id = $1 FOR UPDATE',
      'SELECT id FROM auth_sessions WHERE identity_id = $1',
    );
  },
  'FOR UPDATE',
);

expectFailure(
  'Password rotation forgets migration 070 authority',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'migration 070 owns the revocation invariant',
      'local deletion owns revocation',
    );
  },
  'migration 070 authority',
);

expectFailure(
  'TOTP rewrap loses enabled-state CAS',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'totp_enabled = TRUE',
      'TRUE',
    );
  },
  'totp_enabled = TRUE',
);

expectFailure(
  'PostgreSQL auth adapter gains SQLite coupling',
  (snapshot) => {
    snapshot.postgresRepository += '\nuse rusqlite::Connection;\n';
  },
  'must not depend on SQLite',
);

expectFailure(
  'Auth rate policy loses backend-neutral repository injection',
  (snapshot) => {
    snapshot.authRateLimit = snapshot.authRateLimit.replaceAll(
      'pub(crate) fn from_repository(repository: AuthSecurityRepository)',
      'fn REMOVED_AUTH_RATE_REPOSITORY_INJECTION',
    );
  },
  'from_repository',
);

expectFailure(
  'Session policy loses backend-neutral creation seam',
  (snapshot) => {
    snapshot.sessionSecurity = snapshot.sessionSecurity.replaceAll(
      'pub(crate) fn create_session_with_repository(',
      'fn REMOVED_SESSION_REPOSITORY_INJECTION(',
    );
  },
  'create_session_with_repository',
);

expectFailure(
  'TOTP policy loses backend-neutral repository seam',
  (snapshot) => {
    snapshot.totpLogin = snapshot.totpLogin.replaceAll(
      'pub(crate) fn verify_login_totp_with_repository(',
      'fn REMOVED_TOTP_REPOSITORY_INJECTION(',
    );
  },
  'verify_login_totp_with_repository',
);

expectFailure(
  'Router bypasses AppState AuthSecurity composition',
  (snapshot) => {
    snapshot.routesMod = snapshot.routesMod.replaceAll(
      'state.auth_security_repository().clone()',
      'AuthSecurityRepository::new(other_pool.clone())',
    );
  },
  'router composition',
);

expectFailure(
  'Auth routes lose repository-backed session security',
  (snapshot) => {
    snapshot.authRoute = snapshot.authRoute.replaceAll(
      'state.auth_security_repository()',
      'REMOVED_AUTH_SECURITY_REPOSITORY',
    );
  },
  'route composition',
);

expectFailure(
  'Maintenance cleanup bypasses AuthSecurity repository',
  (snapshot) => {
    snapshot.workers = snapshot.workers.replaceAll(
      'cleanup_expired_sessions_with_repository(',
      'cleanup_expired_sessions(',
    );
  },
  'maintenance composition',
);

expectFailure(
  'Live proof stops checking session revocation',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(remaining_sessions, 0);',
      'assert!(true);',
    );
  },
  'remaining_sessions',
);

expectFailure(
  'Live proof stops checking TOTP CAS result',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(rewrapped, "v1:new");',
      'assert!(true);',
    );
  },
  'rewrapped',
);

expectFailure(
  'Composition root stops selecting PostgreSQL AuthSecurity',
  (snapshot) => {
    snapshot.main = snapshot.main.replaceAll(
      'AuthSecurityRepository::postgres(pg.clone())',
      'AuthSecurityRepository::new(pool.clone())',
    );
  },
  'production composition',
);

expectFailure(
  'Maintenance composition stops consuming selected AuthSecurity repository',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'auth_security_repository.clone(),',
      'AuthSecurityRepository::new(pool.clone()),',
    );
  },
  'auth_security_repository.clone()',
);

expectFailure(
  'Two-factor PostgreSQL lifecycle loses tenant membership scope',
  (snapshot) => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'tm.tenant_id = $1',
      'tm.tenant_id IS NOT NULL',
    );
  },
  'tm.tenant_id = $1',
);

expectFailure(
  'Two-factor persistent mutations become simulation-writable',
  (snapshot) => {
    snapshot.twoFaCompatibility = snapshot.twoFaCompatibility.replace(
      'SimulationSupport::Blocked',
      'SimulationSupport::Supported',
    );
  },
  'blocked in Simulation mode',
);

expectFailure(
  'Two-factor factory ignores injected compatibility module',
  (snapshot) => {
    snapshot.factory = snapshot.factory.replace(
      'let two_fa_built = match self.two_fa_module.clone()',
      'let two_fa_built = match None::<TwoFaCompatibilityModule>',
    );
  },
  'let two_fa_built = match self.two_fa_module.clone()',
);

expectFailure(
  'Composition root stops injecting selected AuthSecurity into two-factor module',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'TwoFaCompatibilityModule::new(auth_security_repository.clone())',
      'TwoFaCompatibilityModule::new(AuthSecurityRepository::new(pool.clone()))',
    );
  },
  'TwoFaCompatibilityModule::new(auth_security_repository.clone())',
);

expectFailure(
  'Live proof stops checking shared-identity MFA protection',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      'TotpDisableOutcome::SharedIdentityForbidden',
      'TotpDisableOutcome::Disabled',
    );
  },
  'TotpDisableOutcome::SharedIdentityForbidden',
);

expectFailure(
  'Auth security production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Auth security gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

expectFailure(
  'AppState bundle stops consuming selected AuthSecurity repository',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,/,
      'AppStateRepositories::new(audit_compatibility_repository, AuthSecurityRepository::new(pool.clone()),',
    );
  },
  'AppState bundle must consume the selected AuthSecurity',
);

console.log('R4-P8 Auth/Security cutover mutation tests passed.');
