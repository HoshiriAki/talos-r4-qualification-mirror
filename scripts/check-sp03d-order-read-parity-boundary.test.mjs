#!/usr/bin/env node

import assert from 'node:assert/strict'

import { checkSp03dOrderReadParityBoundary } from './check-sp03d-order-read-parity-boundary.mjs'

const filters = [
  'address',
  'province',
  'startDateFrom',
  'startDateTo',
  'endDateFrom',
  'endDateTo',
  'includedDate',
  'deliveryDateFrom',
  'deliveryDateTo',
  'deliveryDate',
  'pickupMethods',
  'startDate',
  'endDate',
  'serialNo',
  'trackingNo',
]

const gaps = [
  'response-envelope',
  'status-normalization',
  'extended-filter-surface',
  'not-found-error-contract',
  'registry-caller-semantics',
]

const clean = {
  'backend/src/application/mod.rs': '#[cfg(test)]\nmod order_read_parity_tests;',
  'backend/src/application/order_read_parity_tests.rs': `
use official_order::FeatureOrder;
use system_core::SystemModule;
use crate::application::OrderQueryService;
fn repository_pool(database_path: &Path) {
  let repository_pool = Pool::builder()
    .build(SqliteConnectionManager::file(database_path));
  let mut module = FeatureOrder::new();
  module.init(json!({
    "databaseUrl": database_path.to_string_lossy().as_ref(),
  }));
  let provider = SqliteRepositoryProvider::new(repository_pool.clone());
}
fn context(tenant: &str) {}
fn exercise(module: FeatureOrder, service: OrderQueryService, ctx: ExecutionContext) {
  module.execute("list_orders", payload, &ctx);
  module.execute("get_order", payload, &ctx);
  service.list(ctx, request);
  service.get_by_id(ctx, "id");
  official_order::state_machine::display_label("paid");
  let localized = "已付款";
  let code = "BIZ_ORDER_NOT_FOUND";
  repository.is_none();
}
${filters.map((filter) => `const FILTER_${filter.replaceAll(/[^a-zA-Z]/g, '_')}: &str = "${filter}";`).join('\n')}
${gaps.map((gap) => `const GAP_${gap.replaceAll('-', '_')}: &str = "${gap}";`).join('\n')}
#[test] fn projection_fields_match_after_explicit_status_normalization() {}
#[test] fn tenant_scope_remains_equivalent_across_both_read_paths() {}
#[test] fn status_filter_requires_an_explicit_compatibility_adapter() {}
#[test] fn extended_filter_surface_blocks_caller_cutover() {}
#[test] fn not_found_error_contract_blocks_caller_cutover() {}
#[test] fn response_envelope_requires_an_explicit_compatibility_adapter() {}
#[test] fn caller_cutover_remains_blocked_until_all_named_gaps_close() {}
`,
  'package.json': `{
    "quality:talos-ops:order-parity:test": "node scripts/check-sp03d-order-read-parity-boundary.test.mjs",
    "quality:talos-ops:order-parity": "node scripts/check-sp03d-order-read-parity-boundary.mjs",
    "quality:talos-ops": "pnpm quality:talos-ops:order-parity:test && pnpm quality:talos-ops:order-parity"
  }`,
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03d-order-read-parity-contract.md': `
NOT_READY_FOR_CALLER_CUTOVER
Response envelope
Status normalization
Extended filter surface
Not-found and error contract
Registry caller semantics
SP-03E Order Read Compatibility Adapter
`,
}

function changed(path, transform) {
  const fixture = structuredClone(clean)
  fixture[path] = transform(fixture[path] ?? '')
  return fixture
}

function expectFailure(number, fixture, evidence, changedPaths = [], options = {}) {
  const failures = checkSp03dOrderReadParityBoundary(fixture, changedPaths, options)
  assert(
    failures.some((item) => item.evidence.includes(evidence)),
    `${number}. expected failure containing: ${evidence}\n${JSON.stringify(failures, null, 2)}`,
  )
}

assert.deepEqual(checkSp03dOrderReadParityBoundary(clean), [], '1. clean boundary passes')
expectFailure(2, changed('backend/src/application/mod.rs', (source) => source.replace('#[cfg(test)]', '')), 'test-only')
expectFailure(3, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('use official_order::FeatureOrder;', '')), 'real official module')
expectFailure(
  '4a',
  changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('"databaseUrl": database_path', '"databaseUrl": other_path')),
  'same SQLite database',
)
expectFailure(
  '4b',
  changed('backend/src/application/order_read_parity_tests.rs', (source) => `${source}\nmodule.pool.lock();`),
  'initialized through SystemModule::init',
)
expectFailure(5, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('module.execute("get_order", payload, &ctx);', '')), 'both list and get')
expectFailure(6, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('projection_fields_match_after_explicit_status_normalization', 'projection_removed')), 'missing executable parity case')
expectFailure(7, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('official_order::state_machine::display_label("paid");', '')), 'status parity')
expectFailure(8, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('"trackingNo"', '"tracking_removed"')), 'trackingNo')
expectFailure(9, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('BIZ_ORDER_NOT_FOUND', 'BIZ_OTHER')), 'not-found parity')
expectFailure(10, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('"registry-caller-semantics"', '"gap-removed"')), 'registry-caller-semantics')
expectFailure(11, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('caller_cutover_remains_blocked_until_all_named_gaps_close', 'historical_case_removed')), 'missing executable parity case')
expectFailure(12, changed('backend/src/application/order_read_parity_tests.rs', (source) => source.replace('service.get_by_id(ctx, "id");', '')), 'both list and get')
expectFailure(13, changed('policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03d-order-read-parity-contract.md', (source) => source.replace('NOT_READY_FOR_CALLER_CUTOVER', 'READY')), 'historical assessment decision')
expectFailure(14, changed('policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03d-order-read-parity-contract.md', (source) => source.replace('SP-03E Order Read Compatibility Adapter', 'direct route cutover')), 'historical next boundary')
expectFailure(15, changed('package.json', (source) => source.replace('check-sp03d-order-read-parity-boundary.test.mjs', 'missing.test.mjs')), 'checker and fixture commands')

const allowedPaths = [
  'backend/src/application/mod.rs',
  'backend/src/application/order_read_parity_tests.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/README.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03d-order-read-parity-contract.md',
  'scripts/check-sp03d-order-read-parity-boundary.mjs',
  'scripts/check-sp03d-order-read-parity-boundary.test.mjs',
  'documentation-link-check.log',
  'repository-layout-check.log',
  'talos-ops-boundary-check.log',
]
assert.deepEqual(
  checkSp03dOrderReadParityBoundary(clean, allowedPaths, { enforceSliceScope: true }),
  [],
  '16a. authorized slice and generated CI reports pass',
)
expectFailure(
  '16b',
  clean,
  'exceed the authorized',
  ['backend/src/routes/orders.rs'],
  { enforceSliceScope: true },
)
expectFailure(
  '16c',
  clean,
  'changed-path discovery failed closed',
  ['GIT_CHANGED_PATH_DISCOVERY_FAILED:boom'],
)

console.log('SP-03D Order read parity boundary checker self-test passed: 16 cases.')
