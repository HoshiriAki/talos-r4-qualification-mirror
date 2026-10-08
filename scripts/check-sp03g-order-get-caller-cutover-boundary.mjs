#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-024'
const PATHS = {
  route: 'backend/src/routes/orders.rs',
  package: 'package.json',
  doc: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03g-order-get-caller-cutover.md',
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

function functionSlice(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  if (start < 0 || end < 0) return ''
  return source.slice(start, end)
}

function compactRust(source) {
  return source.replace(/\s+/g, '')
}

function countOccurrences(source, token) {
  return source.split(token).length - 1
}

export function checkSp03gOrderGetCallerCutoverBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(failure(path, 'required SP-03G evidence is missing'))
  }
  if (failures.length > 0) return failures

  const route = files[PATHS.route]
  const packageJson = files[PATHS.package]
  const doc = files[PATHS.doc]
  const listHandler = functionSlice(route, 'async fn list_orders(', 'async fn get_order_by_id(')
  const getHandler = functionSlice(route, 'async fn get_order_by_id(', 'async fn update_order(')
  const compactListHandler = compactRust(listHandler)
  const compactGetHandler = compactRust(getHandler)

  requireText(failures, route, 'get(get_order_by_id)', PATHS.route, 'GET /users/{id} must remain mounted on get_order_by_id')
  requireText(failures, getHandler, 'tenant_user: TenantUser', PATHS.route, 'get caller must retain trusted TenantUser extraction')
  requireText(failures, getHandler, 'let ctx = make_ctx(&user, state.http_client.clone());', PATHS.route, 'get caller must rebuild the trusted ExecutionContext')

  const compatibilityCall = '.execute("order_read_compatibility","get_order",serde_json::json!({"id":id}),&ctx,)'
  requireText(
    failures,
    compactGetHandler,
    compatibilityCall,
    PATHS.route,
    'GET /users/{id} must execute the compatibility Registry command',
  )
  forbidText(
    failures,
    compactGetHandler,
    '.execute("order","get_order"',
    PATHS.route,
    'official Order get command must no longer own the public get caller',
  )
  forbidText(
    failures,
    compactGetHandler,
    'order_queries()',
    PATHS.route,
    'get route must not bypass Registry through ApplicationServices',
  )
  if (countOccurrences(compactGetHandler, compatibilityCall) !== 1) {
    failures.push(failure(PATHS.route, 'get handler must contain exactly one compatibility Registry call'))
  }

  requireText(
    failures,
    compactListHandler,
    '.execute("order_read_compatibility","list_orders",payload,&ctx)',
    PATHS.route,
    'SP-03G must preserve the SP-03F page-list caller',
  )
  requireText(
    failures,
    packageJson,
    'quality:talos-ops:order-offset-cutover',
    PATHS.package,
    'current offset caller authority must be delegated to TALOS-OPS-025',
  )

  for (const token of [
    'quality:talos-ops:order-get-cutover:test',
    'quality:talos-ops:order-get-cutover',
    'check-sp03g-order-get-caller-cutover-boundary.test.mjs',
    'check-sp03g-order-get-caller-cutover-boundary.mjs',
  ]) {
    requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)
  }

  for (const token of [
    'GET_CALLER_CUTOVER_COMPLETE',
    'PAGE_LIST_CALLER_PRESERVED',
    'OFFSET_CALLER_UNCHANGED',
    'WRITE_SIDE_READS_UNCHANGED',
    'order_read_compatibility.get_order',
    'SP-03H',
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
  const failures = checkSp03gOrderGetCallerCutoverBoundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-03G Order get caller cutover boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03G Order get caller cutover boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
