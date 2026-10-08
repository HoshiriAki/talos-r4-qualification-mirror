#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

import {
  discoverChangedPaths,
  runtimeSource,
} from './check-sp08-scoped-repository-boundary.mjs'

const RULE = 'TALOS-OPS-020'
const EXTRACTORS_PATH = 'backend/src/middleware/tenant_extractors.rs'
const NOTIFY_PATH = 'backend/src/routes/notify.rs'
const WORKERS_PATH = 'backend/src/application/workers.rs'
const GENERATED_CI_LOGS = new Set([
  'documentation-link-check.log',
  'repository-layout-check.log',
  'talos-ops-boundary-check.log',
])

function failure(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

function structBody(source, name) {
  return source.match(new RegExp(`pub\\s+struct\\s+${name}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1] ?? ''
}

export function checkSp03cTrustedTenantResolutionBoundary(
  files,
  changedPaths = [],
  { enforceSliceScope = false } = {},
) {
  const runtime = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), runtimeSource(source)]),
  )
  const failures = []
  const extractors = runtime[EXTRACTORS_PATH] ?? ''
  const notify = runtime[NOTIFY_PATH] ?? ''
  const workers = runtime[WORKERS_PATH] ?? ''

  for (const path of [EXTRACTORS_PATH, NOTIFY_PATH, WORKERS_PATH]) {
    if (!Object.hasOwn(runtime, path)) failures.push(failure(path, 'required SP-03C source is missing'))
  }
  if (failures.length > 0) return failures

  for (const name of ['TrustedTenantUser', 'TrustedTenantAdmin']) {
    const body = structBody(extractors, name)
    if (!body
        || !/tenant\s*:\s*Tenant/.test(body)
        || !/user\s*:\s*AuthUserInfo/.test(body)
        || !/context\s*:\s*ExecutionContext/.test(body)
        || /\bpub(?:\([^)]*\))?\s+(?:tenant|user|context)\s*:/.test(body)) {
      failures.push(failure(EXTRACTORS_PATH, `${name} must privately bind Tenant, AuthUserInfo, and ExecutionContext`))
    }
  }

  if (!/fn\s+resolve_trusted_tenant_context\s*\(\s*tenant\s*:\s*&Tenant\s*,\s*user\s*:\s*&AuthUserInfo\s*,\s*http_client\s*:\s*Arc\s*<\s*dyn\s+HttpClient\s*>/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'trusted resolver must consume middleware Tenant and authenticated AuthUserInfo rather than raw tenant input'))
  }
  if (/resolve_trusted_tenant_context\s*\([^)]*\bTenantId\b/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'trusted resolver must not accept caller-supplied TenantId'))
  }
  if (!/tenant\.status\s*!=\s*"active"/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'trusted resolver must reject inactive tenants'))
  }
  if (!/AuthorityContext::Tenant\s*\{\s*tenant_id\s*,\s*\.\./.test(extractors)
      || !/AuthorityContext::Platform\s*\{\s*\.\.\s*\}/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'trusted resolver must require tenant authority and reject platform authority'))
  }
  if (!/authority_tenant\.as_str\(\)\s*!=\s*tenant\.id/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'host tenant and authenticated authority tenant must match exactly'))
  }
  if (!/TenantId::new\(tenant\.id\.clone\(\)\)/.test(extractors)
      || !/DataScope::production\s*\(/.test(extractors)
      || !/TenantScope::tenant\(tenant_id\)/.test(extractors)
      || !/ExecutionMode::Normal/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'ExecutionContext scope must derive from the matched active tenant'))
  }
  if (!/RequestId::new\(format!\("http:\{\}",\s*uuid::Uuid::new_v4\(\)\)\)/.test(extractors)) {
    failures.push(failure(EXTRACTORS_PATH, 'trusted request contexts must receive fresh server-generated correlation ids'))
  }

  if (!/tenant_extractors::\{TrustedTenantAdmin,\s*TrustedTenantUser\}/.test(notify)) {
    failures.push(failure(NOTIFY_PATH, 'notification routes must import the trusted tenant extractors'))
  }
  if (/middleware::auth::\{?[^;]*(?:AdminUser|AuthUser)/.test(notify)
      || /registry::make_ctx/.test(notify)
      || /state\.http_client/.test(notify)) {
    failures.push(failure(NOTIFY_PATH, 'notification routes must not reconstruct tenant contexts from auth-only inputs'))
  }
  const trustedHandlerCount = notify.match(/(?:admin\s*:\s*TrustedTenantAdmin|auth\s*:\s*TrustedTenantUser)/g)?.length ?? 0
  if (trustedHandlerCount < 6) {
    failures.push(failure(NOTIFY_PATH, 'all six notification handlers must use trusted tenant extractors'))
  }
  const contextUseCount = notify.match(/(?:admin|auth)\.context\(\)/g)?.length ?? 0
  if (contextUseCount < 6) {
    failures.push(failure(NOTIFY_PATH, 'all notification Registry calls must use the extractor-owned ExecutionContext'))
  }

  if (/pub\s+fn\s+tenant_context\s*\(\s*&self\s*,\s*tenant_id\s*:\s*TenantId/.test(workers)) {
    failures.push(failure(WORKERS_PATH, 'production WorkerContextFactory must not trust a caller-supplied TenantId'))
  }

  for (const rawPath of changedPaths) {
    const path = normalizePath(rawPath)
    if (path.startsWith('GIT_CHANGED_PATH_DISCOVERY_FAILED:')) {
      failures.push(failure('git', 'changed-path discovery failed closed'))
      continue
    }
    if (!enforceSliceScope || GENERATED_CI_LOGS.has(path)) continue
    const allowed = path === EXTRACTORS_PATH
      || path === NOTIFY_PATH
      || path === WORKERS_PATH
      || path === 'package.json'
      || path === 'policy/qualification/legacy-evidence/docs/README.md'
      || path === 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03c-trusted-tenant-resolution.md'
      || path === 'scripts/check-sp03b-order-read-repository-boundary.mjs'
      || path === 'scripts/check-sp03b-order-read-repository-boundary.test.mjs'
      || path === 'scripts/check-sp03c-trusted-tenant-resolution-boundary.mjs'
      || path === 'scripts/check-sp03c-trusted-tenant-resolution-boundary.test.mjs'
    if (!allowed) {
      failures.push(failure(path, 'SP-03C changed paths exceed the authorized trusted-resolution/Gate/docs scope'))
    }
  }

  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of [EXTRACTORS_PATH, NOTIFY_PATH, WORKERS_PATH]) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const root = process.cwd()
  const failures = checkSp03cTrustedTenantResolutionBoundary(
    loadFiles(root),
    discoverChangedPaths(root),
  )
  if (failures.length > 0) {
    console.error('SP-03C trusted tenant resolution boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03C trusted tenant resolution boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
