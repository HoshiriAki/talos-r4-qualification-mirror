#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-025'
const PATHS = {
  route: 'backend/src/routes/orders.rs',
  repository: 'backend/src/repositories/order_read.rs',
  compatibility: 'backend/src/application/order_read_compatibility.rs',
  package: 'package.json',
  doc: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03h-order-offset-caller-cutover.md',
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

function compact(source) {
  return source.replace(/\s+/g, '')
}

function countOccurrences(source, token) {
  return source.split(token).length - 1
}

export function checkSp03hOrderOffsetCallerCutoverBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(failure(path, 'required SP-03H evidence is missing'))
  }
  if (failures.length > 0) return failures

  const route = files[PATHS.route]
  const repository = files[PATHS.repository]
  const compatibility = files[PATHS.compatibility]
  const packageJson = files[PATHS.package]
  const doc = files[PATHS.doc]
  const listHandler = functionSlice(route, 'async fn list_orders(', 'async fn get_order_by_id(')
  const getHandler = functionSlice(route, 'async fn get_order_by_id(', 'async fn update_order(')
  const compactRoute = compact(route)
  const compactList = compact(listHandler)
  const compactGet = compact(getHandler)
  const compactRepository = compact(repository)
  const compactCompatibility = compact(compatibility)

  requireText(failures, compactRoute, '.route("/users",get(list_orders))', PATHS.route, 'GET /users mount must remain on list_orders; SP-03H does not own the retired POST create surface')
  requireText(failures, listHandler, 'tenant_user: TenantUser', PATHS.route, 'offset caller must retain trusted TenantUser extraction')
  requireText(failures, listHandler, 'let ctx = make_ctx(&user, state.http_client.clone());', PATHS.route, 'offset caller must rebuild the trusted ExecutionContext')
  requireText(failures, compactList, 'ifletSome(offset)=q.offset{', PATHS.route, 'offset branch must be explicit and value-bound')
  requireText(failures, compactList, '"limit":q.limit.unwrap_or(50)', PATHS.route, 'legacy default limit must remain 50')
  requireText(failures, compactList, '"offset":offset', PATHS.route, 'exact native offset must be forwarded without page approximation')
  requireText(failures, compactList, '"filter":filter', PATHS.route, 'offset caller must preserve the existing filter object')
  requireText(failures, compactList, '"sortBy":q.sort_by.clone()', PATHS.route, 'offset caller must preserve sortBy')
  requireText(failures, compactList, '"sortOrder":q.sort_order.clone()', PATHS.route, 'offset caller must preserve sortOrder')

  const offsetCall = '.execute("order_read_compatibility","list_orders_offset",payload,&ctx,)'
  requireText(failures, compactList, offsetCall, PATHS.route, 'offset caller must execute the compatibility Registry command')
  if (countOccurrences(compactList, offsetCall) !== 1) {
    failures.push(failure(PATHS.route, 'offset handler must contain exactly one compatibility Registry call'))
  }
  forbidText(failures, listHandler, 'order_service::query_orders_offset(', PATHS.route, 'route must not retain the legacy raw-pool offset service')
  forbidText(failures, listHandler, 'order_queries()', PATHS.route, 'route must not bypass Registry through ApplicationServices')
  requireText(failures, compactList, '.execute("order_read_compatibility","list_orders",payload,&ctx)', PATHS.route, 'page-list compatibility caller must remain preserved')
  requireText(failures, compactGet, '.execute("order_read_compatibility","get_order",serde_json::json!({"id":id}),&ctx,)', PATHS.route, 'public get compatibility caller must remain preserved')

  requireText(failures, repository, 'pub offset: Option<u64>,', PATHS.repository, 'typed Order request must expose an optional native offset')
  requireText(failures, compactRepository, 'offset:None,', PATHS.repository, 'page-based callers must default to no native offset override')
  requireText(failures, compactRepository, 'letoffset=request.offset.unwrap_or_else(||u64::from(page-1)*u64::from(page_size));', PATHS.repository, 'repository must use exact offset when supplied and retain page fallback otherwise')
  requireText(failures, repository, 'LIMIT ? OFFSET ?', PATHS.repository, 'repository must retain native SQL LIMIT/OFFSET')
  requireText(failures, compactRepository, 'i64::try_from(offset).unwrap_or(i64::MAX)', PATHS.repository, 'offset conversion must remain bounded and non-panicking')

  requireText(failures, compatibility, 'struct CompatibilityOffsetListInput', PATHS.compatibility, 'compatibility adapter must own a typed offset input')
  requireText(failures, compatibility, 'struct CompatibilityOffsetPage', PATHS.compatibility, 'compatibility adapter must own the legacy offset envelope')
  requireText(failures, compactCompatibility, 'users:Vec<OrderReadProjection>,limit:u32,total:u32,', PATHS.compatibility, 'legacy envelope must remain users + limit + total')
  requireText(failures, compatibility, 'fn list_offset(', PATHS.compatibility, 'compatibility service must expose an offset entry point')
  requireText(failures, compactCompatibility, 'request.offset=Some(offset);', PATHS.compatibility, 'adapter must bind the exact offset to the typed request')
  requireText(failures, compatibility, '"list_orders_offset"', PATHS.compatibility, 'Registry command, execution branch and schema must remain registered')
  if (countOccurrences(compatibility, '"list_orders_offset"') < 3) {
    failures.push(failure(PATHS.compatibility, 'offset command must exist in metadata, execution dispatch and schema'))
  }
  requireText(failures, compatibility, 'offset_command_preserves_exact_non_page_aligned_window_and_legacy_envelope', PATHS.compatibility, 'executable exact-window parity test is required')
  requireText(failures, compactCompatibility, '"limit":2,"offset":1', PATHS.compatibility, 'test must cover a non-page-aligned offset')
  requireText(failures, compactCompatibility, 'assert_eq!(ids,vec!["order-c","order-b"]);', PATHS.compatibility, 'test must prove the exact selected window')
  requireText(failures, compactCompatibility, 'assert!(result.get("pagination").is_none());', PATHS.compatibility, 'test must prove the legacy flat envelope')

  for (const token of [
    'quality:talos-ops:order-offset-cutover:test',
    'quality:talos-ops:order-offset-cutover',
    'check-sp03h-order-offset-caller-cutover-boundary.test.mjs',
    'check-sp03h-order-offset-caller-cutover-boundary.mjs',
  ]) {
    requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)
  }

  for (const token of [
    'EXACT_OFFSET_CALLER_CUTOVER_COMPLETE',
    'NON_PAGE_ALIGNED_OFFSET_PRESERVED',
    'LEGACY_OFFSET_ENVELOPE_PRESERVED',
    'PAGE_LIST_CALLER_PRESERVED',
    'GET_CALLER_PRESERVED',
    'WRITE_PATHS_UNCHANGED',
    'order_read_compatibility.list_orders_offset',
    'SP-03I',
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
  const failures = checkSp03hOrderOffsetCallerCutoverBoundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-03H Order offset caller cutover boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03H Order offset caller cutover boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
