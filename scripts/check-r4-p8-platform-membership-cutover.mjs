#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  repository: 'backend/src/repositories/platform_membership.rs',
  dispatch: 'backend/src/repositories/platform_membership_dispatch.rs',
  postgres: 'backend/src/repositories/platform_membership_postgres.rs',
  route: 'backend/src/routes/platform_memberships.rs',
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification: 'backend/src/repositories/postgres/platform_membership_qualification_tests.rs',
  exactHead: '.github/workflows/exact-head-qualification.yml',
}

function read(pathname) {
  return readFileSync(path.resolve(process.cwd(), pathname), 'utf8')
}

export function collectP8PlatformMembershipSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, pathname]) => [key, read(pathname)]))
}

export function validateP8PlatformMembershipSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) struct PlatformMembershipProjection',
    'PlatformMembershipAuditActor',
    'CreatePlatformMembership',
    'UpdatePlatformMembership',
    'RevokePlatformMembership',
    'SqlitePlatformMembershipRepository',
    'TransactionBehavior::Immediate',
    'append_platform_audit_sqlite(',
    'actor_has_active_owner_sqlite(',
    'ensure_active_owner_sqlite(',
    'pub(crate) fn create(',
    'pub(crate) fn update(',
    'pub(crate) fn revoke(',
  ]) {
    if (!snapshot.repository.includes(token)) {
      errors.push(`SQLite platform membership transaction invariant missing: ${token}`)
    }
  }

  for (const token of [
    'enum PlatformMembershipBackend',
    'Sqlite(SqlitePlatformMembershipRepository)',
    'Postgres(PostgresPlatformMembershipRepository)',
    'pub(crate) struct PlatformMembershipRepository',
    'pub(crate) fn postgres(',
    'pub(crate) fn list(',
    'pub(crate) fn create(',
    'pub(crate) fn update(',
    'pub(crate) fn revoke(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Platform membership dispatch invariant missing: ${token}`)
    }
  }

  for (const token of [
    'PostgresPlatformMembershipRepository',
    'FROM platform_memberships pm',
    'WHERE platform_membership_id = $1 ORDER BY role',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'LOCK TABLE platform_memberships, platform_role_grants IN SHARE ROW EXCLUSIVE MODE',
    'append_platform_audit_pg(',
    'actor_has_active_owner_pg(',
    'ensure_active_owner_pg(',
    'PlatformMembershipMutationError::LastActiveOwner',
    'platform membership requires the multi-thread runtime',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push(`PostgreSQL platform membership invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL platform membership repository must not depend on SQLite types')
  }

  if (!/state\s*\.\s*platform_membership_repository\s*\(\s*\)/m.test(snapshot.route)) {
    errors.push('Platform membership route must consume composed AppState repository')
  }

  for (const token of [
    '.list()',
    '.create(CreatePlatformMembership {',
    '.update(UpdatePlatformMembership {',
    '.revoke(RevokePlatformMembership {',
    'platform_membership_audit_actor(&user)?',
    'actor_is_platform_owner',
  ]) {
    if (!snapshot.route.includes(token)) {
      errors.push(`Platform membership route composition missing: ${token}`)
    }
  }
  for (const forbidden of [
    'rusqlite::',
    'state.pool',
    'audit_service::write_authority_event',
    'BEGIN IMMEDIATE',
    'SELECT pm.id',
    'INSERT INTO platform_memberships',
    'UPDATE platform_memberships',
    'DELETE FROM platform_role_grants',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push(`Platform membership route still owns persistence: ${forbidden}`)
    }
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'platform_membership_repository: PlatformMembershipRepository',
    'pub(crate) fn platform_membership_repository(&self) -> &PlatformMembershipRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push(`Platform membership AppState composition missing: ${token}`)
    }
  }
  if (snapshot.state.includes('set_platform_membership_repository')) {
    errors.push('Platform membership AppState must not restore post-construction repository override')
  }

  for (const token of [
    'PlatformMembershipRepository::postgres(pg.clone())',
    'PlatformMembershipRepository::new(pool.clone())',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Platform membership production composition missing: ${token}`)
    }
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,\s*platform_membership_repository,/.test(snapshot.main)) {
    errors.push('Platform membership AppState bundle must consume the selected PlatformMembership repository')
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Platform Membership production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Platform Membership cutover must not restore the retired transitional production barrier')
  }

  if (!snapshot.pgMod.includes('mod platform_membership_qualification_tests;')) {
    errors.push('Platform membership PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_platform_membership_read_preserves_projection_roles_and_recomposition',
    'live_pg18_platform_membership_mutations_preserve_owner_and_audit_atomicity',
    'actor_is_platform_owner: true,',
    'Err(PlatformMembershipMutationError::OwnerAuthorityRequired)',
    'Err(PlatformMembershipMutationError::LastActiveOwner)',
    'auditor_status_after_failed_audit',
    'assert_eq!(failed_owner_audits, 0);',
    'assert_eq!(revoke_audits, 1);',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Platform membership PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-platform-membership-cutover.test.mjs',
    'node scripts/check-r4-p8-platform-membership-cutover.mjs',
    'live_pg18_platform_membership_read_preserves_projection_roles_and_recomposition',
    'live_pg18_platform_membership_mutations_preserve_owner_and_audit_atomicity',
  ]) {
    if (!snapshot.exactHead.includes(token)) {
      errors.push(`Exact-head platform membership qualification missing: ${token}`)
    }
  }

  return errors
}

function main() {
  const errors = validateP8PlatformMembershipSnapshot(collectP8PlatformMembershipSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Platform Membership PostgreSQL cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Platform Membership PostgreSQL cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
