#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'
import { checkR1P2PricePageCutover } from './check-r1p2-pricepage-cutover.mjs'

const RULE = 'TALOS-OPS-028'
const PATHS = Object.freeze({
  domain: 'backend/src/domain/quote.rs',
  sqliteMigration: 'backend/src/db/migrations/054_quote_pricing_orderline.sql',
  sqliteRegistry: 'backend/src/db/migrations.rs',
  pgMigration: 'backend/src/db/migrations/postgres/054_quote_pricing_orderline.sql',
  pgRegistry: 'backend/src/db/migrations_pg.rs',
  repository: 'backend/src/repositories/quote.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  application: 'backend/src/application/quote.rs',
  tests: 'backend/src/application/quote_tests.rs',
  routes: 'backend/src/routes/quotes.rs',
  routesIndex: 'backend/src/routes/mod.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  delegation: 'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs',
  package: 'package.json',
  docsIndex: 'policy/qualification/legacy-evidence/docs/README.md',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(finding(path, evidence))
}

function forbidText(failures, source, token, path, evidence) {
  if (source.includes(token)) failures.push(finding(path, evidence))
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function checkR1P2QuotePricingOrderLineBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R1-P2 evidence is missing'))
  }
  if (failures.length > 0) return failures

  const domain = files[PATHS.domain]
  const sqliteMigration = files[PATHS.sqliteMigration]
  const sqliteRegistry = files[PATHS.sqliteRegistry]
  const pgMigration = files[PATHS.pgMigration]
  const pgRegistry = files[PATHS.pgRegistry]
  const repository = files[PATHS.repository]
  const provider = files[PATHS.provider]
  const application = files[PATHS.application]
  const tests = files[PATHS.tests]
  const routes = files[PATHS.routes]
  const routesIndex = files[PATHS.routesIndex]
  const descriptors = files[PATHS.descriptors]
  const factory = files[PATHS.factory]
  const delegation = files[PATHS.delegation]
  const packageJson = files[PATHS.package]
  const docsIndex = files[PATHS.docsIndex]
  const record = files[PATHS.record]

  for (const token of [
    'pub struct Money',
    'pub minor: i64',
    'pub currency: String',
    'checked_mul',
    'checked_add',
    'Uuid::new_v4().to_string()',
    'pub enum QuoteStatus',
    'pub enum QuoteLineKind',
  ]) requireText(failures, domain, token, PATHS.domain, `Quote domain missing ${token}`)
  forbidText(failures, domain, 'pub amount: f64', PATHS.domain, 'new Money contract must not persist floating-point business amounts')

  for (const [source, path] of [[sqliteMigration, PATHS.sqliteMigration], [pgMigration, PATHS.pgMigration]]) {
    for (const token of [
      'accessory_catalog',
      'quotes',
      'quote_lines',
      'order_lines',
      'source_quote_id',
      'total_minor',
      'unit_price_minor',
      'subtotal_minor',
      'price_snapshot_json',
      'idx_orders_id_tenant_unique',
      'confirmed quote lines are immutable',
    ]) requireText(failures, source, token, path, `migration missing ${token}`)
    forbidText(failures, source, 'unit_price REAL', path, 'Quote V2 unit price must use integer minor units')
    forbidText(failures, source, 'subtotal REAL', path, 'Quote V2 subtotal must use integer minor units')
  }
  requireText(failures, sqliteRegistry, 'id: "054_quote_pricing_orderline"', PATHS.sqliteRegistry, 'SQLite registry must own migration 054')
  requireText(failures, sqliteRegistry, 'migrations/054_quote_pricing_orderline.sql', PATHS.sqliteRegistry, 'SQLite registry must embed migration 054')
  requireText(failures, pgRegistry, 'id: "054_quote_pricing_orderline"', PATHS.pgRegistry, 'PostgreSQL registry must own migration 054')
  requireText(failures, pgRegistry, 'migrations/postgres/054_quote_pricing_orderline.sql', PATHS.pgRegistry, 'PostgreSQL registry must embed migration 054')

  for (const token of [
    'pub struct ScopedQuoteRepository',
    'self.session.binding().tenant_id()',
    'pub fn create(',
    'pub fn confirm(',
    'pub fn expire(',
    'pub fn create_order_from_quote(',
    'pub fn upsert_accessory(',
    'ORD-',
    'INSERT INTO order_lines',
    "status = 'converted'",
  ]) requireText(failures, repository, token, PATHS.repository, `scoped Quote repository missing ${token}`)
  forbidText(failures, repository, 'Pool<SqliteConnectionManager>', PATHS.repository, 'Quote repository must not own a raw pool')
  forbidText(failures, repository, 'INSERT INTO order_devices', PATHS.repository, 'Quote conversion must not allocate concrete devices')
  requireText(failures, provider, 'pub fn quotes(&self) -> ScopedQuoteRepository', PATHS.provider, 'ScopedRepositories must expose Quote only after bind(ctx)')

  for (const token of [
    '#[serde(rename_all = "camelCase", deny_unknown_fields)]',
    '.get("totalPrice")',
    'Money::from_major_f64',
    '"authority": "pricing.estimate_pricing"',
    '"authority": "accessory_catalog"',
    'AccessRequirement::TenantAdmin',
    'SimulationSupport::Blocked',
    'self.repositories.bind(ctx)?',
    '"create_quote"',
    '"confirm_quote"',
    '"expire_quote"',
    '"create_order_from_quote"',
  ]) requireText(failures, application, token, PATHS.application, `Quote application boundary missing ${token}`)
  requireText(
    failures,
    compact(application),
    'self.pricing.execute("estimate_pricing",pricing_payload,ctx)',
    PATHS.application,
    'Quote must invoke the existing estimate_pricing server authority',
  )
  forbidText(failures, application, 'client_total', PATHS.application, 'client total authority must not exist')
  forbidText(failures, application, 'total_minor:', PATHS.application, 'CreateQuote input must not accept a persisted client total')

  requireText(failures, compact(descriptors), 'descriptor!("quote",Business,ModuleActivation::Always,QUOTE,Quote)', PATHS.descriptors, 'descriptor catalog must register quote')
  requireText(failures, descriptors, 'ModuleRequirement::Module("pricing")', PATHS.descriptors, 'Quote descriptor must depend on Pricing authority')
  requireText(failures, factory, '(QuoteModule, Quote, "quote")', PATHS.factory, 'ModuleFactory construction identity must own Quote')
  requireText(
    failures,
    compact(factory).replaceAll(',)', ')'),
    'QuoteModule::new(repository_provider.clone(),pricing.clone())',
    PATHS.factory,
    'Quote must receive scoped repository provider and existing Pricing module',
  )

  requireText(failures, routes, 'TrustedTenantUser', PATHS.routes, 'Quote routes must use trusted tenant resolution')
  requireText(failures, routes, 'TrustedTenantAdmin', PATHS.routes, 'Accessory catalog route must require trusted tenant admin')
  requireText(failures, routes, 'tenant_user.context()', PATHS.routes, 'Quote routes must forward trusted ExecutionContext')
  requireText(failures, routes, 'tenant_admin.context()', PATHS.routes, 'Accessory route must forward trusted ExecutionContext')
  requireText(failures, routes, '"/api/v2/quotes"', PATHS.routes, 'Quote V2 route must exist')
  requireText(failures, routes, '/api/v2/quotes/{id}/expire', PATHS.routes, 'Quote expiry route must exist')
  forbidText(failures, routes, 'make_ctx(', PATHS.routes, 'Quote routes must not rebuild trusted context')
  forbidText(failures, routes, 'SELECT ', PATHS.routes, 'route-layer SQL is forbidden')
  forbidText(failures, routes, 'INSERT ', PATHS.routes, 'route-layer SQL is forbidden')
  requireText(failures, routesIndex, '.merge(quotes::quote_routes())', PATHS.routesIndex, 'Quote routes must be mounted')

  for (const token of [
    'quote_uses_server_price_minor_units_and_multiple_lines',
    'create_quote_rejects_client_price_fields',
    'confirmed_quote_snapshot_is_immutable_and_converts_once_without_device_binding',
    'quote_expiry_is_due_only_and_blocks_accept_or_conversion',
    'quote_is_tenant_scoped_preview_read_only_and_simulation_fail_closed',
    '"totalPrice": 12.34',
    '"shippingFee": 2.50',
    'SELECT COUNT(*) FROM order_devices',
  ]) requireText(failures, tests, token, PATHS.tests, `Quote tests missing ${token}`)

  requireText(failures, delegation, 'TALOS-OPS-028', PATHS.delegation, 'historical SP-08 authority must explicitly delegate Quote to TALOS-OPS-028')
  requireText(failures, delegation, 'backend/src/repositories/quote.rs', PATHS.delegation, 'delegation must name the exact Quote repository finding')

  for (const token of [
    'SERVER_PRICE_AUTHORITY_REQUIRED',
    'CLIENT_TOTAL_FORBIDDEN',
    'MONEY_MINOR_UNITS_REQUIRED',
    'QUOTE_PRICE_SNAPSHOT_IMMUTABLE',
    'MULTI_LINE_QUOTE_REQUIRED',
    'ACCESSORY_CATALOG_PRICE_REQUIRED',
    'SERVER_ORDER_NUMBER_REQUIRED',
    'NO_DEVICE_ALLOCATION_IN_QUOTE_OR_ORDER_CREATION',
    'CREATE_ORDER_FROM_CONFIRMED_QUOTE_ONLY',
    'LEGACY_PRICING_CALCULATOR_SINGLE_AUTHORITY',
    'TALOS-OPS-028',
    'PRICEPAGE_QUOTE_V2_CUTOVER',
    'LEGACY_DIRECT_ORDER_CREATE_RETIRED',
    'R1-P3',
  ]) requireText(failures, record, token, PATHS.record, `R1-P2 record missing ${token}`)
  for (const falseClaim of [
    'status: R1_COMPLETE',
    'status: ORDER_QUERY_API_V2_COMPLETE',
    'status: RESERVATION_ALLOCATION_COMPLETE',
    'status: POSTGRESQL_PARITY_COMPLETE',
    'status: SIMULATION_STORAGE_COMPLETE',
  ]) forbidText(failures, record, falseClaim, PATHS.record, `R1-P2 must not claim ${falseClaim}`)

  requireText(failures, docsIndex, 'R1-P2 Quote', PATHS.docsIndex, 'documentation index must register R1-P2')
  for (const token of [
    'quality:talos-ops:quote-pricing-orderline:test',
    'quality:talos-ops:quote-pricing-orderline',
    'check-r1p2-quote-pricing-orderline-boundary.test.mjs',
    'check-r1p2-quote-pricing-orderline-boundary.mjs',
  ]) requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)

  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function loadPricePageFiles(root) {
  const paths = [
    'frontend/src/pages/PricePage.vue',
    'frontend/src/components/price/DeviceAccessoryPanel.vue',
    'frontend/src/api/quotes.ts',
    'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
  ]
  return Object.fromEntries(paths.filter(path => existsSync(join(root, path))).map(path => [path, readFileSync(join(root, path), 'utf8')]))
}

function main() {
  const failures = [
    ...checkR1P2QuotePricingOrderLineBoundary(loadFiles(process.cwd())),
    ...checkR1P2PricePageCutover(loadPricePageFiles(process.cwd())),
  ]
  if (failures.length > 0) {
    console.error('R1-P2 Quote / Pricing / OrderLine boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R1-P2 Quote / Pricing / OrderLine boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
