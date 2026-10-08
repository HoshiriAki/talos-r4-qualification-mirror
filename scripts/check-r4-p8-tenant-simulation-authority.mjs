#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  simulation: 'backend/system/tenant-simulation/src/lib.rs',
  compatibility: 'backend/src/application/tenant_simulation_compatibility.rs',
  postgres: 'backend/system/tenant-simulation/src/postgres.rs',
  cargo: 'backend/Cargo.toml',
  simulationCargo: 'backend/system/tenant-simulation/Cargo.toml',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  main: 'backend/src/main.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/tenant_simulation_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectTenantSimulationSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateTenantSimulationSnapshot(snapshot) {
  const errors = []
  const simulation = compact(snapshot.simulation)
  const compatibility = compact(snapshot.compatibility)
  const postgres = compact(snapshot.postgres)
  const factory = compact(snapshot.factory)
  const assembler = compact(snapshot.assembler)
  const descriptors = compact(snapshot.descriptors)
  const main = compact(snapshot.main)
  const pgTest = compact(snapshot.pgTest)

  for (const token of [
    'enum SimulationBackend',
    'Sqlite(Pool<SqliteConnectionManager>)',
    'Postgres(sqlx::PgPool)',
    'pub fn with_postgres(pool: sqlx::PgPool)',
    'backend: Mutex<SimulationBackend>',
    'return postgres::create(&pool,input,ctx)',
    'return postgres::get(&pool,input,ctx)',
    'return postgres::discard(&pool,input,ctx)',
    'return postgres::diff(&pool,input,ctx)',
    'return postgres::diff_get(&pool,input,ctx)',
    'SimulationBackend::Unconfigured',
    '*backend=SimulationBackend::Sqlite(pool)',
  ]) {
    if (!simulation.includes(compact(token))) {
      errors.push('Tenant Simulation backend selector invariant missing: ' + token)
    }
  }
  if (snapshot.simulation.includes('DB_BACKEND')) {
    errors.push('Tenant Simulation restored ambient DB_BACKEND selection')
  }
  if (simulation.includes('self.pool.lock()')) {
    errors.push('Tenant Simulation retains stale direct pool field access')
  }

  for (const token of [
    'FeatureTenantSimulation::new()',
    'FeatureTenantSimulation::with_pool(pool)',
    'FeatureTenantSimulation::with_postgres(pool)',
    'self.inner.metadata()',
    'self.inner.commands()',
    'self.inner.execute(command,payload,ctx)',
    'self.inner.schema()',
  ]) {
    if (!compatibility.includes(compact(token))) {
      errors.push('Tenant Simulation compatibility wrapper invariant missing: ' + token)
    }
  }

  for (const token of [
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1 AND status='active')",
    'ON CONFLICT (actor_id,idempotency_key_hash) DO NOTHING',
    'FROM simulation_sessions',
    'actor_id=$2',
    'simulation_revision_evidence',
    'simulation_overlay_entries',
    'simulation_command_results',
    'simulation_diff_evidence',
    'simulation_terminal_inputs',
    'simulation_cleanup_jobs',
  ]) {
    if (!postgres.includes(compact(token))) {
      errors.push('PostgreSQL Tenant Simulation invariant missing: ' + token)
    }
  }

  if (!snapshot.cargo.includes('feature-tenant-simulation/postgres')) {
    errors.push('backend postgres feature must enable tenant-simulation postgres adapter')
  }
  if (!snapshot.simulationCargo.includes('postgres = ["dep:sqlx", "dep:tokio"]')) {
    errors.push('tenant-simulation crate postgres feature wiring missing')
  }

  if (!factory.includes('with_tenant_simulation_module')) {
    errors.push('ModuleFactory lacks injected Tenant Simulation composition')
  }
  if (!factory.includes('unwrap_or_else(TenantSimulationCompatibilityModule::new)')) {
    errors.push('ModuleFactory Tenant Simulation fallback must be compatibility-wrapped and storage-neutral')
  }
  if (snapshot.factory.includes('FeatureTenantSimulation')) {
    errors.push('ModuleFactory restored concrete Tenant Simulation feature ownership')
  }

  for (const token of [
    'TenantSimulationCompatibilityModule::with_pool(pool.clone())',
    'with_tenant_simulation_module(tenant_simulation_module)',
  ]) {
    if (!snapshot.assembler.includes(token)) {
      errors.push('Registry assembler Tenant Simulation composition missing: ' + token)
    }
  }

  if (snapshot.assembler.includes('FeatureTenantSimulation')) {
    errors.push('Registry assembler must not import or construct concrete Tenant Simulation Feature')
  }

  if (!descriptors.includes(
    'descriptor!("tenant_simulation"=>"feature-tenant-simulation",Core,ModuleActivation::Always,NONE,TenantSimulation)',
  )) {
    errors.push('Tenant Simulation descriptor must not require SQLite')
  }

  for (const token of [
    'TenantSimulationCompatibilityModule::with_postgres(pg)',
    'tenant_simulation_module',
  ]) {
    if (!main.includes(compact(token))) {
      errors.push('composition root Tenant Simulation backend selection missing: ' + token)
    }
  }

  const localSimulationBindingPattern =
    /TenantSimulationCompatibilityModule::with_pool\(\s*require_sqlite_pool\(\s*"tenant simulation compatibility"\s*,?\s*\)\?\s*,?\s*\)/g
  const localSimulationBindingCount =
    snapshot.main.match(localSimulationBindingPattern)?.length ?? 0
  if (localSimulationBindingCount !== 2) {
    errors.push(
      'composition root Tenant Simulation local fallback must bind the explicit SQLite capability directly in exactly both compile paths',
    )
  }
  if (main.includes(compact('TenantSimulationCompatibilityModule::with_pool(pool.clone())'))) {
    errors.push(
      'composition root Tenant Simulation local fallback restored implicit shared SQLite pool access',
    )
  }

  if (!snapshot.pgMod.includes('mod tenant_simulation_qualification_tests;')) {
    errors.push('PG18 Tenant Simulation qualification module is not registered')
  }
  for (const token of [
    'live_pg18_tenant_simulation_preserves_scope_idempotency_overlay_evidence_and_recomposition',
    'IDEMPOTENCY_CONFLICT',
    'SIMULATION_NOT_FOUND',
    'SESSION_NOT_EXECUTABLE',
    'production_label, "base"',
    'terminal_count, 1',
    'cleanup_count, 1',
    'persisted["status"], "discarded"',
    'let recomposed = FeatureTenantSimulation::with_postgres(fixture.pool.clone())',
  ]) {
    if (!pgTest.includes(compact(token))) {
      errors.push('PG18 Tenant Simulation evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-tenant-simulation-authority.test.mjs',
    'node scripts/check-r4-p8-tenant-simulation-authority.mjs',
    'live_pg18_tenant_simulation_preserves_scope_idempotency_overlay_evidence_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Tenant Simulation qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateTenantSimulationSnapshot(collectTenantSimulationSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Tenant Simulation authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Tenant Simulation authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
