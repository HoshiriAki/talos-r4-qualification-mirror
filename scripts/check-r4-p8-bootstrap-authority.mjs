#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  sqlite: 'backend/src/repositories/bootstrap_authority.rs',
  dispatch: 'backend/src/repositories/bootstrap_authority_dispatch.rs',
  postgres: 'backend/src/repositories/bootstrap_authority_postgres.rs',
  auth: 'backend/src/services/auth_service.rs',
  main: 'backend/src/main.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  live: 'backend/src/repositories/postgres/bootstrap_authority_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.resolve(process.cwd(), relative), 'utf8')
}

export function collectBootstrapAuthoritySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateBootstrapAuthoritySnapshot(snapshot) {
  const errors = []

  for (const token of [
    'SqliteBootstrapAuthorityRepository',
    'TransactionBehavior::Immediate',
    'SELECT COUNT(1) FROM identities',
    'INSERT INTO identities',
    'INSERT INTO platform_memberships',
    'INSERT INTO platform_role_grants',
    "'platform_owner'",
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push(`SQLite bootstrap authority invariant missing: ${token}`)
    }
  }

  for (const token of [
    'enum BootstrapAuthorityBackend',
    'Sqlite(SqliteBootstrapAuthorityRepository)',
    'Postgres(PostgresBootstrapAuthorityRepository)',
    'pub(crate) struct BootstrapAuthorityRepository',
    'pub(crate) fn postgres(',
    'ensure_initial_platform_owner(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Bootstrap authority dispatch invariant missing: ${token}`)
    }
  }

  for (const token of [
    'PostgresBootstrapAuthorityRepository',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'LOCK TABLE identities, platform_memberships, platform_role_grants',
    'SELECT EXISTS(SELECT 1 FROM identities)',
    'INSERT INTO identities',
    'INSERT INTO platform_memberships',
    'INSERT INTO platform_role_grants',
    "'platform_owner'",
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push(`PostgreSQL bootstrap authority invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL bootstrap authority must not depend on SQLite runtime types')
  }

  for (const token of [
    'ensure_initial_admin_with_repository(',
    'BootstrapPlatformOwnerCommand',
    'BootstrapPlatformOwnerOutcome::Created',
    'BootstrapPlatformOwnerOutcome::AlreadyInitialized',
  ]) {
    if (!snapshot.auth.includes(token)) {
      errors.push(`Auth bootstrap seam missing: ${token}`)
    }
  }

  for (const token of [
    'BootstrapAuthorityRepository::postgres(pg.clone())',
    'BootstrapAuthorityRepository::new(pool.clone())',
    'ensure_initial_admin_with_repository(',
    '&bootstrap_authority_repository',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Bootstrap production composition missing: ${token}`)
    }
  }
  if (snapshot.main.includes('ensure_initial_admin(&pool')) {
    errors.push('Main startup still binds admin bootstrap directly to SQLite')
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Bootstrap production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Bootstrap cutover must not restore the retired transitional production barrier')
  }

  if (!snapshot.pgMod.includes('mod bootstrap_authority_qualification_tests;')) {
    errors.push('Bootstrap PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_bootstrap_authority_serializes_initial_owner_creation_and_recomposition',
    'assert_eq!(created, 1);',
    'assert_eq!(skipped, 1);',
    'assert_eq!(identity_count, 1);',
    'assert_eq!(membership_count, 1);',
    'assert_eq!(owner_grant_count, 1);',
    'BootstrapPlatformOwnerOutcome::AlreadyInitialized',
  ]) {
    if (!snapshot.live.includes(token)) {
      errors.push(`Bootstrap PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-bootstrap-authority.test.mjs',
    'node scripts/check-r4-p8-bootstrap-authority.mjs',
    'live_pg18_bootstrap_authority_serializes_initial_owner_creation_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push(`Exact-head bootstrap qualification missing: ${token}`)
    }
  }

  return errors
}

function main() {
  const errors = validateBootstrapAuthoritySnapshot(collectBootstrapAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 bootstrap authority cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 bootstrap authority PostgreSQL cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
