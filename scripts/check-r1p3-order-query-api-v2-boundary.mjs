#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-029'
const PATHS = Object.freeze({
  application: 'backend/src/application/order_query_v2.rs',
  applicationIndex: 'backend/src/application/mod.rs',
  repository: 'backend/src/repositories/order_read.rs',
  legacyService: 'backend/src/services/order_compatibility_support.rs',
  routes: 'backend/src/routes/orders_v2.rs',
  legacyRoutes: 'backend/src/routes/orders.rs',
  routesIndex: 'backend/src/routes/mod.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  frontendApi: 'frontend/src/api/orders.ts',
  frontendStore: 'frontend/src/stores/orders.ts',
  frontendActions: 'frontend/src/components/order/OrderActions.vue',
  package: 'package.json',
  docsIndex: 'policy/qualification/legacy-evidence/docs/README.md',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p3-order-query-api-v2.md',
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

function requirePattern(failures, source, pattern, path, evidence) {
  if (!pattern.test(source)) failures.push(finding(path, evidence))
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function checkR1P3OrderQueryApiV2Boundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R1-P3 evidence is missing'))
  }
  if (failures.length > 0) return failures

  const application = files[PATHS.application]
  const applicationIndex = files[PATHS.applicationIndex]
  const repository = files[PATHS.repository]
  const legacyService = files[PATHS.legacyService]
  const routes = files[PATHS.routes]
  const legacyRoutes = files[PATHS.legacyRoutes]
  const routesIndex = files[PATHS.routesIndex]
  const descriptors = files[PATHS.descriptors]
  const factory = files[PATHS.factory]
  const frontendApi = files[PATHS.frontendApi]
  const frontendStore = files[PATHS.frontendStore]
  const frontendActions = files[PATHS.frontendActions]
  const packageJson = files[PATHS.package]
  const docsIndex = files[PATHS.docsIndex]
  const record = files[PATHS.record]

  for (const token of [
    'pub struct OrderSummary',
    'pub struct OrderDetail',
    'pub struct OrderLifecycleSummary',
    'pub struct OrderQueryV2Module',
    'OrderQueryService::new(repository_provider',
    'SimulationSupport::Blocked',
    'offset: None',
    '"list_orders"',
    '"get_order"',
    'canonical_filter_normalizes_legacy_status_aliases_without_offset',
  ]) requireText(failures, application, token, PATHS.application, `Order Query V2 application boundary missing ${token}`)

  const lifecycleV2Delegated =
    application.includes('LifecycleAllowedAction')
    && application.includes('lifecycles().operational_view')
    && application.includes('allowed_actions: operational.allowed_actions')
    && packageJson.includes('"quality:talos-ops:lifecycle-v2:test":')
    && packageJson.includes('"quality:talos-ops:lifecycle-v2":')

  if (lifecycleV2Delegated) {
    for (const token of [
      'LifecycleAllowedAction',
      'lifecycles().operational_view',
      'allowed_actions: operational.allowed_actions',
    ]) requireText(failures, application, token, PATHS.application, `Order Query V2 Lifecycle V2 successor delegation missing ${token}`)
    forbidText(failures, application, 'state_machine::allowed_next', PATHS.application, 'Lifecycle V2 successor delegation must not restore the retired single-axis action graph')
  } else {
    for (const token of [
      'pub struct OrderAllowedAction',
      'state_machine::allowed_next',
      'projection_derives_allowed_actions_from_official_state_machine',
      'closed_order_is_terminal_in_query_projection',
    ]) requireText(failures, application, token, PATHS.application, `Order Query V2 application boundary missing ${token}`)
  }

  forbidText(failures, application, 'Pool<SqliteConnectionManager>', PATHS.application, 'canonical query module must not own a raw SQLite pool')
  requireText(failures, applicationIndex, 'pub use order_query_v2::OrderQueryV2Module;', PATHS.applicationIndex, 'application index must export OrderQueryV2Module')

  for (const token of [
    'self.repository_provider.bind(ctx)?.orders().list(request)',
    'self.repository_provider.bind(ctx)?.orders().get_by_id(id)',
  ]) requireText(failures, files['backend/src/application/order_queries.rs'] ?? '', token, 'backend/src/application/order_queries.rs', `OrderQueryService must remain scoped: ${token}`)

  requireText(failures, repository, 'self.session.binding().tenant_id()', PATHS.repository, 'Order repository must remain tenant-bound')
  requireText(failures, repository, 'LIMIT ? OFFSET ?', PATHS.repository, 'repository may retain internal native offset support for legacy compatibility')
  forbidText(failures, legacyService, 'pub fn query_orders_offset(', PATHS.legacyService, 'zero-caller raw-pool offset query must be deleted')

  for (const token of [
    'TrustedTenantUser',
    '"/api/v2/orders"',
    '"/api/v2/orders/{id}"',
    '"order_query_v2"',
    'tenant_user.context()',
  ]) requireText(failures, routes, token, PATHS.routes, `V2 route missing ${token}`)
  forbidText(failures, routes, 'offset:', PATHS.routes, 'public V2 route must not expose native offset')
  forbidText(failures, routes, 'SELECT ', PATHS.routes, 'route-layer SQL is forbidden')
  forbidText(failures, routes, 'order_service::', PATHS.routes, 'V2 route must not call legacy Order service')
  requireText(failures, routesIndex, 'pub mod orders_v2;', PATHS.routesIndex, 'orders_v2 module must be declared')
  requireText(failures, routesIndex, '.merge(orders_v2::order_v2_routes())', PATHS.routesIndex, 'orders_v2 routes must be mounted')
  requireText(failures, legacyRoutes, '"order_read_compatibility"', PATHS.legacyRoutes, 'legacy /users reads must remain compatibility Registry-backed')

  requirePattern(
    failures,
    compact(descriptors),
    /descriptor!\("order_query_v2",Business,ModuleActivation::Always,[A-Z_]+,OrderQueryV2\)/,
    PATHS.descriptors,
    'descriptor catalog must register order_query_v2',
  )
  requireText(failures, factory, '(OrderQueryV2Module, OrderQueryV2, "order_query_v2")', PATHS.factory, 'ModuleFactory identity must own order_query_v2')
  requireText(failures, factory, 'OrderQueryV2Module::new(repository_provider.clone())', PATHS.factory, 'ModuleFactory must inject the scoped repository provider')
  requireText(failures, factory, '"order_query_v2"', PATHS.factory, 'baseline module set must include order_query_v2')

  requireText(failures, frontendApi, 'requestJson(`/api/v2/orders?', PATHS.frontendApi, 'frontend page reads must target /api/v2/orders')
  requireText(failures, frontendApi, 'result.items ?? []', PATHS.frontendApi, 'frontend must consume the canonical items envelope')
  forbidText(failures, frontendApi, 'export async function fetchOffset', PATHS.frontendApi, 'frontend offset API must be removed')
  forbidText(failures, frontendApi, 'STATUS_TRANSITIONS', PATHS.frontendApi, 'frontend must not own the Order transition graph')
  forbidText(failures, frontendStore, 'ordersApi.fetchOffset(', PATHS.frontendStore, 'Order store must not use offset reads')
  requireText(failures, frontendStore, "mode: 'page'", PATHS.frontendStore, 'Order store must use the single page strategy')
  requireText(failures, frontendActions, 'allowedActions', PATHS.frontendActions, 'OrderActions must consume server-provided allowedActions')
  forbidText(failures, frontendActions, 'STATUS_TRANSITIONS', PATHS.frontendActions, 'OrderActions must not import a client transition graph')

  for (const token of [
    'ORDER_QUERY_V2_SINGLE_READ_AUTHORITY',
    'ORDER_V2_PAGE_STRATEGY_ONLY',
    'ORDER_SUMMARY_DETAIL_TYPED',
    'SERVER_ALLOWED_ACTIONS_REQUIRED',
    'LEGACY_USERS_READ_COMPATIBILITY_PRESERVED',
    'LEGACY_OFFSET_SERVICE_QUERY_REMOVED',
    'TALOS-OPS-029',
    'R1-P4',
  ]) requireText(failures, record, token, PATHS.record, `R1-P3 execution record missing ${token}`)
  for (const falseClaim of [
    'status: R1_COMPLETE',
    'status: ORDER_WRITES_MIGRATED',
    'status: LEGACY_ORDER_SERVICE_DELETABLE',
    'status: MASTER_PLAN_SP_03_COMPLETE',
  ]) forbidText(failures, record, falseClaim, PATHS.record, `R1-P3 must not claim ${falseClaim}`)

  requireText(failures, docsIndex, 'R1-P3 Order Query / API V2', PATHS.docsIndex, 'documentation index must register R1-P3')
  for (const token of [
    'quality:talos-ops:order-query-v2:test',
    'quality:talos-ops:order-query-v2',
    'check-r1p3-order-query-api-v2-boundary.test.mjs',
    'check-r1p3-order-query-api-v2-boundary.mjs',
  ]) requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)

  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of [...Object.values(PATHS), 'backend/src/application/order_queries.rs']) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const failures = checkR1P3OrderQueryApiV2Boundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('R1-P3 Order Query / API V2 boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R1-P3 Order Query / API V2 boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
