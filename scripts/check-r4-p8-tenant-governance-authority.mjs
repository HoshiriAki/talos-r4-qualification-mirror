#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  application: 'backend/src/application/tenant_governance_compatibility.rs',
  sqlite: 'backend/src/repositories/tenant_governance.rs',
  postgres: 'backend/src/repositories/tenant_governance_postgres.rs',
  dispatch: 'backend/src/repositories/tenant_governance_dispatch.rs',
  repositories: 'backend/src/repositories/mod.rs',
  binding: 'backend/src/repositories/contracts/binding.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  main: 'backend/src/main.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/tenant_governance_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectTenantGovernanceSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateTenantGovernanceSnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)
  const main = compact(snapshot.main)

  for (const token of [
    'TenantGovernanceCompatibilityModule',
    'require_platform_authority(ctx)?',
    'ctx.actor().has_platform_authority()',
    'ctx.data_scope().is_platform()',
    '.tenant_list(',
    '.tenant_get(',
    '.tenant_health(',
    '.audit_query(',
    '.audit_get(',
    '.record_change_intent(',
    'redact_audit_detail',
    'FeatureTenantGovernance::new().metadata()',
    'FeatureTenantGovernance::new().commands()',
    'FeatureTenantGovernance::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Tenant Governance compatibility invariant missing: ' + token)
    }
  }
  if (snapshot.application.includes('DB_BACKEND')) {
    errors.push('Tenant Governance compatibility restored ambient DB_BACKEND selection')
  }

  for (const token of [
    'mod tenant_governance;',
    'mod tenant_governance_dispatch;',
    'mod tenant_governance_postgres;',
    'TenantGovernanceRepository',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Tenant Governance repository wiring missing: ' + token)
    }
  }
  if (!snapshot.dispatch.includes('SqliteTenantGovernanceRepository')
      || !snapshot.dispatch.includes('PostgresTenantGovernanceRepository')) {
    errors.push('Tenant Governance backend dispatch is incomplete')
  }

  for (const token of [
    'FROM tenants',
    'FROM audit_logs',
    'INSERT INTO change_intents',
    'platform_governance',
    'tenant_id=?',
    'sqlite_tenant_governance_preserves_platform_scope_redaction_health_and_intents',
  ]) {
    if (!snapshot.sqlite.includes(token) && !snapshot.application.includes(token)) {
      errors.push('SQLite Tenant Governance evidence missing: ' + token)
    }
  }

  for (const token of [
    'FROM tenants',
    'FROM audit_logs',
    '"detailJson"::jsonb',
    '"createdAt"::timestamptz',
    'pool.begin()',
    'INSERT INTO change_intents',
    'platform_governance',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL Tenant Governance invariant missing: ' + token)
    }
  }

  if (!snapshot.binding.includes('if data_scope.is_platform() || ctx.tenant_scope().is_platform()')) {
    errors.push('tenant-scoped RepositoryProvider must continue rejecting platform scope')
  }
  if (!snapshot.binding.includes('return Err(RepositoryError::TenantScopeRequired)')) {
    errors.push('platform scope rejection lost its fail-closed Repository error')
  }

  if (!factory.includes('TenantGovernanceCompatibilityModule::new(')) {
    errors.push('Registry Tenant Governance compatibility construction missing')
  }
  if (snapshot.factory.includes('FeatureTenantGovernance::with_pool')) {
    errors.push('Registry Tenant Governance restored direct Feature SQLite construction')
  }
  if (!factory.includes('with_tenant_governance_module')) {
    errors.push('ModuleFactory lacks injected Tenant Governance production composition')
  }
  if (!compact(snapshot.assembler).includes('with_tenant_governance_module(tenant_governance_module)')) {
    errors.push('Registry assembler does not forward injected Tenant Governance module')
  }
  if (!descriptors.includes(
    'descriptor!("tenant_governance"=>"feature-tenant-governance",Core,ModuleActivation::Always,NONE,TenantGovernance)',
  )) {
    errors.push('Tenant Governance descriptor must not require SQLite')
  }

  for (const token of [
    'TenantGovernanceRepository::postgres(pg.clone())',
    'TenantGovernanceRepository::new(pool.clone())',
    'TenantGovernanceCompatibilityModule::new(tenant_governance_repository)',
  ]) {
    if (!main.includes(compact(token))) {
      errors.push('composition root Tenant Governance backend selection missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod tenant_governance_qualification_tests;')) {
    errors.push('PG18 Tenant Governance qualification module is not registered')
  }
  const pgTest = compact(snapshot.pgTest)
  for (const token of [
    'live_pg18_tenant_governance_preserves_platform_scope_redaction_health_intent_and_recomposition',
    'module.execute("tenant.list"',
    'audit.query',
    'passwordHash',
    'change_intents',
    "UPDATE change_intents SET reason='rewritten'",
    'TenantGovernanceRepository::postgres(fixture.pool.clone())',
  ]) {
    if (!pgTest.includes(compact(token))) {
      errors.push('PG18 Tenant Governance evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-tenant-governance-authority.test.mjs',
    'node scripts/check-r4-p8-tenant-governance-authority.mjs',
    'live_pg18_tenant_governance_preserves_platform_scope_redaction_health_intent_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Tenant Governance qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateTenantGovernanceSnapshot(collectTenantGovernanceSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Tenant Governance authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Tenant Governance authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
