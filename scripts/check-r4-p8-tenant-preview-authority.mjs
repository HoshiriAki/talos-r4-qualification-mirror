#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  application: 'backend/src/application/tenant_preview_compatibility.rs',
  sqlite: 'backend/src/repositories/tenant_preview.rs',
  postgres: 'backend/src/repositories/tenant_preview_postgres.rs',
  dispatch: 'backend/src/repositories/tenant_preview_dispatch.rs',
  repositories: 'backend/src/repositories/mod.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  main: 'backend/src/main.rs',
  route: 'backend/src/routes/tenant_preview.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/tenant_preview_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectTenantPreviewSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateTenantPreviewSnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const sqlite = compact(snapshot.sqlite)
  const postgres = compact(snapshot.postgres)
  const factory = compact(snapshot.factory)
  const assembler = compact(snapshot.assembler)
  const descriptors = compact(snapshot.descriptors)
  const main = compact(snapshot.main)
  const route = compact(snapshot.route)
  const pgTest = compact(snapshot.pgTest)

  for (const token of [
    'TenantPreviewCompatibilityModule',
    'require_platform_authority(ctx)?',
    'ctx.actor().has_platform_authority()',
    'ctx.data_scope().is_platform()',
    'ctx.execution_mode()!=&ExecutionMode::Normal',
    '.create_session(command)',
    '.load_owned(',
    '.end_session(',
    '.tenant_is_active(',
    '.dashboard_summary(',
    'PREVIEW_SESSION_EXPIRED',
    'PREVIEW_SESSION_INACTIVE',
    'PREVIEW_TENANT_INACTIVE',
    'FeatureTenantPreview::new().metadata()',
    'FeatureTenantPreview::new().commands()',
    'FeatureTenantPreview::new().schema()',
    'sqlite_tenant_preview_preserves_actor_binding_modes_expiry_and_tenant_reads',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Tenant Preview compatibility invariant missing: ' + token)
    }
  }
  const platformGateCalls =
    snapshot.application.split('Self::require_platform_authority(ctx)?;').length - 1
  if (platformGateCalls < 4) {
    errors.push(
      'Tenant Preview compatibility invariant missing: require_platform_authority(ctx)? on each platform session command',
    )
  }

  if (snapshot.application.includes('DB_BACKEND')) {
    errors.push('Tenant Preview compatibility restored ambient DB_BACKEND selection')
  }

  for (const token of [
    'mod tenant_preview;',
    'mod tenant_preview_dispatch;',
    'mod tenant_preview_postgres;',
    'TenantPreviewRepository',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Tenant Preview repository wiring missing: ' + token)
    }
  }
  if (!snapshot.dispatch.includes('SqliteTenantPreviewRepository')
      || !snapshot.dispatch.includes('PostgresTenantPreviewRepository')) {
    errors.push('Tenant Preview backend dispatch is incomplete')
  }

  for (const token of [
    'TransactionBehavior::Immediate',
    'SELECT name,slug FROM tenants',
    'FROM simulation_sessions',
    'INSERT INTO tenant_preview_sessions',
    'INSERT INTO tenant_workspace_sessions',
    "UPDATE tenant_preview_sessions",
    "UPDATE tenant_workspace_sessions",
    'WHERE s.id=?1 AND s.actor_id=?2',
    'WHERE tenant_id=?1',
  ]) {
    if (!sqlite.includes(compact(token))) {
      errors.push('SQLite Tenant Preview invariant missing: ' + token)
    }
  }

  const serializableWrites =
    snapshot.postgres.split('SET TRANSACTION ISOLATION LEVEL SERIALIZABLE').length - 1
  if (serializableWrites < 2) {
    errors.push(
      'PostgreSQL Tenant Preview invariant missing: serialized create/end session writes',
    )
  }

  for (const token of [
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    "WHERE id=$1 AND status='active'",
    'FROM simulation_sessions',
    'FOR SHARE',
    'INSERT INTO tenant_preview_sessions',
    'INSERT INTO tenant_workspace_sessions',
    'FROM tenant_preview_sessions',
    'FOR UPDATE',
    "UPDATE tenant_workspace_sessions",
    'WHERE tenant_id=$1',
  ]) {
    if (!postgres.includes(compact(token))) {
      errors.push('PostgreSQL Tenant Preview invariant missing: ' + token)
    }
  }

  if (!factory.includes('TenantPreviewCompatibilityModule::new(')) {
    errors.push('Registry Tenant Preview compatibility construction missing')
  }
  if (snapshot.factory.includes('FeatureTenantPreview::with_pool')) {
    errors.push('Registry Tenant Preview restored direct Feature SQLite construction')
  }
  if (!factory.includes('with_tenant_preview_module')) {
    errors.push('ModuleFactory lacks injected Tenant Preview production composition')
  }
  if (!assembler.includes('with_tenant_preview_module(tenant_preview_module)')) {
    errors.push('Registry assembler does not forward injected Tenant Preview module')
  }
  if (!descriptors.includes(
    'descriptor!("tenant_preview"=>"feature-tenant-preview",Core,ModuleActivation::Always,NONE,TenantPreview)',
  )) {
    errors.push('Tenant Preview descriptor must not require SQLite')
  }

  for (const token of [
    'TenantPreviewRepository::postgres(pg.clone())',
    'TenantPreviewRepository::new(pool.clone())',
    'TenantPreviewCompatibilityModule::new(tenant_preview_repository)',
  ]) {
    if (!main.includes(compact(token))) {
      errors.push('composition root Tenant Preview backend selection missing: ' + token)
    }
  }

  for (const token of [
    '"session.resolve"',
    'letresolved=state.registry.execute("tenant_preview","session.resolve"',
    'require_resolved_workspace_capability(admin,&resolved)',
    '.get("tenantId")',
    'ExecutionMode::ReadOnlyPreview(',
    'DataScope::production(',
  ]) {
    if (!route.includes(compact(token))) {
      errors.push('Tenant Preview route server-owned resolution invariant missing: ' + token)
    }
  }

  const readOnlyPreviewBranches =
    snapshot.route.split('ExecutionMode::ReadOnlyPreview(').length - 1
  if (readOnlyPreviewBranches < 2) {
    errors.push(
      'Tenant Preview route server-owned resolution invariant missing: both preview and diagnostics must remain read-only',
    )
  }

  if (!snapshot.pgMod.includes('mod tenant_preview_qualification_tests;')) {
    errors.push('PG18 Tenant Preview qualification module is not registered')
  }
  for (const token of [
    'live_pg18_tenant_preview_preserves_actor_binding_workspace_modes_tenant_reads_and_recomposition',
    'workspace.create_preview',
    'session.resolve',
    'SIMULATION_NOT_FOUND',
    'dashboard.summary',
    'tenant_workspace_sessions',
    'PREVIEW_SESSION_INACTIVE',
    'TenantPreviewRepository::postgres(',
    'fixture.pool.clone()',
  ]) {
    if (!pgTest.includes(compact(token))) {
      errors.push('PG18 Tenant Preview evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-tenant-preview-authority.test.mjs',
    'node scripts/check-r4-p8-tenant-preview-authority.mjs',
    'live_pg18_tenant_preview_preserves_actor_binding_workspace_modes_tenant_reads_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Tenant Preview qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateTenantPreviewSnapshot(collectTenantPreviewSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Tenant Preview authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Tenant Preview authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
