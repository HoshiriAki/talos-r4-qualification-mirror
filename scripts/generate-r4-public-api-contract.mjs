import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const ROUTES_DIR = path.join(ROOT, 'backend/src/routes')
const ROUTES_MOD = path.join(ROUTES_DIR, 'mod.rs')
const GOVERNANCE = path.join(ROOT, 'backend/src/middleware/api_governance.rs')
const ERROR_RS = path.join(ROOT, 'backend/src/error.rs')
const MACHINE_MIGRATION = path.join(ROOT, 'backend/src/db/migrations/066_r3_machine_api.sql')
const ORDER_V2_ROUTE = path.join(ROOT, 'backend/src/routes/orders_v2.rs')
const P2_CLOSURE = path.join(ROOT, 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p2-legacy-compatibility-closure.md')

const METHOD_NAMES = ['get', 'post', 'put', 'delete', 'patch', 'head', 'options']

function rustStringLiterals(source) {
  return [...source.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g)].map(match => match[1])
}

function extractConstStringArray(source, name) {
  const match = source.match(new RegExp(`const\\s+${name}:\\s*&\\[&str\\]\\s*=\\s*&\\[([\\s\\S]*?)\\];`))
  if (!match) throw new Error(`missing string array constant ${name}`)
  return rustStringLiterals(match[1])
}

function sourceSection(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  if (start === -1) throw new Error(`missing source section start: ${startToken}`)
  const end = source.indexOf(endToken, start + startToken.length)
  if (end === -1) throw new Error(`missing source section end: ${endToken}`)
  return source.slice(start, end)
}

function exactRoutesForVariant(section, variant) {
  const pattern = new RegExp(`((?:\\s*"[^"]+"\\s*(?:\\|\\s*)?)+)=>\\s*Self::${variant}`)
  const match = section.match(pattern)
  if (!match) return []
  return rustStringLiterals(match[1])
}

function prefixForVariant(section, variant) {
  const pattern = new RegExp(`path\\.starts_with\\("([^"]+)"\\)\\s*=>\\s*Self::${variant}`)
  return section.match(pattern)?.[1] ?? null
}

// Ingress classification uses if/else blocks rather than match arms. Keep the
// prefix and its Self::<Variant> inside one brace block so a regex can never
// start at the machine branch and drift across a later webhook branch.
function ingressPrefixForVariant(section, variant) {
  const pattern = new RegExp(
    `path\\.starts_with\\("([^"]+)"\\)\\s*\\{[^{}]*Self::${variant}\\b[^{}]*\\}`,
  )
  return section.match(pattern)?.[1] ?? null
}

function ingressExactPathsForVariant(section, variant) {
  const pattern = new RegExp(
    `((?:path\\s*==\\s*"[^"]+"\\s*(?:\\|\\|\\s*)?)+)\\{[^{}]*Self::${variant}\\b[^{}]*\\}`,
  )
  const match = section.match(pattern)
  return match ? rustStringLiterals(match[1]) : []
}

export function governancePolicy(source) {
  const ingress = sourceSection(source, 'impl IngressSurface {', '/// HTTP resource profile')
  const resource = sourceSection(source, 'impl ApiResourceProfile {', '#[derive(Clone, Debug)]\npub struct ApiGovernancePolicy')
  const machinePrefix = ingressPrefixForVariant(ingress, 'MachineBearer')
  const webhookIngressPrefix = ingressPrefixForVariant(ingress, 'ProviderWebhook')
  const diagnosticPaths = ingressExactPathsForVariant(ingress, 'PublicDiagnostic')
  const webhookResourcePrefix = prefixForVariant(resource, 'Webhook')
  if (!machinePrefix || !webhookIngressPrefix || diagnosticPaths.length === 0 || !webhookResourcePrefix) {
    throw new Error('unable to derive ingress/resource policy from api_governance.rs')
  }
  if (machinePrefix === webhookIngressPrefix) {
    throw new Error('machine and webhook ingress prefixes must remain distinct')
  }
  if (webhookIngressPrefix !== webhookResourcePrefix) {
    throw new Error('webhook ingress and resource prefixes diverge')
  }
  return {
    apiPrefixes: extractConstStringArray(source, 'API_PREFIXES'),
    machinePrefix,
    webhookPrefix: webhookIngressPrefix,
    diagnosticPaths: new Set(diagnosticPaths),
    uploadRoutes: new Set(exactRoutesForVariant(resource, 'Upload')),
    extendedRoutes: new Set(exactRoutesForVariant(resource, 'Extended')),
    noBodyRoutes: new Set(exactRoutesForVariant(resource, 'NoBody')),
  }
}

