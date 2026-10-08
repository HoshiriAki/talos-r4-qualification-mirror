#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkR1P3OrderQueryApiV2Boundary } from './check-r1p3-order-query-api-v2-boundary.mjs'

const paths = [
  'backend/src/application/order_query_v2.rs',
  'backend/src/application/order_queries.rs',
  'backend/src/application/mod.rs',
  'backend/src/repositories/order_read.rs',
  'backend/src/services/order_compatibility_support.rs',
  'backend/src/routes/orders_v2.rs',
  'backend/src/routes/orders.rs',
  'backend/src/routes/mod.rs',
  'backend/src/registry/descriptors.rs',
  'backend/src/registry/factory.rs',
  'frontend/src/api/orders.ts',
  'frontend/src/stores/orders.ts',
  'frontend/src/components/order/OrderActions.vue',
  'package.json',
  'policy/qualification/legacy-evidence/docs/README.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p3-order-query-api-v2.md',
]

function normalize(source) {
  return source.replace(/\r\n/g, '\n')
}

const baseline = Object.fromEntries(
  paths.map((path) => [path, normalize(readFileSync(join(process.cwd(), path), 'utf8'))]),
)
const crlf = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replace(/\n/g, '\r\n')]),
)

assert.deepEqual(checkR1P3OrderQueryApiV2Boundary(baseline), [])
assert.deepEqual(checkR1P3OrderQueryApiV2Boundary(crlf), [])

const cases = [
  {
    name: 'rejects missing canonical route',
    path: 'backend/src/routes/orders_v2.rs',
    mutate: (source) => source.replace('"/api/v2/orders"', '"/api/internal/orders"'),
  },
  {
    name: 'rejects public native offset',
    path: 'backend/src/routes/orders_v2.rs',
    mutate: (source) => source.replace('    page: Option<u32>,', '    offset: Option<u64>,\n    page: Option<u32>,'),
  },
  {
    name: 'requires server-owned allowed actions through the Lifecycle V2 successor',
    path: 'backend/src/application/order_query_v2.rs',
    mutate: (source) => source.replace('allowed_actions: operational.allowed_actions', 'allowed_actions: Vec::new()'),
  },
  {
    name: 'requires Registry descriptor wiring',
    path: 'backend/src/registry/descriptors.rs',
    mutate: (source) => source.replace('"order_query_v2"', '"order_query_missing"'),
  },
  {
    name: 'rejects frontend fallback to users',
    path: 'frontend/src/api/orders.ts',
    mutate: (source) => source.replace('/api/v2/orders?', '/users?'),
  },
  {
    name: 'rejects frontend offset API restoration',
    path: 'frontend/src/api/orders.ts',
    mutate: (source) => `${source}\nexport async function fetchOffset() {}\n`,
  },
  {
    name: 'rejects client transition authority',
    path: 'frontend/src/components/order/OrderActions.vue',
    mutate: (source) => source.replace('allowedActions', 'STATUS_TRANSITIONS'),
  },
  {
    name: 'rejects raw-pool legacy offset query restoration',
    path: 'backend/src/services/order_compatibility_support.rs',
    mutate: (source) => `${source}\npub fn query_orders_offset() {}\n`,
  },
  {
    name: 'requires package aggregate registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:order-query-v2', 'quality:talos-ops:order-query-v2-missing'),
  },
]

for (const testCase of cases) {
  const files = { ...baseline, [testCase.path]: testCase.mutate(baseline[testCase.path]) }
  assert.ok(
    checkR1P3OrderQueryApiV2Boundary(files).length > 0,
    `${testCase.name}: expected structural failure`,
  )
}

console.log(`R1-P3 Order Query V2 fixtures passed: ${cases.length} negative + CRLF parity`)
