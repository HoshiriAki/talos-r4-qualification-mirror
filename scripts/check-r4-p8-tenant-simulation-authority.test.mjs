#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectTenantSimulationSnapshot,
  validateTenantSimulationSnapshot,
} from './check-r4-p8-tenant-simulation-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectTenantSimulationSnapshot())
  mutate(snapshot)
  const errors = validateTenantSimulationSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateTenantSimulationSnapshot(collectTenantSimulationSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Tenant Simulation descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tenant_simulation" => "feature-tenant-simulation", Core, ModuleActivation::Always, NONE, TenantSimulation)',
      'descriptor!("tenant_simulation" => "feature-tenant-simulation", Core, ModuleActivation::Always, SQLITE, TenantSimulation)',
      'Tenant Simulation descriptor restores SQLite storage',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Tenant Simulation factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory += '\n// FeatureTenantSimulation::with_pool(self.pool.clone())\n'
  },
  'concrete Tenant Simulation feature ownership',
)

expectFailure(
  'one local Tenant Simulation path bypasses explicit sqlite capability',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'require_sqlite_pool("tenant simulation compatibility"',
      'legacy_sqlite_pool("tenant simulation compatibility"',
      'one local Tenant Simulation path bypasses explicit sqlite capability',
    )
  },
  'exactly both compile paths',
)

expectFailure(
  'Tenant Simulation keeps capability token but severs module binding',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'TenantSimulationCompatibilityModule::with_pool(require_sqlite_pool(',
      'TenantSimulationCompatibilityModule::with_pool(legacy_pool.clone());\n        let _ = require_sqlite_pool(',
      'Tenant Simulation keeps capability token but severs module binding',
    )
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'local Tenant Simulation restores implicit shared SQLite pool',
  snapshot => {
    snapshot.main +=
      '\n// TenantSimulationCompatibilityModule::with_pool(pool.clone())\n'
  },
  'restored implicit shared SQLite pool access',
)

expectFailure(
  'production composition loses PostgreSQL Simulation backend',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'TenantSimulationCompatibilityModule::with_postgres(pg)',
      'TenantSimulationCompatibilityModule::with_pool(pool.clone())',
      'production composition loses PostgreSQL Simulation backend',
    )
  },
  'backend selection missing',
)

expectFailure(
  'Tenant Simulation loses explicit factory injection',
  snapshot => {
    assert.ok(
      snapshot.assembler.includes('.with_tenant_simulation_module('),
      'Tenant Simulation loses explicit factory injection: mutation anchor missing',
    )
    snapshot.assembler = snapshot.assembler.replaceAll(
      '.with_tenant_simulation_module(',
      '.without_tenant_simulation_module(',
    )
  },
  'Registry assembler Tenant Simulation composition missing',
)

expectFailure(
  'Tenant Simulation assembler restores concrete Feature import',
  snapshot => {
    snapshot.assembler += '\nuse feature_tenant_simulation::FeatureTenantSimulation;\n'
  },
  'must not import or construct concrete Tenant Simulation Feature',
)

expectFailure(
  'Tenant Simulation restores stale direct pool field',
  snapshot => {
    snapshot.simulation += '\n// self.pool.lock()\n'
  },
  'stale direct pool field access',
)

expectFailure(
  'PostgreSQL Simulation loses serializable writes',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'PostgreSQL Tenant Simulation invariant missing',
)

expectFailure(
  'PostgreSQL Simulation loses actor ownership predicates',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replaceAll('actor_id=$2', 'actor_id IS NOT NULL')
  },
  'PostgreSQL Tenant Simulation invariant missing',
)

expectFailure(
  'PostgreSQL Simulation loses idempotent create conflict',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'ON CONFLICT (actor_id,idempotency_key_hash) DO NOTHING',
      'ON CONFLICT DO NOTHING',
      'PostgreSQL Simulation loses idempotent create conflict',
    )
  },
  'PostgreSQL Tenant Simulation invariant missing',
)

expectFailure(
  'PostgreSQL Simulation loses terminal evidence persistence',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replaceAll(
      'simulation_terminal_inputs',
      'removed_terminal_inputs',
    )
  },
  'PostgreSQL Tenant Simulation invariant missing',
)

expectFailure(
  'PG18 Simulation proof loses tenant isolation',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'SESSION_NOT_EXECUTABLE',
      'SIMULATION_ALLOWED',
      'PG18 Simulation proof loses tenant isolation',
    )
  },
  'PG18 Tenant Simulation evidence missing',
)

expectFailure(
  'PG18 Simulation proof loses recomposition',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'let recomposed = FeatureTenantSimulation::with_postgres(fixture.pool.clone());',
      'let recomposed = FeatureTenantSimulation::new();',
      'PG18 Simulation proof loses recomposition',
    )
  },
  'PG18 Tenant Simulation evidence missing',
)

console.log('R4-P8 Tenant Simulation authority mutation tests passed.')