function isApiPath(routePath, policy) {
  return policy.apiPrefixes.some(prefix => routePath.startsWith(prefix))
}

function ingressSurface(routePath, policy) {
  if (routePath.startsWith(policy.machinePrefix)) return 'machine-bearer'
  if (routePath.startsWith(policy.webhookPrefix)) return 'provider-webhook'
  if (policy.diagnosticPaths.has(routePath)) return 'public-diagnostic'
  if (isApiPath(routePath, policy)) return 'browser-session'
  return 'web-ui'
}

function resourceProfile(routePath, policy) {
  if (policy.uploadRoutes.has(routePath)) return 'upload'
  if (policy.extendedRoutes.has(routePath)) return 'extended'
  if (routePath.startsWith(policy.webhookPrefix)) return 'webhook'
  if (policy.noBodyRoutes.has(routePath) || !isApiPath(routePath, policy)) return 'no-body'
  return 'json'
}

export function mountedRouteFactories(source) {
  const factories = new Map()
  for (const match of source.matchAll(/\.merge\(\s*([A-Za-z_][A-Za-z0-9_]*)::([A-Za-z_][A-Za-z0-9_]*)\s*\(/g)) {
    const moduleName = match[1]
    const factoryName = match[2]
    const key = `${moduleName}::${factoryName}`
    factories.set(key, { moduleName, factoryName })
  }
  if (factories.size === 0) throw new Error('no mounted route factories found in routes/mod.rs')
  return [...factories.values()].sort(
    (a, b) => a.moduleName.localeCompare(b.moduleName) || a.factoryName.localeCompare(b.factoryName),
  )
}

export function mountedRouteModules(source) {
  return [...new Set(mountedRouteFactories(source).map(factory => factory.moduleName))].sort()
}

function skipQuoted(source, index, quote) {
  let i = index + 1
  while (i < source.length) {
    if (source[i] === '\\') {
      i += 2
      continue
    }
    if (source[i] === quote) return i + 1
    i += 1
  }
  throw new Error(`unterminated ${quote} literal`)
}

function matchingParen(source, openIndex) {
  let depth = 0
  let i = openIndex
  while (i < source.length) {
    if (source.startsWith('//', i)) {
      const newline = source.indexOf('\n', i + 2)
      i = newline === -1 ? source.length : newline + 1
      continue
    }
    if (source.startsWith('/*', i)) {
      const end = source.indexOf('*/', i + 2)
      if (end === -1) throw new Error('unterminated block comment')
      i = end + 2
      continue
    }
    const ch = source[i]
    if (ch === '"' || ch === "'") {
      i = skipQuoted(source, i, ch)
      continue
    }
    if (ch === '(') depth += 1
    if (ch === ')') {
      depth -= 1
      if (depth === 0) return i
    }
    i += 1
  }
  throw new Error('unterminated .route(...) call')
}

function matchingBrace(source, openIndex) {
  let depth = 0
  let i = openIndex
  while (i < source.length) {
    if (source.startsWith('//', i)) {
      const newline = source.indexOf('\n', i + 2)
      i = newline === -1 ? source.length : newline + 1
      continue
    }
    if (source.startsWith('/*', i)) {
      const end = source.indexOf('*/', i + 2)
      if (end === -1) throw new Error('unterminated block comment')
      i = end + 2
      continue
    }
    const ch = source[i]
    if (ch === '"') {
      i = skipQuoted(source, i, ch)
      continue
    }
    if (ch === '{') depth += 1
    if (ch === '}') {
      depth -= 1
      if (depth === 0) return i
    }
    i += 1
  }
  throw new Error('unterminated route factory body')
}

export function routeFactoryBody(source, factoryName) {
  const signature = new RegExp(`\\bpub\\s+fn\\s+${factoryName}\\s*\\(`)
  const match = signature.exec(source)
  if (!match) throw new Error(`mounted route factory not found: ${factoryName}`)
  const open = source.indexOf('{', match.index + match[0].length)
  if (open === -1) throw new Error(`route factory body not found: ${factoryName}`)
  const close = matchingBrace(source, open)
  return source.slice(open + 1, close)
}

export function parseRouteCalls(source, relativeFile, policy) {
  const endpoints = []
  let searchFrom = 0
  while (true) {
    const marker = source.indexOf('.route(', searchFrom)
    if (marker === -1) break
    const open = marker + '.route'.length
    let i = open + 1
    while (/\s/.test(source[i] ?? '')) i += 1
    if (source[i] !== '"') {
      throw new Error(`${relativeFile}: non-literal Router::route path at offset ${marker}`)
    }
    const stringEnd = skipQuoted(source, i, '"')
    const routePath = source.slice(i + 1, stringEnd - 1)
    const close = matchingParen(source, open)
    const call = source.slice(stringEnd, close)
    const methods = METHOD_NAMES
      .filter(method => new RegExp(`\\b${method}\\s*\\(`).test(call))
      .map(method => method.toUpperCase())
    if (methods.length === 0) {
      throw new Error(`${relativeFile}: no HTTP method found for ${routePath}`)
    }
    for (const method of methods) {
      endpoints.push({
        method,
        path: routePath,
        source: relativeFile,
        ingressSurface: ingressSurface(routePath, policy),
        resourceProfile: resourceProfile(routePath, policy),
      })
    }
    searchFrom = close + 1
  }
  return endpoints
}

function readNumericConst(source, name) {
  const match = source.match(new RegExp(`pub const ${name}: (?:usize|u64) = ([0-9_+*/ ()]+);`))
  if (!match) throw new Error(`missing governance constant ${name}`)
  const expression = match[1].replaceAll('_', '')
  if (!/^[0-9+*/ ()]+$/.test(expression)) throw new Error(`unsafe numeric expression for ${name}`)
  return Function(`"use strict"; return (${expression})`)()
}

function stableErrorCodes(...sources) {
  return [...new Set(sources.flatMap(source => source.match(/\b(?:AUTH|HTTP|VAL|BIZ|SYS|WEBHOOK|MACHINE|PREVIEW)_[A-Z0-9_]+\b/g) ?? []))].sort()
}

function machineVersion(migration) {
  const match = migration.match(/api_version\s+TEXT\s+NOT\s+NULL\s+CHECK\s*\(api_version\s*=\s*'(v\d+)'\)/i)
  if (!match) throw new Error('machine API version constraint missing from migration 066')
  return match[1]
}

function orderV2Pagination(source) {
  const page = source.match(/"page":\s*query\.page\.unwrap_or\((\d+)\)/)
  const pageSize = source.match(/"pageSize":\s*query\.page_size\.unwrap_or\((\d+)\)/)
  if (!page || !pageSize) throw new Error('canonical Order V2 pagination defaults not found')
  return {
    path: '/api/v2/orders',
    strategy: 'page',
    pageParameter: 'page',
    pageSizeParameter: 'pageSize',
    defaultPage: Number(page[1]),
    defaultPageSize: Number(pageSize[1]),
    cursorParameter: null,
  }
}

function compatibilityLifecycle(source) {
  for (const marker of ['ORDER_USERS_COMPATIBILITY_HOLD', 'DEVICE_SERIAL_COMPATIBILITY_HOLD']) {
    if (!source.includes(marker)) throw new Error(`P2 compatibility marker missing: ${marker}`)
  }
  const replacement = source.match(/^ORDER_USERS_REPLACEMENT_PATH=(\/api\/v2\/orders)$/m)?.[1]
  if (!replacement) throw new Error('P2 /users replacement projection missing or invalid')
  return {
    routeHolds: [
      {
        pathPrefix: '/users',
        marker: 'ORDER_USERS_COMPATIBILITY_HOLD',
        replacement,
        newCallersForbidden: true,
        deprecationDate: null,
        sunsetDate: null,
        note: 'Runtime Deprecation/Sunset headers remain forbidden until telemetry and an explicit sunset gate approve dates.',
      },
    ],
    fieldHolds: [
      {
        field: 'deviceSerialNo',
        marker: 'DEVICE_SERIAL_COMPATIBILITY_HOLD',
        newCallersForbidden: true,
        deprecationDate: null,
        sunsetDate: null,
      },
    ],
  }
}

export function contract() {
  const governance = fs.readFileSync(GOVERNANCE, 'utf8')
  const policy = governancePolicy(governance)
  const routesMod = fs.readFileSync(ROUTES_MOD, 'utf8')
  const mountedFactories = mountedRouteFactories(routesMod)
  const mountedModules = [...new Set(mountedFactories.map(factory => factory.moduleName))].sort()
  const endpoints = mountedFactories.flatMap(({ moduleName, factoryName }) => {
    const absolute = path.join(ROUTES_DIR, `${moduleName}.rs`)
    if (!fs.existsSync(absolute)) throw new Error(`mounted route module has no source file: ${moduleName}`)
    const relative = path.relative(ROOT, absolute).replaceAll('\\', '/')
    const source = fs.readFileSync(absolute, 'utf8')
    const factoryBody = routeFactoryBody(source, factoryName)
    return parseRouteCalls(factoryBody, `${relative}#${factoryName}`, policy)
  })

  endpoints.sort((a, b) => a.path.localeCompare(b.path) || a.method.localeCompare(b.method))
  const keys = new Set()
  for (const endpoint of endpoints) {
    const key = `${endpoint.method} ${endpoint.path}`
    if (keys.has(key)) throw new Error(`duplicate HTTP contract entry: ${key}`)
    keys.add(key)
  }

  const errors = fs.readFileSync(ERROR_RS, 'utf8')
  const machineMigration = fs.readFileSync(MACHINE_MIGRATION, 'utf8')
  const orderV2 = fs.readFileSync(ORDER_V2_ROUTE, 'utf8')
  const p2 = fs.readFileSync(P2_CLOSURE, 'utf8')

  return {
    schema: 'talos.public-api-contract/v1',
    authority: {
      endpointTruth: 'backend/src/routes/mod.rs mounted route factories + their Router::route literals',
      transportPolicyTruth: 'backend/src/middleware/api_governance.rs',
      errorTruth: 'backend/src/error.rs + framework normalization in api_governance.rs',
      machineVersionTruth: 'backend/src/db/migrations/066_r3_machine_api.sql',
      lifecycleTruth: 'R4-P2 compatibility closure markers',
      mountedModules,
      mountedFactories: mountedFactories.map(factory => `${factory.moduleName}::${factory.factoryName}`),
      generated: true,
      handMaintainedEndpointCopy: false,
    },
    versioning: {
      machineApi: '/api/machine/{version}/tenants/{tenant}/execute',
      currentMachineVersion: machineVersion(machineMigration),
      versionNegotiation: 'path',
      acceptHeaderVersionNegotiation: false,
      browserSessionAdapters: 'deployment-coupled; versioned domain adapters retain their explicit /api/vN paths',
    },
    pagination: {
      canonicalOrderV2: orderV2Pagination(orderV2),
      genericCursorContract: null,
      note: 'R4 does not invent cursor compatibility over an existing page-based canonical V2 contract.',
    },
    lifecycle: compatibilityLifecycle(p2),
    transport: {
      serverRequestIdHeader: 'x-request-id',
      correlationHeader: 'x-correlation-id',
      idempotencyHeader: 'idempotency-key',
      stableErrorEnvelope: { ok: false, code: 'string', error: 'string' },
      contentTypeNegotiation: 'explicit endpoint media type; no implicit schema/version negotiation',
      bodyLimitBytes: {
        json: readNumericConst(governance, 'JSON_REQUEST_BODY_LIMIT_BYTES'),
        upload: readNumericConst(governance, 'UPLOAD_REQUEST_BODY_LIMIT_BYTES'),
        webhook: readNumericConst(governance, 'WEBHOOK_REQUEST_BODY_LIMIT_BYTES'),
      },
      timeoutSeconds: {
        json: readNumericConst(governance, 'JSON_REQUEST_TIMEOUT_SECS'),
        extended: readNumericConst(governance, 'EXTENDED_REQUEST_TIMEOUT_SECS'),
        webhook: readNumericConst(governance, 'WEBHOOK_REQUEST_TIMEOUT_SECS'),
        noBody: readNumericConst(governance, 'NO_BODY_REQUEST_TIMEOUT_SECS'),
      },
      deadlineSemantics: {
        hardCancellationMethods: ['GET', 'HEAD', 'OPTIONS'],
        mutationMethods: 'cooperative-deadline-only; outer HTTP timeout must not erase committed outcomes',
      },
      streaming: {
        webSocketPublicSurface: false,
        ssePublicSurface: false,
        note: 'No current backend WebSocket/SSE public route exists; any future streaming surface requires explicit origin, duration, inflight and resume policy.',
      },
    },
    stableErrorCodes: stableErrorCodes(errors, governance),
    endpoints,
  }
}

export function validate(result) {
  const required = [
    'POST /auth/login',
    'GET /auth/me',
    'POST /api/machine/{version}/tenants/{tenant}/execute',
    'POST /api/integrations/webhooks/{endpoint_token}',
    'GET /health',
    'GET /ready',
    'GET /metrics',
  ]
  const keys = new Set(result.endpoints.map(endpoint => `${endpoint.method} ${endpoint.path}`))
  for (const key of required) {
    if (!keys.has(key)) throw new Error(`required public contract entry missing: ${key}`)
  }
  for (const endpoint of result.endpoints) {
    if (endpoint.ingressSurface === 'web-ui') {
      throw new Error(`mounted HTTP route is not covered by ingress API policy: ${endpoint.method} ${endpoint.path} (${endpoint.source})`)
    }
  }
  const machine = result.endpoints.find(endpoint => endpoint.path.startsWith('/api/machine/'))
  if (!machine || machine.ingressSurface !== 'machine-bearer') {
    throw new Error('machine execution route is not classified as machine-bearer')
  }
  const webhook = result.endpoints.find(endpoint => endpoint.path.startsWith('/api/integrations/webhooks/'))
  if (!webhook || webhook.ingressSurface !== 'provider-webhook' || webhook.resourceProfile !== 'webhook') {
    throw new Error('provider webhook route is not governed by webhook ingress/resource profile')
  }
  const ready = result.endpoints.find(endpoint => endpoint.path === '/ready')
  if (!ready || ready.ingressSurface !== 'public-diagnostic' || ready.resourceProfile !== 'no-body') {
    throw new Error('/ready must remain a public diagnostic no-body surface')
  }
  const metrics = result.endpoints.find(endpoint => endpoint.path === '/metrics')
  if (!metrics || metrics.ingressSurface !== 'browser-session' || metrics.resourceProfile !== 'json') {
    throw new Error('/metrics must remain a governed authenticated API surface')
  }
  if (result.lifecycle.routeHolds[0].deprecationDate !== null || result.lifecycle.routeHolds[0].sunsetDate !== null) {
    throw new Error('/users compatibility lifecycle must not invent unapproved deprecation/sunset dates')
  }
}

function emit() {
  const result = contract()
  validate(result)
  const json = `${JSON.stringify(result, null, 2)}\n`
  const digest = crypto.createHash('sha256').update(json).digest('hex')

  if (process.argv.includes('--check')) {
    console.log(`R4-P4 generated public API contract: PASS (${result.endpoints.length} operations, ${result.authority.mountedFactories.length} mounted factories, sha256:${digest})`)
  } else {
    process.stdout.write(json)
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  emit()
}
