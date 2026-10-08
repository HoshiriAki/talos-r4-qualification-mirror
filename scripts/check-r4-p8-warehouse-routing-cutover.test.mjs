#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectWarehouseRoutingSnapshot,
  validateWarehouseRoutingSnapshot,
} from './check-r4-p8-warehouse-routing-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectWarehouseRoutingSnapshot())
  mutate(snapshot)
  const errors = validateWarehouseRoutingSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateWarehouseRoutingSnapshot(collectWarehouseRoutingSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'pricing route restores raw SQLite warehouse routing',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = &state.pool; }\n'
  },
  'state.pool',
)

expectFailure(
  'PostgreSQL routing query loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'w.tenant_id=$1 AND r.province=$2 AND w.enabled=true',
      'r.province=$2 AND w.enabled=true',
      'PostgreSQL routing query loses tenant scope',
    )
  },
  'w.tenant_id=$1 AND r.province=$2 AND w.enabled=true',
)

expectFailure(
  'application routing stops using scoped repository rules',
  snapshot => {
    snapshot.authority = replaceRequired(
      snapshot.authority,
      '.routing_rules_for_province(province)',
      '.region_rules(province)',
      'application routing stops using scoped repository rules',
    )
  },
  '.routing_rules_for_province(province)',
)

expectFailure(
  'PG18 routing proof loses cross-tenant isolation',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      "assert!(\n            scoped_b\n                .warehouses()\n                .routing_rules_for_province(\"Shanghai\")?\n                .is_empty()\n        );",
      'let _tenant_b_routing_scope_proof_removed = true;',
      'PG18 routing proof loses cross-tenant isolation',
    )
  },
  'PG18 warehouse routing evidence missing',
)

console.log('R4-P8 warehouse routing cutover mutation tests passed.')
