#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  sqliteRepository: 'backend/src/repositories/auth_security.rs',
  dispatch: 'backend/src/repositories/auth_security_dispatch.rs',
  postgresRepository: 'backend/src/repositories/auth_security_postgres.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  authRateLimit: 'backend/src/services/auth_rate_limit.rs',
  sessionSecurity: 'backend/src/services/session_security.rs',
  totpLogin: 'backend/src/services/totp_login.rs',
  liveQualification:
    'backend/src/repositories/postgres/auth_security_qualification_tests.rs',
  migration070:
    'backend/src/db/migrations/postgres/070_r4_session_revocation_invariant.sql',
  state: 'backend/src/state.rs',
  routesMod: 'backend/src/routes/mod.rs',
  authRoute: 'backend/src/routes/auth.rs',
  workers: 'backend/src/application/workers.rs',
  twoFaCompatibility: 'backend/src/application/two_fa_compatibility.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8AuthSecuritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8AuthSecuritySnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'mod auth_security_dispatch;',
    'mod auth_security_postgres;',
    'auth_security_dispatch::AuthSecurityRepository',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Auth security backend-neutral wiring missing: ${token}`);
    }
  }

  if (!snapshot.sqliteRepository.includes('struct SqliteAuthSecurityRepository')) {
    errors.push('SQLite auth security implementation must remain explicitly local');
  }

  for (const token of [
    'enum AuthSecurityBackend',
    'Postgres(PostgresAuthSecurityRepository)',
    'pub(crate) fn postgres(pool: sqlx::PgPool)',
    'PostgresAuthSecurityRepository::new(pool)',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Auth security dispatch invariant missing: ${token}`);
    }
  }

  for (const token of [
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'FOR UPDATE',
    'ON CONFLICT (key_hash) DO UPDATE SET',
    'INSERT INTO auth_sessions',
    'SELECT id FROM auth_sessions WHERE identity_id = $1 FOR UPDATE',
    'UPDATE identities',
    'totp_secret_ciphertext = $1',
    'totp_enabled = TRUE',
    'install_pending_totp_secret(',
    'verify_and_enable_scoped_totp<T>(',
    'disable_scoped_totp(',
    'scoped_totp_status(',
    "tm.tenant_id = $1",
    "status != 'revoked'",
    'SYS_AUTH_PERSISTENCE',
  ]) {
    if (!snapshot.postgresRepository.includes(token)) {
      errors.push(`Auth security PostgreSQL invariant missing: ${token}`);
    }
  }

  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresRepository)) {
    errors.push('Auth security PostgreSQL adapter must not depend on SQLite runtime types');
  }

  if (!snapshot.migration070.includes('talos_revoke_sessions_on_password_change')) {
    errors.push('P7 password-rotation session-revocation trigger authority is missing');
  }
  if (!snapshot.postgresRepository.includes('migration 070 owns the revocation invariant')) {
    errors.push('PostgreSQL password rotation must explicitly preserve migration 070 authority');
  }

  for (const [source, tokens] of [
    [snapshot.authRateLimit, [
      'pub(crate) fn from_repository(repository: AuthSecurityRepository)',
      'pub(crate) fn postgres(pool: sqlx::PgPool)',
    ]],
    [snapshot.sessionSecurity, [
      'pub(crate) fn create_session_with_repository(',
      'pub(crate) fn rotate_password_and_revoke_sessions_with_repository(',
    ]],
    [snapshot.totpLogin, [
      'pub(crate) fn verify_login_totp_with_repository(',
    ]],
  ]) {
    for (const token of tokens) {
      if (!source.includes(token)) {
        errors.push(`Auth security policy injection seam missing: ${token}`);
      }
    }
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'auth_security_repository: AuthSecurityRepository',
    'identity_authority_repository: IdentityAuthorityRepository',
    'pub(crate) fn auth_security_repository(&self) -> &AuthSecurityRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push(`Auth security AppState composition missing: ${token}`);
    }
  }
  if (snapshot.state.includes('pub(crate) fn set_auth_repositories(')) {
    errors.push('Auth security AppState must not restore post-construction repository overrides');
  }

  for (const token of [
    'AuthRateLimiter::from_repository(',
    'state.auth_security_repository().clone()',
  ]) {
    if (!snapshot.routesMod.includes(token)) {
      errors.push(`Auth security router composition missing: ${token}`);
    }
  }

  for (const token of [
    'verify_login_totp_with_repository(',
    'create_session_with_repository(',
    'delete_session_with_repository(',
    'rotate_password_and_revoke_sessions_with_repository(',
    'state.auth_security_repository()',
  ]) {
    if (!snapshot.authRoute.includes(token)) {
      errors.push(`Auth security route composition missing: ${token}`);
    }
  }
  const compactAuthRoute = snapshot.authRoute.replace(/\s+/g, ' ');
  for (const forbidden of [
    'totp_login::verify_login_totp(&state.pool',
    'session_security::create_session_with_strength(&state.pool',
    'auth_service::delete_session(&state.pool',
    'session_security::rotate_password_and_revoke_sessions(&state.pool',
  ]) {
    if (compactAuthRoute.includes(forbidden)) {
      errors.push(`Auth security route regressed to SQLite authority: ${forbidden}`);
    }
  }

  for (const token of [
    'auth_security_repository: AuthSecurityRepository',
    'new_with_repositories(',
    'cleanup_expired_sessions_with_repository(',
  ]) {
    if (!snapshot.workers.includes(token)) {
      errors.push(`Auth security maintenance composition missing: ${token}`);
    }
  }

  for (const token of [
    'pub(crate) struct TwoFaCompatibilityModule',
    'install_pending_totp_secret(',
    'verify_and_enable_scoped_totp(',
    'disable_scoped_totp(',
    'scoped_totp_status(',
    'pub(crate) fn new(repository: AuthSecurityRepository)',
  ]) {
    if (!snapshot.twoFaCompatibility.includes(token)) {
      errors.push(`Two-factor compatibility module missing: ${token}`);
    }
  }
  if ((snapshot.twoFaCompatibility.match(/SimulationSupport::Blocked/g) || []).length < 3) {
    errors.push('Two-factor persistent mutations must be blocked in Simulation mode');
  }

  for (const token of [
    'two_fa_module: Option<TwoFaCompatibilityModule>',
    '(TwoFaCompatibilityModule, TwoFa, "two_fa")',
    'pub(crate) fn with_two_fa_module(',
    'let two_fa_built = match self.two_fa_module.clone()',
    'None => constructed(TwoFaCompatibilityModule::new(',
    'AuthSecurityRepository::new(',
    'insert(two_fa_built);',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push(`Two-factor factory composition missing: ${token}`);
    }
  }

  for (const token of [
    'two_fa_module: TwoFaCompatibilityModule',
    '.with_two_fa_module(two_fa_module)',
  ]) {
    if (!snapshot.assembler.includes(token)) {
      errors.push(`Two-factor assembler composition missing: ${token}`);
    }
  }

  if (!snapshot.pgMod.includes('mod auth_security_qualification_tests;')) {
    errors.push('Auth security PostgreSQL live qualification wiring missing');
  }
  for (const token of [
    'live_pg18_auth_security_preserves_rate_session_revocation_and_totp_cas',
    'AuthSecurityRepository::postgres',
    'assert_eq!(durable_rate_rows, 3);',
    'let recomposed = AuthSecurityRepository::postgres',
    'assert_eq!(remaining_sessions, 0);',
    'assert_eq!(stored_password, "new-hash");',
    'assert_eq!(rewrapped, "v1:new");',
    'TotpEnrollmentOutcome::Installed',
    'TotpEnrollmentOutcome::AlreadyEnabled',
    'TotpDisableOutcome::SharedIdentityForbidden',
    'TotpDisableOutcome::Disabled',
    'has_secret: false',
    'has_secret: true',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Auth security PostgreSQL live proof missing: ${token}`);
    }
  }

  for (const token of [
    'AuthSecurityRepository::postgres(pg.clone())',
    'TwoFaCompatibilityModule::new(auth_security_repository.clone())',
    'two_fa_module,',
    'auth_security_repository.clone()',
    'LegacyMaintenanceAdapter::new_with_repositories(',
    'auth_security_repository.clone()',
    'AppStateRepositories::new(',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Auth security production composition missing: ${token}`);
    }
  }

  if (!/LegacyMaintenanceAdapter::new_with_repositories\(\s*maintenance_compatibility_repository,\s*auth_security_repository\.clone\(\),\s*\)/.test(snapshot.main)) {
    errors.push('Auth security maintenance composition must pass auth_security_repository.clone()');
  }

  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,/.test(snapshot.main)) {
    errors.push('Auth security AppState bundle must consume the selected AuthSecurity and Identity repositories');
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Auth security production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Auth security cutover must not restore the retired transitional production barrier')
  }

  return errors;
}

function main() {
  const errors = validateP8AuthSecuritySnapshot(collectP8AuthSecuritySnapshot());
  if (errors.length > 0) {
    console.error('R4-P8 Auth/Security cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 Auth/Security PostgreSQL adapter gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
