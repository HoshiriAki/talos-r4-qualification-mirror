#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectWarehouseAuthoritySnapshot,
  validateWarehouseAuthoritySnapshot,
} from './check-r4-p8-warehouse-authority-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectWarehouseAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateWarehouseAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateWarehouseAuthoritySnapshot(collectWarehouseAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'warehouse route restores raw SQLite pool',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'warehouse route bypasses application boundary for repository provider',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = state.application_services().repository_provider(); }\n'
  },
  '.repository_provider()',
)

expectFailure(
  'factory restores FeatureWarehouse SQLite authority',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'WarehouseCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureWarehouse)',
    )
  },
  'WarehouseCompatibilityModule::new(repository_provider.clone())',
)

expectFailure(
  'provider hides scoped warehouse authority',
  snapshot => {
    snapshot.provider = replaceRequired(
      snapshot.provider,
      "pub fn warehouses(&self) -> ScopedWarehouseRepository<'_>",
      "fn removed_warehouses(&self) -> ScopedWarehouseRepository<'_>",
      'provider hides scoped warehouse authority',
    )
  },
  'ScopedRepositories must expose',
)

expectFailure(
  'PostgreSQL warehouse point read loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'FROM warehouses WHERE tenant_id=$1 AND id=$2 LIMIT 1',
      'FROM warehouses WHERE id=$2 LIMIT 1',
      'PostgreSQL warehouse point read loses tenant scope',
    )
  },
  'FROM warehouses WHERE tenant_id=$1 AND id=$2 LIMIT 1',
)

expectFailure(
  'SQLite 072 stops disabling foreign keys before the parent rebuild',
  snapshot => {
    snapshot.r4Migrations = replaceRequired(
      snapshot.r4Migrations,
      'conn.pragma_update(None, "foreign_keys", 0_i64)?;',
      '// foreign-key guard removed',
      'SQLite 072 stops disabling foreign keys before the parent rebuild',
    )
  },
  'pragma_update(None, "foreign_keys", 0_i64)',
)

expectFailure(
  'PostgreSQL warehouse names become globally unique again',
  snapshot => {
    snapshot.pgMigration = replaceRequired(
      snapshot.pgMigration,
      'ON warehouses(tenant_id, name);',
      'ON warehouses(name);',
      'PostgreSQL warehouse names become globally unique again',
    )
  },
  'ON warehouses(tenant_id, name)',
)

expectFailure(
  'PG18 proof loses cross-tenant same-name evidence',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(created_a.name, created_b.name)',
      'assert_ne!(created_a.name, created_b.name)',
      'PG18 proof loses cross-tenant same-name evidence',
    )
  },
  'assert_eq!(created_a.name, created_b.name)',
)

console.log('R4-P8 warehouse authority cutover mutation tests passed.')
