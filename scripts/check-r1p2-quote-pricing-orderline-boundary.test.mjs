#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

import { checkR1P2QuotePricingOrderLineBoundary } from './check-r1p2-quote-pricing-orderline-boundary.mjs'

const paths = [
  'backend/src/domain/quote.rs',
  'backend/src/db/migrations/054_quote_pricing_orderline.sql',
  'backend/src/db/migrations.rs',
  'backend/src/db/migrations/postgres/054_quote_pricing_orderline.sql',
  'backend/src/db/migrations_pg.rs',
  'backend/src/repositories/quote.rs',
  'backend/src/repositories/contracts/provider.rs',
  'backend/src/application/quote.rs',
  'backend/src/application/quote_tests.rs',
  'backend/src/routes/quotes.rs',
  'backend/src/routes/mod.rs',
  'backend/src/registry/descriptors.rs',
  'backend/src/registry/factory.rs',
  'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/README.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
]

function normalize(source) {
  return source.replaceAll('\r\n', '\n')
}

const baseline = Object.fromEntries(
  paths.map((path) => [path, normalize(readFileSync(join(process.cwd(), path), 'utf8'))]),
)
const crlfBaseline = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replaceAll('\n', '\r\n')]),
)

assert.deepEqual(checkR1P2QuotePricingOrderLineBoundary(baseline), [])
assert.deepEqual(checkR1P2QuotePricingOrderLineBoundary(crlfBaseline), [])

const cases = [
  {
    name: 'requires integer Money minor units',
    path: 'backend/src/domain/quote.rs',
    mutate: (source) => source.replace('pub minor: i64', 'pub amount: f64'),
  },
  {
    name: 'requires immutable Quote snapshot trigger',
    path: 'backend/src/db/migrations/054_quote_pricing_orderline.sql',
    mutate: (source) => source.replaceAll('confirmed quote lines are immutable', 'immutable rule removed'),
  },
  {
    name: 'requires SQLite migration registry ownership',
    path: 'backend/src/db/migrations.rs',
    mutate: (source) => source.replace('id: "054_quote_pricing_orderline"', 'id: "054_quote_removed"'),
  },
  {
    name: 'requires composite Order parent key',
    path: 'backend/src/db/migrations/054_quote_pricing_orderline.sql',
    mutate: (source) => source.replaceAll('idx_orders_id_tenant_unique', 'parent_key_removed'),
  },
  {
    name: 'requires PostgreSQL migration parity',
    path: 'backend/src/db/migrations/postgres/054_quote_pricing_orderline.sql',
    mutate: (source) => source.replaceAll('price_snapshot_json', 'snapshot_removed'),
  },
  {
    name: 'rejects raw pool ownership in Quote repository',
    path: 'backend/src/repositories/quote.rs',
    mutate: (source) => `${source}\n// forbidden fixture: Pool<SqliteConnectionManager>\n`,
  },
  {
    name: 'rejects device allocation during Quote conversion',
    path: 'backend/src/repositories/quote.rs',
    mutate: (source) => `${source}\n// forbidden fixture: INSERT INTO order_devices\n`,
  },
  {
    name: 'requires scoped Quote provider binding',
    path: 'backend/src/repositories/contracts/provider.rs',
    mutate: (source) => source.replace('pub fn quotes(&self) -> ScopedQuoteRepository', 'pub fn quote_removed(&self) -> ScopedQuoteRepository'),
  },
  {
    name: 'requires existing server Pricing authority',
    path: 'backend/src/application/quote.rs',
    mutate: (source) => source.replace('"estimate_pricing"', '"client_price"'),
  },
  {
    name: 'requires client price rejection coverage',
    path: 'backend/src/application/quote_tests.rs',
    mutate: (source) => source.replace('create_quote_rejects_client_price_fields', 'client_price_rejection_removed'),
  },
  {
    name: 'requires executable expiry coverage',
    path: 'backend/src/application/quote_tests.rs',
    mutate: (source) => source.replace('quote_expiry_is_due_only_and_blocks_accept_or_conversion', 'expiry_coverage_removed'),
  },
  {
    name: 'requires no-device conversion coverage',
    path: 'backend/src/application/quote_tests.rs',
    mutate: (source) => source.replace('SELECT COUNT(*) FROM order_devices', 'SELECT COUNT(*) FROM orders'),
  },
  {
    name: 'requires trusted Quote route context',
    path: 'backend/src/routes/quotes.rs',
    mutate: (source) => source.replaceAll('TrustedTenantUser', 'TenantUser'),
  },
  {
    name: 'requires Quote ModuleFactory identity',
    path: 'backend/src/registry/factory.rs',
    mutate: (source) => source.replace('(QuoteModule, Quote, "quote")', '(QuoteModule, Quote, "quote_removed")'),
  },
  {
    name: 'requires Quote repository and Pricing constructor wiring',
    path: 'backend/src/registry/factory.rs',
    mutate: (source) => source.replace(
      '            repository_provider.clone(),\n            pricing.clone(),',
      '            pricing.clone(),\n            pricing.clone(),',
    ),
  },
  {
    name: 'requires explicit SP-08 Quote delegation',
    path: 'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs',
    mutate: (source) => source.replaceAll('TALOS-OPS-028', 'REMOVED-QUOTE-AUTHORITY'),
  },
  {
    name: 'rejects false R1 completion',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
    mutate: (source) => `${source}\nstatus: R1_COMPLETE\n`,
  },
  {
    name: 'requires package Gate registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:quote-pricing-orderline', 'quality:talos-ops:quote-gate-removed'),
  },
]

for (const testCase of cases) {
  const files = {
    ...baseline,
    [testCase.path]: testCase.mutate(baseline[testCase.path]),
  }
  assert.ok(
    checkR1P2QuotePricingOrderLineBoundary(files).length > 0,
    `${testCase.name}: expected fail-closed finding`,
  )
}

const pricePageFixtures = spawnSync(process.execPath, ['scripts/check-r1p2-pricepage-cutover.test.mjs'], {
  cwd: process.cwd(),
  encoding: 'utf8',
})
assert.equal(pricePageFixtures.status, 0, pricePageFixtures.stdout + pricePageFixtures.stderr)

console.log(`R1-P2 Quote / Pricing / OrderLine fixtures passed: ${cases.length} negative + CRLF parity; PricePage fixtures passed`)
