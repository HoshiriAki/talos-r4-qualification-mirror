#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-026'
const PATHS = {
  binding: 'backend/src/repositories/contracts/binding.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  session: 'backend/src/repositories/sqlite/session.rs',
  services: 'backend/src/application/services.rs',
  orderRepository: 'backend/src/repositories/order_read.rs',
  orderQueries: 'backend/src/application/order_queries.rs',
  compatibility: 'backend/src/application/order_read_compatibility.rs',
  route: 'backend/src/routes/orders.rs',
  foundationGate: 'scripts/check-sp08-scoped-repository-boundary.mjs',
  offsetGate: 'scripts/check-sp03h-order-offset-caller-cutover-boundary.mjs',
  offsetRecord: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03h-order-offset-caller-cutover.md',
  closureRecord: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
  package: 'package.json',
  docsIndex: 'policy/qualification/legacy-evidence/docs/README.md',
}

function failure(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(failure(path, evidence))
}

function forbidText(failures, source, token, path, evidence) {
  if (source.includes(token)) failures.push(failure(path, evidence))
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

function countOccurrences(source, token) {
  return source.split(token).length - 1
}

export function checkSp03iScopedRepositoryFoundationClosure(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(failure(path, 'required foundation closure evidence is missing'))
  }
  if (failures.length > 0) return failures

  const binding = files[PATHS.binding]
  const provider = files[PATHS.provider]
  const session = files[PATHS.session]
  const services = files[PATHS.services]
  const orderRepository = files[PATHS.orderRepository]
  const orderQueries = compact(files[PATHS.orderQueries])
  const compatibility = files[PATHS.compatibility]
  const route = compact(files[PATHS.route])
  const foundationGate = files[PATHS.foundationGate]
  const offsetGate = files[PATHS.offsetGate]
  const offsetRecord = files[PATHS.offsetRecord]
  const closureRecord = files[PATHS.closureRecord]
  const packageJson = files[PATHS.package]
  const docsIndex = files[PATHS.docsIndex]

  requireText(failures, binding, 'pub fn from_execution', PATHS.binding, 'RepositoryBinding must remain derivable from ExecutionContext')
  requireText(failures, binding, '&ExecutionContext', PATHS.binding, 'RepositoryBinding must retain ExecutionContext provenance')
  requireText(failures, provider, 'pub trait RepositoryProvider: Send + Sync', PATHS.provider, 'object-safe RepositoryProvider contract must remain present')
  requireText(failures, provider, 'fn bind(', PATHS.provider, 'RepositoryProvider::bind must remain present')
  requireText(failures, provider, '&ExecutionContext', PATHS.provider, 'RepositoryProvider::bind must retain ExecutionContext input')
  requireText(failures, session, 'struct QueryOnlyGuard', PATHS.session, 'SQLite read capability guard must remain present')
  requireText(failures, session, 'impl Drop for QueryOnlyGuard', PATHS.session, 'SQLite read capability restoration must remain RAII-backed')
  requireText(failures, session, 'PreviewWriteDenied', PATHS.session, 'Preview writes must remain denied')
  requireText(failures, services, 'repository_provider: Arc<dyn RepositoryProvider>', PATHS.services, 'ApplicationServices must retain the repository provider')
  forbidText(failures, services, 'pub repository_provider:', PATHS.services, 'repository provider must remain a private ApplicationServices handle')
  requireText(failures, foundationGate, "rule: 'TALOS-OPS-018'", PATHS.foundationGate, 'scoped repository runtime authority must remain active')

  requireText(failures, orderRepository, 'pub struct OrderListRequest', PATHS.orderRepository, 'first production domain read contract must remain present')
  requireText(failures, orderRepository, 'pub offset: Option<u64>', PATHS.orderRepository, 'exact offset support must remain typed and optional')
  requireText(failures, orderRepository, 'pub struct ScopedOrderReadRepository', PATHS.orderRepository, 'first production scoped repository must remain present')
  requireText(failures, orderQueries, 'self.repository_provider.bind(ctx)?.orders().list(request)', PATHS.orderQueries, 'Order query service must bind repositories per ExecutionContext')
  requireText(failures, orderQueries, 'self.repository_provider.bind(ctx)?.orders().get_by_id(id)', PATHS.orderQueries, 'Order get query must remain scoped')

  for (const command of ['"list_orders"', '"get_order"', '"list_orders_offset"']) {
    if (countOccurrences(compatibility, command) < 3) {
      failures.push(failure(PATHS.compatibility, `${command} must remain registered in metadata, dispatch and schema/test evidence`))
    }
  }
  requireText(failures, route, '.execute("order_read_compatibility","list_orders",payload,&ctx)', PATHS.route, 'page-list public caller must remain migrated')
  requireText(failures, route, '.execute("order_read_compatibility","get_order",serde_json::json!({"id":id}),&ctx,)', PATHS.route, 'get public caller must remain migrated')
  requireText(failures, route, '.execute("order_read_compatibility","list_orders_offset",payload,&ctx,)', PATHS.route, 'offset public caller must remain migrated')
  forbidText(failures, route, 'order_service::query_orders_offset(', PATHS.route, 'legacy offset route branch must not return')

  requireText(failures, offsetGate, "const RULE = 'TALOS-OPS-025'", PATHS.offsetGate, 'SP-03H current offset authority must remain active')
  requireText(failures, offsetRecord, 'EXACT_OFFSET_CALLER_CUTOVER_COMPLETE', PATHS.offsetRecord, 'SP-03H implementation record must remain present')
  requireText(failures, offsetRecord, 'EXACT_HEAD_CI_VERIFIED', PATHS.offsetRecord, 'SP-03H final verification evidence must remain present')

  for (const token of [
    'SCOPED_REPOSITORY_FOUNDATION_COMPLETE',
    'FOUNDATION_RUNTIME_SCOPE_FROZEN',
    'ORDER_READ_CALLERS_MIGRATED',
    'DEFERRED_OWNER_MATRIX_COMPLETE',
    'EXACT_HEAD_CI_VERIFIED',
    'MERGED_TO_PROTOTYPE',
    '31189788330',
    '31189788695',
    '8a7b5b414d4b544d21ba550944644d516b552dbc',
    'R1-P1 Customer 与 Typed IDs',
    'R1-P2 Quote、Pricing 与 OrderLine',
    'R1-P3 Order Query/API V2',
    'R1-P4 Reservation/Allocation',
    'R1-P6 Durable Rental Workflow',
    'R2-P1',
    'R2-P6',
    'R3-P4 Simulation',
    'R3-P5 PostgreSQL Parity',
    'O-02',
    'O-07',
    'ADR_GATE_C_DEFERRED',
  ]) {
    requireText(failures, closureRecord, token, PATHS.closureRecord, `closure record missing ${token}`)
  }

  for (const falseClaim of [
    'status: MASTER_PLAN_SP_03_COMPLETE',
    'status: ALL_DOMAIN_REPOSITORIES_COMPLETE',
    'status: POSTGRESQL_PARITY_COMPLETE',
    'status: SIMULATION_STORAGE_COMPLETE',
    'master-plan SP-03: COMPLETE',
    'CI_DEFERRED_EXTERNAL_ACTIONS_OUTAGE',
  ]) {
    forbidText(failures, closureRecord, falseClaim, PATHS.closureRecord, `foundation closure must not claim ${falseClaim}`)
  }

  for (const token of [
    'quality:talos-ops:repository-foundation-closure:test',
    'quality:talos-ops:repository-foundation-closure',
    'check-sp03i-scoped-repository-foundation-closure.test.mjs',
    'check-sp03i-scoped-repository-foundation-closure.mjs',
    'quality:talos-ops:order-offset-cutover',
  ]) {
    requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)
  }

  requireText(failures, docsIndex, 'SP-03I Scoped Repository Foundation closure', PATHS.docsIndex, 'documentation index must register SP-03I')
  requireText(failures, docsIndex, 'foundation complete', PATHS.docsIndex, 'documentation index must distinguish foundation completion')
  requireText(failures, docsIndex, 'R1–R3', PATHS.docsIndex, 'documentation index must preserve deferred release-train ownership')
  requireText(failures, docsIndex, '已验证并合并', PATHS.docsIndex, 'documentation index must expose final verified merge state')

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

function main() {
  const failures = checkSp03iScopedRepositoryFoundationClosure(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-03I scoped repository foundation closure check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03I scoped repository foundation closure check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
