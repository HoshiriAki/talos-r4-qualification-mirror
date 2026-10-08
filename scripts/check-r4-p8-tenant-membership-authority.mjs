#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  sqlite: 'backend/src/repositories/tenant_membership_authority.rs',
  dispatch: 'backend/src/repositories/tenant_membership_authority_dispatch.rs',
  postgres: 'backend/src/repositories/tenant_membership_authority_postgres.rs',
  compatibility: 'backend/src/application/tenant_membership_compatibility.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  route: 'backend/src/routes/tenant_memberships.rs',
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  live: 'backend/src/repositories/postgres/tenant_membership_authority_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.resolve(process.cwd(), relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

export function collectP8TenantMembershipSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateP8TenantMembershipSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) struct TenantMembershipActor',
    'SqliteTenantMembershipAuthorityRepository',
    'TransactionBehavior::Immediate',
    'validate_actor_sqlite(',
    'would_remove_last_human_governance_sqlite(',
    'require_tenant_exclusive_identity_sqlite(',
    'append_tenant_authority_audit_sqlite(',
    'SELECT COUNT(*) FROM auth_sessions',
    'pub(crate) fn transfer_ownership(',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push(`SQLite tenant membership authority invariant missing: ${token}`)
    }
  }
  if (snapshot.sqlite.includes('UPDATE auth_sessions')) {
    errors.push('SQLite tenant password reset must rely on the migration-070 session revocation invariant')
  }

  for (const token of [
    'enum TenantMembershipAuthorityBackend',
    'Sqlite(SqliteTenantMembershipAuthorityRepository)',
    'Postgres(PostgresTenantMembershipAuthorityRepository)',
    'pub(crate) struct TenantMembershipAuthorityRepository',
    'pub(crate) fn postgres(',
    'pub(crate) fn transfer_ownership(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Tenant membership dispatch invariant missing: ${token}`)
    }
  }

  for (const token of [
    'PostgresTenantMembershipAuthorityRepository',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    "SELECT id FROM tenants WHERE id = $1 AND status != 'deleted' FOR UPDATE",
    'validate_actor_pg(',
    'would_remove_last_human_governance_pg(',
    'require_tenant_exclusive_identity_pg(',
    'append_tenant_authority_audit_pg(',
    'SELECT COUNT(*) FROM auth_sessions',
    'database.code().as_deref() == Some("40001")',
    'TenantMembershipAuthorityError::StaleOwner',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push(`PostgreSQL tenant membership authority invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL tenant membership authority must not depend on SQLite types')
  }
  if (snapshot.postgres.includes('UPDATE auth_sessions')) {
    errors.push('PostgreSQL tenant password reset must rely on the migration-070 session revocation invariant')
  }

  for (const token of [
    'pub(crate) struct TenantMembershipCompatibilityModule',
    'ExecutionMode::Normal',
    'AuthorityContext::Tenant',
    'ctx.data_scope().tenant_id_opt() != Some(tenant_id)',
    'SimulationSupport::Blocked',
    '.repository',
    '.create(&actor',
    '.reset_password(&actor',
    '.update_username(&actor',
    '.transfer_ownership(&actor',
    '"transfer_ownership"',
  ]) {
    if (!snapshot.compatibility.includes(token)) {
      errors.push(`Staff compatibility authority invariant missing: ${token}`)
    }
  }

  for (const token of [
    'staff_module: Option<TenantMembershipCompatibilityModule>',
    'pub(crate) fn with_staff_module(',
    '(TenantMembershipCompatibilityModule, Staff, "staff")',
    'insert(staff_built);',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push(`ModuleFactory staff composition invariant missing: ${token}`)
    }
  }
  const staffFactoryBlock = between(
    snapshot.factory,
    'let staff_built = match self.staff_module.clone() {',
    '\n        let excel_import_concrete',
  )
  for (const token of [
    'Some(module) => constructed(module)',
    'None => constructed(TenantMembershipCompatibilityModule::new(',
    'TenantMembershipAuthorityRepository::new(\n                    self.require_sqlite_pool("staff membership compatibility")?,',
  ]) {
    if (!staffFactoryBlock.includes(token)) {
      errors.push(`ModuleFactory staff runtime selection missing: ${token}`)
    }
  }

  for (const token of [
    'assemble_with_metrics_audit_sink_integration_and_staff_module',
    '.with_staff_module(staff_module)',
  ]) {
    if (!snapshot.assembler.includes(token)) {
      errors.push(`Registry staff composition root missing: ${token}`)
    }
  }

  const runtimeRoute = snapshot.route.split('#[cfg(test)]')[0]
  const transferStart = runtimeRoute.indexOf('async fn transfer_ownership(')
  const transferEnd = runtimeRoute.indexOf('\nasync fn list_users(', transferStart)
  const transferBody =
    transferStart >= 0 && transferEnd > transferStart
      ? runtimeRoute.slice(transferStart, transferEnd)
      : runtimeRoute
  if (!runtimeRoute.includes('MACHINE_OWNER_FORBIDDEN')) {
    errors.push('Tenant ownership route application boundary missing: MACHINE_OWNER_FORBIDDEN')
  }
  for (const token of ['.registry', '.execute(', '"staff"', '"transfer_ownership"']) {
    if (!transferBody.includes(token)) {
      errors.push(`Tenant ownership route application boundary missing: ${token}`)
    }
  }
  for (const forbidden of [
    'state.pool',
    'rusqlite',
    'BEGIN IMMEDIATE',
    'UPDATE tenant_memberships',
    'audit_service::write_authority_event',
    'crate::repositories',
  ]) {
    if (transferBody.includes(forbidden) || (forbidden === 'crate::repositories' && runtimeRoute.includes(forbidden))) {
      errors.push(`Tenant ownership transfer bypasses application boundary: ${forbidden}`)
    }
  }

  if (snapshot.state.includes('TenantMembershipAuthorityRepository')) {
    errors.push('Tenant membership repository must remain private to application composition, not AppState')
  }

  for (const token of [
    'TenantMembershipAuthorityRepository::postgres(pg.clone())',
    'TenantMembershipAuthorityRepository::new(pool.clone())',
    'TenantMembershipCompatibilityModule::new(',
    'tenant_membership_authority_repository.clone()',
    'assemble_with_metrics_audit_sink_integration_and_staff_module',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Tenant membership production composition missing: ${token}`)
    }
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Tenant Membership production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Tenant Membership cutover must not restore the retired transitional production barrier')
  }

  if (!snapshot.pgMod.includes('mod tenant_membership_authority_qualification_tests;')) {
    errors.push('Tenant membership PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_tenant_membership_authority_preserves_scope_credentials_human_governance_and_transfer',
    'live_pg18_tenant_ownership_transfer_serializes_competing_successors',
    'Err(TenantMembershipAuthorityError::SharedIdentity)',
    'Err(TenantMembershipAuthorityError::MachineOwnerForbidden)',
    'assert!(direct_machine_owner.is_err());',
    'assert_eq!(machine_owner_count, 0);',
    'assert_eq!(reset.revoked_session_count, 1);',
    'assert_eq!(remaining_sessions, 0);',
    'assert_eq!(successes, 1);',
    'assert_eq!(owner_rows.len(), 1);',
    'assert_eq!(transfer_audits, 1);',
  ]) {
    if (!snapshot.live.includes(token)) {
      errors.push(`Tenant membership PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-tenant-membership-authority.test.mjs',
    'node scripts/check-r4-p8-tenant-membership-authority.mjs',
    'live_pg18_tenant_membership_authority_preserves_scope_credentials_human_governance_and_transfer',
    'live_pg18_tenant_ownership_transfer_serializes_competing_successors',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push(`Exact-head tenant membership qualification missing: ${token}`)
    }
  }

  return errors
}

function main() {
  const errors = validateP8TenantMembershipSnapshot(collectP8TenantMembershipSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Tenant Membership authority cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Tenant Membership authority PostgreSQL cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
