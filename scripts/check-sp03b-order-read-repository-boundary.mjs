#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

import {
  discoverChangedPaths,
  runtimeSource,
} from './check-sp08-scoped-repository-boundary.mjs'

const RULE = 'TALOS-OPS-019'
const ORDER_PATH = 'backend/src/repositories/order_read.rs'
const PROVIDER_PATH = 'backend/src/repositories/contracts/provider.rs'
const ORDER_QUERIES_PATH = 'backend/src/application/order_queries.rs'
const SERVICES_PATH = 'backend/src/application/services.rs'
const ROUTES_PREFIX = 'backend/src/routes/'

function failure(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

function structBody(source, name) {
  return source.match(new RegExp(`(?:pub\\s+)?struct\\s+${name}(?:\\s*<[^>{}]*>)?\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1] ?? ''
}

export function checkSp03bOrderReadRepositoryBoundary(
  files,
  changedPaths = [],
  { enforceSliceScope = false } = {},
) {
  const runtime = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), runtimeSource(source)]),
  )
  const failures = []
  const order = runtime[ORDER_PATH] ?? ''
  const provider = runtime[PROVIDER_PATH] ?? ''
  const queries = runtime[ORDER_QUERIES_PATH] ?? ''
  const services = runtime[SERVICES_PATH] ?? ''
  const requestBody = structBody(order, 'OrderListRequest')
  const queryBindings = queries.match(/repository_provider\.bind\(ctx\)\?\.orders\(\)/g)?.length ?? 0

  if (!/pub\s+struct\s+ScopedOrderReadRepository\s*</.test(order)) {
    failures.push(failure(ORDER_PATH, 'ScopedOrderReadRepository must be a concrete typed production adapter'))
  }
  if (!/pub\s*\(\s*in\s+crate\s*::\s*repositories\s*\)\s+fn\s+new\s*\(/.test(order)) {
    failures.push(failure(ORDER_PATH, 'order read repository construction must remain inside crate::repositories'))
  }
  if (!/pub\s+fn\s+list\s*\(\s*&self\s*,\s*request\s*:\s*&OrderListRequest/.test(order)
      || !/pub\s+fn\s+get_by_id\s*\(\s*&self\s*,\s*id\s*:\s*&str/.test(order)) {
    failures.push(failure(ORDER_PATH, 'order read repository must expose only typed list and get_by_id reads'))
  }
  if (/pub\s+fn\s+(?:list|get_by_id)\s*\([^)]*\b(?:TenantId|TenantScope|DataScope)\b/.test(order)) {
    failures.push(failure(ORDER_PATH, 'order repository methods must not accept raw tenant or scope arguments'))
  }
  if (/\b(?:tenant|tenant_id)\s*:\s*(?:String|&\s*str|TenantId)/.test(requestBody)) {
    failures.push(failure(ORDER_PATH, 'OrderListRequest must not carry tenant identity'))
  }
  if (!/session\.binding\(\)\.tenant_id\(\)/.test(order)) {
    failures.push(failure(ORDER_PATH, 'order tenant scope must derive from RepositoryBinding'))
  }
  if ((order.match(/tenant_id\s*=\s*\?/g)?.length ?? 0) < 3) {
    failures.push(failure(ORDER_PATH, 'every order and order-device read must bind tenant_id in SQL'))
  }
  if (/\b(?:INSERT|UPDATE|DELETE|REPLACE|CREATE\s+TABLE|DROP\s+TABLE)\b/i.test(order)) {
    failures.push(failure(ORDER_PATH, 'SP-03B order repository must remain read-only in production runtime'))
  }
  if (/\b(?:Pool|SqliteConnectionManager|Transaction)\b/.test(structBody(order, 'ScopedOrderReadRepository'))) {
    failures.push(failure(ORDER_PATH, 'order read repository must use the scoped session rather than own raw storage'))
  }
  if (!/pub\s+enum\s+OrderSortField\s*\{/.test(order)
      || !/pub\s+enum\s+OrderSortDirection\s*\{/.test(order)
      || /sort_by\s*:\s*(?:String|Option\s*<\s*String\s*>)/.test(requestBody)) {
    failures.push(failure(ORDER_PATH, 'order sorting must use closed enums rather than raw SQL strings'))
  }
  if (!/page_size\.clamp\(\s*1\s*,\s*200\s*\)/.test(order)) {
    failures.push(failure(ORDER_PATH, 'order page size must remain bounded to 1..=200'))
  }
  if (!/pub\s+fn\s+orders\s*\(\s*&self\s*\)\s*->\s*ScopedOrderReadRepository/.test(provider)) {
    failures.push(failure(PROVIDER_PATH, 'ScopedRepositories must expose the typed order read adapter'))
  }
  if (!/pub\s+struct\s+OrderQueryService\s*\{/.test(queries)
      || !/repository_provider\s*:\s*Arc\s*<\s*dyn\s+RepositoryProvider\s*>/.test(queries)
      || queryBindings < 2) {
    failures.push(failure(ORDER_QUERIES_PATH, 'OrderQueryService must bind through RepositoryProvider and ExecutionContext'))
  }
  if (/\b(?:Pool|Connection|Transaction|TenantId|DataScope|TenantScope)\b/.test(structBody(queries, 'OrderQueryService'))) {
    failures.push(failure(ORDER_QUERIES_PATH, 'OrderQueryService must own only RepositoryProvider'))
  }
  if (!/pub\s+fn\s+order_queries\s*\(\s*&self\s*\)\s*->\s*OrderQueryService/.test(services)
      || /\border_queries\s*:/.test(structBody(services, 'ApplicationServices'))) {
    failures.push(failure(SERVICES_PATH, 'ApplicationServices must derive OrderQueryService without adding a fifth field'))
  }

  for (const [path, source] of Object.entries(runtime)) {
    if (path.startsWith(ROUTES_PREFIX)
        && /\b(?:OrderQueryService|OrderListRequest|OrderReadProjection|RepositoryProvider|ScopedRepositories)\b/.test(source)) {
      failures.push(failure(path, 'routes must consume approved application facades without owning repository/query types directly'))
    }
  }

  for (const rawPath of changedPaths) {
    const path = normalizePath(rawPath)
    if (path.startsWith('GIT_CHANGED_PATH_DISCOVERY_FAILED:')) {
      failures.push(failure('git', 'changed-path discovery failed closed'))
      continue
    }
    if (enforceSliceScope
        && (path.startsWith('.harness/') || path.startsWith('.superpowers/')
          || path.startsWith('frontend/') || path.startsWith('backend/src/routes/')
          || path.startsWith('backend/src/services/') || path.startsWith('backend/official/')
          || path.startsWith('backend/src/db/migrations/') || path.startsWith('.github/workflows/'))) {
      failures.push(failure(path, 'SP-03B changed paths exceed the authorized repository/application/Gate/docs scope'))
    }
  }

  return failures
}

function collectRust(root, relativePath, files) {
  const directory = join(root, relativePath)
  if (!existsSync(directory)) return
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const child = join(relativePath, entry.name)
    if (entry.isDirectory()) collectRust(root, child, files)
    else if (entry.isFile() && entry.name.endsWith('.rs')) {
      files[normalizePath(child)] = readFileSync(join(root, child), 'utf8')
    }
  }
}

function loadRepositoryFiles(root) {
  const files = {}
  for (const path of ['backend/src/repositories', 'backend/src/application', 'backend/src/routes']) {
    collectRust(root, path, files)
  }
  return files
}

function main() {
  const root = process.cwd()
  const failures = checkSp03bOrderReadRepositoryBoundary(
    loadRepositoryFiles(root),
    discoverChangedPaths(root),
  )
  if (failures.length > 0) {
    console.error('SP-03B order read repository boundary check failed:')
    for (const item of failures) {
      console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    }
    process.exitCode = 1
    return
  }
  console.log('SP-03B order read repository boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
