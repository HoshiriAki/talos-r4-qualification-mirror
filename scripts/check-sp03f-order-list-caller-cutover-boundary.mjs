#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-023'
const PATHS = {
  route: 'backend/src/routes/orders.rs',
  package: 'package.json',
  doc: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03f-order-list-caller-cutover.md',
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

function functionSlice(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  if (start < 0 || end < 0) return ''
  return source.slice(start, end)
}

function countOccurrences(source, token) {
  return source.split(token).length - 1
}

export function checkSp03fOrderListCallerCutoverBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(failure(path, 'required SP-03F evidence is missing'))
  }
  if (failures.length > 0) return failures

  const route = files[PATHS.route]
  const packageJson = files[PATHS.package]
  const doc = files[PATHS.doc]
  const listHandler = functionSlice(route, 'async fn list_orders(', 'async fn get_order_by_id(')

  requireText(
    failures,
    compact(route),
    '.route("/users",get(list_orders))',
    PATHS.route,
    'GET /users must remain mounted on list_orders; SP-03F does not own the retired POST create surface',
  )
  requireText(failures, listHandler, 'tenant_user: TenantUser', PATHS.route, 'page-list caller must retain trusted TenantUser extraction')
  requireText(failures, listHandler, 'let ctx = make_ctx(&user, state.http_client.clone());', PATHS.route, 'page-list caller must rebuild the trusted ExecutionContext')
  requireText(
    failures,
    packageJson,
    'quality:talos-ops:order-offset-cutover',
    PATHS.package,
    'current offset caller authority must be delegated to TALOS-OPS-025',
  )
  requireText(
    failures,
    listHandler,
    '.execute("order_read_compatibility", "list_orders", payload, &ctx)',
    PATHS.route,
    'page-based list must execute the compatibility Registry command',
  )
  forbidText(
    failures,
    listHandler,
    '.execute("order", "list_orders"',
    PATHS.route,
    'official Order list command must no longer own the page-based caller',
  )
  forbidText(
    failures,
    listHandler,
    'order_queries()',
    PATHS.route,
    'route must not bypass Registry through ApplicationServices',
  )

  const compatibilityCall = '.execute("order_read_compatibility", "list_orders", payload, &ctx)'
  if (countOccurrences(listHandler, compatibilityCall) !== 1) {
    failures.push(failure(PATHS.route, 'page-based list must contain exactly one compatibility Registry call'))
  }

  for (const token of [
    'quality:talos-ops:order-list-cutover:test',
    'quality:talos-ops:order-list-cutover',
    'check-sp03f-order-list-caller-cutover-boundary.test.mjs',
    'check-sp03f-order-list-caller-cutover-boundary.mjs',
  ]) {
    requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)
  }

  for (const token of [
    'PAGE_LIST_CALLER_CUTOVER_COMPLETE',
    'OFFSET_CALLER_UNCHANGED',
    'GET_CALLER_UNCHANGED',
    'order_read_compatibility.list_orders',
    'SP-03G',
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
  const failures = checkSp03fOrderListCallerCutoverBoundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-03F Order list caller cutover boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03F Order list caller cutover boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
