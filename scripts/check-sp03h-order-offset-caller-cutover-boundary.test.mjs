#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkSp03hOrderOffsetCallerCutoverBoundary } from './check-sp03h-order-offset-caller-cutover-boundary.mjs'

const paths = [
  'backend/src/routes/orders.rs',
  'backend/src/repositories/order_read.rs',
  'backend/src/application/order_read_compatibility.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03h-order-offset-caller-cutover.md',
]

function normalizeNewlines(source) {
  return source.replace(/\r\n/g, '\n')
}

function mutateListHandler(source, transform) {
  const startToken = 'async fn list_orders('
  const endToken = 'async fn get_order_by_id('
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  assert.ok(start >= 0 && end > start, 'list handler slice must exist')
  return `${source.slice(0, start)}${transform(source.slice(start, end))}${source.slice(end)}`
}

const baseline = Object.fromEntries(
  paths.map((path) => [path, normalizeNewlines(readFileSync(join(process.cwd(), path), 'utf8'))]),
)
const crlfBaseline = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replace(/\n/g, '\r\n')]),
)

assert.deepEqual(checkSp03hOrderOffsetCallerCutoverBoundary(baseline), [])
assert.deepEqual(checkSp03hOrderOffsetCallerCutoverBoundary(crlfBaseline), [])

const cases = [
  {
    name: 'requires GET users to remain mounted on list_orders without owning POST create',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace('.route("/users", get(list_orders))', '.route("/users", get(get_order_by_id))'),
  },
  {
    name: 'rejects fallback to the legacy raw-pool offset service',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateListHandler(source, (handler) => handler.replace(
      '.execute(\n                "order_read_compatibility",\n                "list_orders_offset",\n                payload,\n                &ctx,\n            )',
      'order_service::query_orders_offset(&state.pool, &filter, q.limit.unwrap_or(50), offset)',
    )),
  },
  {
    name: 'requires trusted tenant extraction',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateListHandler(source, (handler) => handler.replace('    tenant_user: TenantUser,\n', '')),
  },
  {
    name: 'requires trusted context construction',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateListHandler(source, (handler) => handler.replace('    let ctx = make_ctx(&user, state.http_client.clone());\n', '')),
  },
  {
    name: 'rejects page approximation of the exact offset',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateListHandler(source, (handler) => handler.replace('            "offset": offset,', '            "page": offset / q.limit.unwrap_or(50) + 1,')),
  },
  {
    name: 'requires the offset compatibility command',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace('"list_orders_offset"', '"list_orders"'),
  },
  {
    name: 'requires the typed native offset field',
    path: 'backend/src/repositories/order_read.rs',
    mutate: (source) => source.replace('    pub offset: Option<u64>,\n', ''),
  },
  {
    name: 'requires exact-offset repository selection',
    path: 'backend/src/repositories/order_read.rs',
    mutate: (source) => source.replace(
      '        let offset = request\n            .offset\n            .unwrap_or_else(|| u64::from(page - 1) * u64::from(page_size));',
      '        let offset = u64::from(page - 1) * u64::from(page_size);',
    ),
  },
  {
    name: 'requires Registry metadata and dispatch registration',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutate: (source) => source.replaceAll('"list_orders_offset"', '"offset_removed"'),
  },
  {
    name: 'requires the exact non-page-aligned semantic test',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutate: (source) => source.replace('"offset": 1,', '"offset": 2,'),
  },
  {
    name: 'requires package registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:order-offset-cutover', 'quality:talos-ops:offset-unregistered'),
  },
  {
    name: 'requires the SP-03I next boundary',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03h-order-offset-caller-cutover.md',
    mutate: (source) => source.replaceAll('SP-03I', 'UNDECIDED'),
  },
]

for (const testCase of cases) {
  const files = {
    ...baseline,
    [testCase.path]: testCase.mutate(baseline[testCase.path]),
  }
  assert.ok(
    checkSp03hOrderOffsetCallerCutoverBoundary(files).length > 0,
    `${testCase.name}: expected failure`,
  )
}

console.log(`SP-03H offset cutover fixtures passed: ${cases.length} negative + CRLF parity`)
