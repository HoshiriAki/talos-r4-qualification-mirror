#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  sqliteRepository: 'backend/src/repositories/identity_authority.rs',
  dispatch: 'backend/src/repositories/identity_authority_dispatch.rs',
  postgresRepository: 'backend/src/repositories/identity_authority_postgres.rs',
  authService: 'backend/src/services/auth_service.rs',
  state: 'backend/src/state.rs',
  authRoute: 'backend/src/routes/auth.rs',
  middlewareAuth: 'backend/src/middleware/auth.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/identity_authority_qualification_tests.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8IdentityAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8IdentityAuthoritySnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'mod identity_authority;',
    'mod identity_authority_dispatch;',
    'mod identity_authority_postgres;',
    'identity_authority_dispatch::IdentityAuthorityRepository',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Identity authority backend-neutral wiring missing: ${token}`);
    }
  }

  if (!snapshot.sqliteRepository.includes('struct SqliteIdentityAuthorityRepository')) {
    errors.push('SQLite identity authority implementation must remain explicitly local');
  }

  for (const token of [
    'enum IdentityAuthorityBackend',
    'Postgres(PostgresIdentityAuthorityRepository)',
    'pub(crate) fn postgres(pool: sqlx::PgPool)',
    'PostgresIdentityAuthorityRepository::new',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Identity authority dispatch invariant missing: ${token}`);
    }
  }

  for (const token of [
    'tm.tenant_id = $2',
    "tm.status = 'active'",
    "pm.status = 'active'",
    'platform_role_grants',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'FOR UPDATE OF s',
    's.revoked_at IS NULL',
    'DELETE FROM auth_sessions WHERE token_hash = $1',
    'UPDATE auth_sessions SET last_seen_at = $1 WHERE token_hash = $2',
    'pub(crate) fn record_successful_login(',
    'SET last_login_at = $1, updated_at = $1',
    'pub(crate) fn update_identity_profile(',
    'RETURNING id, username, password_hash, status, display_name',
    'SYS_IDENTITY_PERSISTENCE',
  ]) {
    if (!snapshot.postgresRepository.includes(token)) {
      errors.push(`Identity authority PostgreSQL invariant missing: ${token}`);
    }
  }

  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresRepository)) {
    errors.push('Identity authority PostgreSQL adapter must not depend on SQLite runtime types');
  }

  for (const token of [
    'find_user_by_username_with_repository',
    'find_platform_identity_by_username_with_repository',
    'find_user_by_id_with_repository',
    'find_identity_by_id_with_repository',
    'get_auth_user_with_repository',
    'record_successful_login_with_repository',
    'update_identity_profile_with_repository',
  ]) {
    if (!snapshot.authService.includes(token)) {
      errors.push(`Auth service identity repository seam missing: ${token}`);
    }
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'identity_authority_repository: IdentityAuthorityRepository',
    'pub(crate) fn identity_authority_repository(&self) -> &IdentityAuthorityRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push(`Identity authority AppState composition missing: ${token}`);
    }
  }
  if (snapshot.state.includes('set_auth_repositories')) {
    errors.push('Identity authority AppState must not restore post-construction repository overrides');
  }

  for (const token of [
    'get_auth_user_with_repository(',
    'state.identity_authority_repository()',
  ]) {
    if (!snapshot.middlewareAuth.includes(token)) {
      errors.push(`Identity authority middleware composition missing: ${token}`);
    }
  }

  for (const token of [
    'find_platform_identity_by_username_with_repository(',
    'find_user_by_username_with_repository(',
    'get_auth_user_with_repository(',
    'record_successful_login_with_repository(',
    'update_identity_profile_with_repository(',
    'find_identity_by_id_with_repository(',
    'state.identity_authority_repository()',
  ]) {
    if (!snapshot.authRoute.includes(token)) {
      errors.push(`Identity authority route composition missing: ${token}`);
    }
  }
  const compactIdentityRoute = snapshot.authRoute.replace(/\s+/g, ' ');
  for (const forbidden of [
    'auth_service::get_auth_user(&state.pool',
    'auth_service::find_user_by_username(&state.pool',
    'auth_service::find_platform_identity_by_username(&state.pool',
    'auth_service::find_identity_by_id(&state.pool',
    'auth_service::update_identity_profile(&state.pool',
    'UPDATE identities SET last_login_at',
  ]) {
    if (compactIdentityRoute.includes(forbidden)) {
      errors.push(`Identity authority route regressed to SQLite authority: ${forbidden}`);
    }
  }

  if (!snapshot.pgMod.includes('mod identity_authority_qualification_tests;')) {
    errors.push('Identity authority PostgreSQL live qualification wiring missing');
  }

  for (const token of [
    'live_pg18_identity_authority_preserves_scope_roles_sessions_and_expiry',
    'IdentityAuthorityRepository::postgres',
    'find_user_by_username(&scope("tenant-b"), "alice")?',
    'get_auth_user(tenant_token, Some("tenant-b"))?',
    'platform_roles()',
    'assert_ne!(tenant_last_seen, "2000-01-01T00:00:00Z");',
    'repository.record_successful_login(',
    'assert_eq!(recorded_last_login.as_deref(), Some("2026-09-20T05:55:00Z"));',
    'let updated_profile = repository.update_identity_profile(',
    'assert_eq!(updated_profile.display_name, "Alice Updated");',
    'assert_eq!(expired_remaining, 0);',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Identity authority PostgreSQL live proof missing: ${token}`);
    }
  }

  if (!/assert_eq!\(\s*persisted_profile,\s*\(\s*"Alice Updated"\.to_string\(\),\s*"alice\.updated@example\.test"\.to_string\(\),\s*"\+1-555-0100"\.to_string\(\),\s*"2026-09-20T05:56:00Z"\.to_string\(\),?\s*\)\s*\);/.test(snapshot.liveQualification)) {
    errors.push('Identity authority PostgreSQL live proof missing persisted profile tuple assertion');
  }

  if (!snapshot.main.includes('IdentityAuthorityRepository::postgres(pg.clone())')) {
    errors.push('Identity authority production composition missing: IdentityAuthorityRepository::postgres(pg.clone())');
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,/.test(snapshot.main)) {
    errors.push('Identity authority AppState bundle must consume the selected Identity repository');
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Identity production cutover must retain the fail-closed PostgreSQL authority guard');
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Identity cutover must not restore the retired transitional production barrier');
  }

  return errors;
}

function main() {
  const errors = validateP8IdentityAuthoritySnapshot(
    collectP8IdentityAuthoritySnapshot(),
  );
  if (errors.length > 0) {
    console.error('R4-P8 Identity Authority cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 Identity Authority PostgreSQL cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
