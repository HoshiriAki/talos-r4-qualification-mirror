#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkSp03eOrderReadCompatibilityBoundary } from './check-sp03e-order-read-compatibility-boundary.mjs'

const paths = [
  'backend/src/application/mod.rs',
  'backend/src/application/order_read_compatibility.rs',
  'backend/src/repositories/order_read.rs',
  'backend/src/registry/descriptors.rs',
  'backend/src/registry/factory.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03e-order-read-compatibility-adapter.md',
]

function normalizeNewlines(source) {
  return source.replace(/\r\n/g, '\n')
}

const baseline = Object.fromEntries(
  paths.map((path) => [
    path,
    normalizeNewlines(readFileSync(join(process.cwd(), path), 'utf8')),
  ]),
)
const crlfBaseline = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replace(/\n/g, '\r\n')]),
)

assert.deepEqual(checkSp03eOrderReadCompatibilityBoundary(baseline), [])
assert.deepEqual(checkSp03eOrderReadCompatibilityBoundary(crlfBaseline), [])

const cases = [
  {
    name: 'rejects a missing extended filter',
    path: 'backend/src/repositories/order_read.rs',
    mutate: (source) => source.replace('    pub serial_no: Option<String>,\n', ''),
  },
  {
    name: 'rejects status alias drift',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutate: (source) => source.replace(' | "active" | "进行中"', ''),
  },
  {
    name: 'requires shared official status display semantics',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutate: (source) => source.replace(
      'state_machine::display_label(persisted)',
      'persisted',
    ),
  },
  {
    name: 'requires fail-closed simulation metadata',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutate: (source) => source.replaceAll(
      'SimulationSupport::Blocked',
      'SimulationSupport::Supported',
    ),
  },
  {
    name: 'requires typed not-found transport',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutate: (source) => source.replaceAll('BIZ_ORDER_NOT_FOUND', 'BIZ_ORDER_MISSING'),
  },
  {
    name: 'requires descriptor registration',
    path: 'backend/src/registry/descriptors.rs',
    mutate: (source) => source.replaceAll('order_read_compatibility', 'order_read_adapter'),
  },
  {
    name: 'requires composition-selected repository provider injection',
    path: 'backend/src/registry/factory.rs',
    mutate: (source) => source.replace(
      'OrderReadCompatibilityModule::new(repository_provider.clone())',
      'OrderReadCompatibilityModule::new(Arc::new(SqliteRepositoryProvider::new(self.pool.clone())))',
    ),
  },
  {
    name: 'requires factory insertion',
    path: 'backend/src/registry/factory.rs',
    mutate: (source) => source.replace('        insert(order_read_compatibility_built);\n', ''),
  },
  {
    name: 'requires explicit cutover decision',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03e-order-read-compatibility-adapter.md',
    mutate: (source) => source.replace(
      'READY_FOR_BOUNDED_CALLER_CUTOVER',
      'UNDECIDED',
    ),
  },
]

for (const testCase of cases) {
  const files = {
    ...baseline,
    [testCase.path]: testCase.mutate(baseline[testCase.path]),
  }
  assert.ok(
    checkSp03eOrderReadCompatibilityBoundary(files).length > 0,
    `${testCase.name}: expected failure`,
  )
}

console.log(`SP-03E compatibility fixtures passed: ${cases.length} negative + CRLF parity`)
