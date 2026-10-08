#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkSp03gOrderGetCallerCutoverBoundary } from './check-sp03g-order-get-caller-cutover-boundary.mjs'

const paths = [
  'backend/src/routes/orders.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03g-order-get-caller-cutover.md',
]

function normalizeNewlines(source) {
  return source.replace(/\r\n/g, '\n')
}

function mutateGetHandler(source, transform) {
  const startToken = 'async fn get_order_by_id('
  const endToken = 'async fn update_order('
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  assert.ok(start >= 0 && end > start, 'get handler slice must exist')
  return `${source.slice(0, start)}${transform(source.slice(start, end))}${source.slice(end)}`
}

const publicGetPattern = /\.execute\(\s*"order_read_compatibility",\s*"get_order",\s*serde_json::json!\(\{"id":\s*id\}\),\s*&ctx,?\s*\)/m

function replacePublicGetCall(source, replacement) {
  return mutateGetHandler(source, (handler) => {
    assert.ok(publicGetPattern.test(handler), 'public compatibility get call must exist')
    return handler.replace(publicGetPattern, replacement)
  })
}

const baseline = Object.fromEntries(
  paths.map((path) => [path, normalizeNewlines(readFileSync(join(process.cwd(), path), 'utf8'))]),
)
const crlfBaseline = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replace(/\n/g, '\r\n')]),
)

assert.deepEqual(checkSp03gOrderGetCallerCutoverBoundary(baseline), [])
assert.deepEqual(checkSp03gOrderGetCallerCutoverBoundary(crlfBaseline), [])

const cases = [
  {
    name: 'rejects fallback to the official get command',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => replacePublicGetCall(
      source,
      '.execute("order", "get_order", serde_json::json!({"id": id}), &ctx)',
    ),
  },
  {
    name: 'requires trusted tenant extraction',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateGetHandler(
      source,
      (handler) => handler.replace('    tenant_user: TenantUser,\n    Path(id): Path<String>,', '    Path(id): Path<String>,'),
    ),
  },
  {
    name: 'requires trusted context construction',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateGetHandler(
      source,
      (handler) => handler.replace('    let ctx = make_ctx(&user, state.http_client.clone());\n', ''),
    ),
  },
  {
    name: 'rejects a direct query-service bypass',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => mutateGetHandler(
      source,
      (handler) => handler.replace(
        '    let result = state\n',
        '    let _bypass = state.application_services.order_queries();\n    let result = state\n',
      ),
    ),
  },
  {
    name: 'requires exactly one compatibility get call',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => replacePublicGetCall(
      source,
      '.execute("order_read_compatibility", "get_order", serde_json::json!({"id": id}), &ctx)\n        .and_then(|_| state.registry.execute("order_read_compatibility", "get_order", serde_json::json!({"id": id}), &ctx))',
    ),
  },
  {
    name: 'preserves the page-list caller',
    path: 'backend/src/routes/orders.rs',
    mutate: (source) => source.replace(
      '.execute("order_read_compatibility", "list_orders", payload, &ctx)',
      '.execute("order", "list_orders", payload, &ctx)',
    ),
  },
  {
    name: 'requires package registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:order-get-cutover:test', 'quality:talos-ops:get:test'),
  },
  {
    name: 'requires the next offset boundary',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03g-order-get-caller-cutover.md',
    mutate: (source) => source.replaceAll('SP-03H', 'UNDECIDED'),
  },
]

for (const testCase of cases) {
  const files = {
    ...baseline,
    [testCase.path]: testCase.mutate(baseline[testCase.path]),
  }
  assert.ok(
    checkSp03gOrderGetCallerCutoverBoundary(files).length > 0,
    `${testCase.name}: expected failure`,
  )
}

console.log(`SP-03G cutover fixtures passed: ${cases.length} negative + CRLF parity`)
