#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  shared: 'backend/src/repositories/machine_authority.rs',
  dispatch: 'backend/src/repositories/machine_authority_dispatch.rs',
  sqliteRepository: 'backend/src/repositories/machine_authority_sqlite.rs',
  postgresRepository: 'backend/src/repositories/machine_authority_postgres.rs',
  service: 'backend/src/services/machine_api.rs',
  registry: 'backend/src/registry/mod.rs',
  route: 'backend/src/routes/machine_api.rs',
  state: 'backend/src/state.rs',
  liveQualification:
    'backend/src/repositories/postgres/machine_authority_qualification_tests.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  r3Gate: 'scripts/check-r3-machine-simulation-boundary.mjs',
  main: 'backend/src/main.rs',
}

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP8MachineAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8MachineAuthoritySnapshot(snapshot) {
  const errors = []

  for (const token of [
    'mod machine_authority;',
    'mod machine_authority_dispatch;',
    'mod machine_authority_postgres;',
    'mod machine_authority_sqlite;',
    'machine_authority_dispatch::MachineAuthorityRepository',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Machine authority backend-neutral wiring missing: ${token}`)
    }
  }

  for (const token of [
    'enum MachineAuthorityBackend',
    'Sqlite(SqliteMachineAuthorityRepository)',
    'Postgres(PostgresMachineAuthorityRepository)',
    'pub(crate) fn postgres(pool: sqlx::PgPool)',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Machine authority dispatch invariant missing: ${token}`)
    }
  }

  for (const token of [
    'MachineAdminContext',
    'MachineAuthorization',
    'MachineIssuedCredential',
    'MachineProvisionRecord',
    'MachineScopeRecord',
    'machine_scope_allowed',
  ]) {
    if (!snapshot.shared.includes(token)) {
      errors.push(`Machine authority shared contract missing: ${token}`)
    }
  }

  for (const token of [
    'provision_with_repository',
    'authorize_with_repository',
    'lifecycle_with_repository',
    'list_with_repository',
    'credential_lifecycle_with_repository',
    'MachineAuthorityRepository::new',
  ]) {
    if (!snapshot.service.includes(token)) {
      errors.push(`Machine service repository seam missing: ${token}`)
    }
  }
  for (const forbidden of [
    'rusqlite',
    'TransactionBehavior',
    'INSERT INTO api_',
    'UPDATE api_',
    'api_usage_windows',
  ]) {
    if (snapshot.service.includes(forbidden)) {
      errors.push(`Machine service must not own persistence token: ${forbidden}`)
    }
  }

  for (const token of [
    'TransactionBehavior::Immediate',
    'ct_eq(expected_hash.as_bytes())',
    'api_usage_windows',
    'api_scope_grants',
    'append_machine_audit',
  ]) {
    if (!snapshot.sqliteRepository.includes(token)) {
      errors.push(`SQLite machine compatibility invariant missing: ${token}`)
    }
  }

  for (const token of [
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'FOR UPDATE OF k,c,m,i',
    'ct_eq(expected_hash.as_bytes())',
    'tenant_id != requested_tenant',
    'version != api_version',
    '!scope_allowed',
    'used >= rate_limit_rpm',
    'api_usage_windows',
    'GREATEST(api_usage_windows.window_start,EXCLUDED.window_start)',
    'append_machine_audit',
    'SYS_MACHINE_PERSISTENCE',
  ]) {
    if (!snapshot.postgresRepository.includes(token)) {
      errors.push(`PostgreSQL machine authority invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresRepository)) {
    errors.push('PostgreSQL machine authority must not depend on SQLite runtime types')
  }

  if (!snapshot.pgMod.includes('mod machine_authority_qualification_tests;')) {
    errors.push('Machine authority PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_machine_authority_preserves_fresh_scope_rotation_rate_and_audit',
    'PostgresMachineAuthorityRepository::new',
    'assert!(forbidden_session.is_err());',
    'assert!(forbidden_owner.is_err());',
    'assert!(matches!(wrong_tenant, AppError::Forbidden));',
    'assert!(matches!(old_secret, AppError::Unauthorized));',
    'let recomposed =',
    'PostgresMachineAuthorityRepository::new',
    'let rate_limited = recomposed',
    'assert!(matches!(rate_limited, AppError::RateLimited { .. }));',
    'assert_eq!(usage, 2);',
    'assert!(admitted_roles.contains("staff"));',
    'assert!(!admitted_roles.contains("admin"));',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Machine authority PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'paths[MACHINE_SQLITE]',
    'paths[MACHINE_POSTGRES]',
    'policy facade must not own persistence token',
    'PostgreSQL machine authority must not depend on SQLite runtime types',
  ]) {
    if (!snapshot.r3Gate.includes(token)) {
      errors.push(`Inherited R3 machine boundary was not preserved: ${token}`)
    }
  }

  if (!snapshot.registry.includes('pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>')) {
    errors.push(
      'SQLite compatibility Registry machine entry must remain available during production recomposition',
    )
  }

  for (const token of [
    'pub(crate) fn execute_machine_with_repository(',
    'machine_api::authorize_with_repository(',
  ]) {
    if (!snapshot.registry.includes(token)) {
      errors.push(`Machine Registry repository composition seam missing: ${token}`)
    }
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'machine_authority_repository: MachineAuthorityRepository',
    'pub(crate) fn machine_authority_repository(&self) -> &MachineAuthorityRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push(`Machine authority AppState composition missing: ${token}`)
    }
  }

  if (snapshot.state.includes('set_machine_authority_repository')) {
    errors.push('Machine authority AppState must not restore post-construction repository override')
  }
  if (/\bpub\s+machine_authority_repository\s*:/.test(snapshot.state)) {
    errors.push('Machine authority AppState handle must remain private')
  }

  const compactRoute = snapshot.route.replace(/\s+/g, '')
  for (const token of [
    'machine_api::list_with_repository(state.machine_authority_repository(),',
    'machine_api::provision_with_repository(state.machine_authority_repository(),',
    'machine_api::lifecycle_with_repository(state.machine_authority_repository(),',
    'machine_api::credential_lifecycle_with_repository(state.machine_authority_repository(),',
    'state.registry.execute_machine_with_repository(state.machine_authority_repository(),',
  ]) {
    if (!compactRoute.includes(token)) {
      errors.push(`Machine route repository composition missing: ${token}`)
    }
  }
  for (const forbidden of [
    'machine_api::list(&state.pool',
    'machine_api::provision(&state.pool',
    'machine_api::lifecycle(&state.pool',
    'machine_api::credential_lifecycle(&state.pool',
    'state.registry.execute_machine(&state.pool',
  ]) {
    if (compactRoute.includes(forbidden)) {
      errors.push(`Machine route regressed to SQLite authority: ${forbidden}`)
    }
  }

  if (!snapshot.main.includes('MachineAuthorityRepository::postgres(pg.clone())')) {
    errors.push('Machine authority production composition missing: MachineAuthorityRepository::postgres(pg.clone())')
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,/.test(snapshot.main)) {
    errors.push('Machine authority AppState bundle must consume the selected Machine repository')
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Machine production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Machine cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8MachineAuthoritySnapshot(
    collectP8MachineAuthoritySnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Machine Authority cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Machine Authority PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
