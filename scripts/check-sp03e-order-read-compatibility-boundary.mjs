#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-022'
const PATHS = {
  app: 'backend/src/application/mod.rs',
  adapter: 'backend/src/application/order_read_compatibility.rs',
  repository: 'backend/src/repositories/order_read.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  package: 'package.json',
  doc: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03e-order-read-compatibility-adapter.md',
}

function failure(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(failure(path, evidence))
}

export function checkSp03eOrderReadCompatibilityBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(failure(path, 'required SP-03E evidence is missing'))
  }
  if (failures.length > 0) return failures

  const application = files[PATHS.app]
  const adapter = files[PATHS.adapter]
  const repository = files[PATHS.repository]
  const descriptors = files[PATHS.descriptors]
  const factory = files[PATHS.factory]
  const packageJson = files[PATHS.package]
  const doc = files[PATHS.doc]

  requireText(failures, application, 'mod order_read_compatibility;', PATHS.app, 'application compatibility module must be compiled')
  requireText(failures, application, 'pub use order_read_compatibility::OrderReadCompatibilityModule;', PATHS.app, 'Registry factory must receive the exported compatibility module')

  for (const token of [
    'pub struct OrderReadCompatibilityModule',
    'OrderReadCompatibilityService',
    'OrderStatusCodec',
    'official_order::state_machine::{self, status}',
    'state_machine::display_label(persisted)',
    '"BIZ_ORDER_NOT_FOUND"',
    '"BIZ_ORDER_STATUS_INVALID"',
    '"SYS_ORDER_STATUS_UNKNOWN"',
    'users: Vec<OrderReadProjection>',
    'pagination: CompatibilityPagination',
    'SimulationSupport::Blocked',
    'AccessRequirement::Authenticated',
    'EffectClass::DatabaseRead',
  ]) {
    requireText(failures, adapter, token, PATHS.adapter, `compatibility adapter missing ${token}`)
  }
  for (const alias of [
    '"草稿" | "draft" | "已预约" | "reserved"',
    '"使用中" | "in_use" | "active" | "进行中"',
    '"维修中" | "repairing"',
  ]) {
    requireText(failures, adapter, alias, PATHS.adapter, `status codec missing alias set ${alias}`)
  }

  const requestFields = [
    'address',
    'province',
    'start_date_from',
    'start_date_to',
    'end_date_from',
    'end_date_to',
    'included_date',
    'delivery_date_from',
    'delivery_date_to',
    'delivery_date',
    'pickup_methods',
    'start_date',
    'end_date',
    'serial_no',
    'tracking_no',
  ]
  for (const field of requestFields) {
    requireText(failures, repository, `pub ${field}:`, PATHS.repository, `OrderListRequest missing compatibility field ${field}`)
  }
  for (const sqlToken of [
    'o.province = ?',
    'o.address LIKE ?',
    'o.startDate <= ?',
    'o.endDate >= ?',
    'o.pickupMethods LIKE ?',
    'SELECT 1 FROM order_devices od',
    'od.tenant_id = o.tenant_id',
    'o.trackingNo LIKE ?',
  ]) {
    requireText(failures, repository, sqlToken, PATHS.repository, `repository missing ${sqlToken}`)
  }

  requireText(failures, descriptors, 'OrderReadCompatibility', PATHS.descriptors, 'descriptor factory identity is missing')
  requireText(failures, descriptors, '"order_read_compatibility"', PATHS.descriptors, 'compatibility Registry descriptor is missing')
  requireText(failures, factory, 'OrderReadCompatibilityModule,', PATHS.factory, 'factory construction identity is missing')
  requireText(failures, factory, 'insert(order_read_compatibility_built);', PATHS.factory, 'compatibility module must enter the validated module set')
  requireText(failures, factory, 'OrderReadCompatibilityModule::new(repository_provider.clone())', PATHS.factory, 'compatibility module must use the composition-selected scoped repository provider')

  for (const token of [
    'quality:talos-ops:order-compat:test',
    'quality:talos-ops:order-compat',
    'check-sp03e-order-read-compatibility-boundary.test.mjs',
    'check-sp03e-order-read-compatibility-boundary.mjs',
  ]) {
    requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)
  }

  for (const token of [
    'READY_FOR_BOUNDED_CALLER_CUTOVER',
    'ACTIVE_ROUTES_UNCHANGED',
    'response-envelope',
    'status-normalization',
    'extended-filter-surface',
    'not-found-error-contract',
    'registry-caller-semantics',
    'SP-03F',
  ]) {
    requireText(failures, doc, token, PATHS.doc, `execution record missing ${token}`)
  }

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
  const failures = checkSp03eOrderReadCompatibilityBoundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-03E Order read compatibility boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03E Order read compatibility boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
