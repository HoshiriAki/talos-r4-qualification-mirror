#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkSp03fOrderListCallerCutoverBoundary } from './check-sp03f-order-list-caller-cutover-boundary.mjs'

const paths = [
  'backend/src/routes/orders.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03f-order-list-caller-cutover.md',
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

assert.deepEqual(checkSp03fOrderListCallerCutoverBoundary(baseline), [])
assert.deepEqual(checkSp03fOrderListCallerCutoverBoundary(crlfBaseline), [])

const cases = [
  {
    name: 'requires GET users to remain mounted on list_orders without owning POST create',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace('.route("/users", get(list_orders))', '.route("/users", get(get_order_by_id))'),
  },
  {
    name: 'rejects fallback to the official page-list command',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace(
      '.execute("order_read_compatibility", "list_orders", payload, &ctx)',
      '.execute("order", "list_orders", payload, &ctx)',
    ),
  },
  {
    name: 'requires offset authority delegation',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:order-offset-cutover', 'quality:talos-ops:offset-deferred'),
  },
  {
    name: 'rejects a direct ApplicationServices bypass',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace(
      '    let result = state\n        .registry',
      '    let _bypass = state.application_services.order_queries();\n    let result = state\n        .registry',
    ),
  },
  {
    name: 'requires one page-list compatibility caller',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace(
      '.execute("order_read_compatibility", "list_orders", payload, &ctx)',
      '.execute("order_read_compatibility", "list_orders", payload, &ctx)\n        .and_then(|_| state.registry.execute("order_read_compatibility", "list_orders", payload, &ctx))',
    ),
  },
  {
    name: 'requires package validation registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:order-list-cutover:test', 'quality:talos-ops:cutover:test'),
  },
  {
    name: 'requires an explicit historical next boundary',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03f-order-list-caller-cutover.md',
    mutate: (source) => source.replaceAll('SP-03G', 'UNDECIDED'),
  },
]

for (const testCase of cases) {
  const files = {
    ...baseline,
    [testCase.path]: testCase.mutate(baseline[testCase.path]),
  }
  assert.ok(
    checkSp03fOrderListCallerCutoverBoundary(files).length > 0,
    `${testCase.name}: expected failure`,
  )
}

console.log(`SP-03F cutover fixtures passed: ${cases.length} negative + CRLF parity`)
