#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const BASELINE_COMMIT = '5cf781fb69cf898a9be0530099724e62c134b031'
export const REMOVED_LEGACY_SF_PATHS = [
  'backend/src/routes/sf_express.rs',
  'backend/src/services/sf_express.rs',
]
export const REMOVED_UNMOUNTED_RESERVATION_ADAPTER_PATH = 'backend/src/routes/reservation.rs'
export const REMOVED_LEGACY_EXECUTION_CONTEXT_IDENTIFIER = 'LegacyExecutionContext'
export const ORDER_SERVICE_PATH = 'backend/src/services/order_service.rs'
export const ORDER_COMPATIBILITY_SUPPORT_PATH = 'backend/src/services/order_compatibility_support.rs'
export const REGISTRY_PATH = 'backend/src/registry/mod.rs'
export const ORDER_STATE_MACHINE_PATH = 'backend/official/order/src/state_machine.rs'
export const CORE_PRIMITIVE_DISPOSITION = Object.freeze({
  orchestration: 'EXPERIMENTAL_FEATURE',
  transport: 'KEEP_CORE_CANONICAL',
})

const CORE_CARGO_PATH = 'backend/system/core/Cargo.toml'
const CORE_LIB_PATH = 'backend/system/core/src/lib.rs'
const EXPERIMENTAL_MOD_PATH = 'backend/system/core/src/experimental/mod.rs'
const EXPERIMENTAL_ORCHESTRATION_PATH = 'backend/system/core/src/experimental/orchestration.rs'
const EXPERIMENTAL_TRANSPORT_PATH = 'backend/system/core/src/experimental/transport.rs'
const MODULE_KIT_PATHS = ['module-kit/INIT.md', 'module-kit/MODULE_TEMPLATE.md']
const ORCHESTRATION_SYMBOLS = ['ModuleOp', 'SagaStep', 'OrchestrationError', 'CompensationLog']
const TRANSPORT_SYMBOLS = ['DataTransport', 'TransportMetadata', 'TransportPriority', 'TransportOptions', 'TransportMessage', 'TransportHealth']
const SP_05_POLICY_MARKER = 'orchestration=EXPERIMENTAL_FEATURE; transport=KEEP_CORE_CANONICAL'
const APPLICATION_MOD_PATH = 'backend/src/application/mod.rs'
const APPLICATION_CLOCK_PATH = 'backend/src/application/clock.rs'
const APPLICATION_MODULE_CLIENT_PATH = 'backend/src/application/module_client.rs'
const APPLICATION_SERVICES_PATH = 'backend/src/application/services.rs'
const APP_STATE_PATH = 'backend/src/state.rs'
const APP_MAIN_PATH = 'backend/src/main.rs'

const WRITE_OPERATION_PATTERN = '(?:INSERT(?:\\s+OR\\s+REPLACE)?\\s+INTO|UPDATE|DELETE\\s+FROM)'
const CORE_TABLE_PATTERN = '(?:orders|booking|bookings|reservations|reservation|payments|payment|shipments|shipment_records|webhooks|webhook_events)'
const BUSINESS_WRITE_PATTERN = new RegExp(`\\b${WRITE_OPERATION_PATTERN}\\s+${CORE_TABLE_PATTERN}\\b`, 'gi')
const ANY_WRITE_PATTERN = new RegExp(`\\b${WRITE_OPERATION_PATTERN}\\s+[a-zA-Z_][\\w]*\\b`, 'gi')

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

export function normalizeEvidence(value) {
  return value.replaceAll(/\s+/g, ' ').trim()
}

function canonicalSource(value) {
  return normalizeEvidence(value.replaceAll('\r\n', '\n').replaceAll('\r', '\n'))
}

function contextEvidence(text, index, length) {
  const start = Math.max(0, index - 96)
  const end = Math.min(text.length, index + length + 256)
  return text.slice(start, end)
}

function runtimeSource(text) {
  const normalized = text.replaceAll('\r\n', '\n').replaceAll('\r', '\n')
  const testBoundary = normalized.search(/#\[cfg\s*\(\s*test\s*\)\]/)
  return testBoundary === -1 ? normalized : normalized.slice(0, testBoundary)
}

function addFinding(findings, rule, path, evidence) {
  findings.push({ rule, path: normalizePath(path), evidence: normalizeEvidence(evidence) })
}

function moduleName(path) {
  return path.split('/').at(-1).replace(/\.rs$/, '')
}

function escapeRegExp(value) {
  return value.replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function isRegisteredService(path, registryText) {
  return new RegExp(`\\b${escapeRegExp(moduleName(path))}\\b`).test(registryText)
}

export function collectFindings(files) {
  const findings = []
  const entries = Object.entries(files).map(([path, text]) => [normalizePath(path), canonicalSource(runtimeSource(text))])
  const registryText = entries
    .filter(([path]) => path === 'backend/src/registry/assembler.rs' || path === 'backend/src/registry/mod.rs')
    .map(([, text]) => text)
    .join(' ')

  for (const [path, text] of entries) {
    const isRust = path.endsWith('.rs')
    const isRoute = path.startsWith('backend/src/routes/') && isRust
    const isService = path.startsWith('backend/src/services/') && isRust
    const isWeb = path.startsWith('frontend/src/') && /\.(?:ts|tsx|vue)$/.test(path)
    const isThirdPartyCargo = path.startsWith('backend/thirdparty/') && path.endsWith('Cargo.toml')

    if (isRoute) {
      for (const match of text.matchAll(new RegExp(BUSINESS_WRITE_PATTERN.source, 'gi'))) {
        addFinding(findings, 'TALOS-OPS-001', path, contextEvidence(text, match.index, match[0].length))
      }

      for (const match of text.matchAll(/\.route\s*\(\s*["']\/users(?:\/|["'{])/g)) {
        addFinding(findings, 'TALOS-OPS-005', path, match[0])
      }
    }

    if (isRust) {
      for (const match of text.matchAll(/\buse\s+crate::services::[^;]*\border_service\b[^;]*;?/g)) {
        addFinding(findings, 'TALOS-OPS-002', path, match[0])
      }

      for (const match of text.matchAll(/\border_id\s*:\s*i64\b/g)) {
        addFinding(findings, 'TALOS-OPS-003', path, contextEvidence(text, match.index, match[0].length))
      }

      for (const match of text.matchAll(/\bfn\s+[a-zA-Z_]*shanghai_now[a-zA-Z_]*\s*\(/g)) {
        addFinding(findings, 'TALOS-OPS-004', path, contextEvidence(text, match.index, match[0].length))
      }

      if (/\bStub\b/.test(text)) {
        for (const match of text.matchAll(/\bOperational\b/g)) {
          addFinding(findings, 'TALOS-OPS-006', path, contextEvidence(text, match.index, match[0].length))
        }
      }
    }

    if (isThirdPartyCargo) {
      for (const match of text.matchAll(/(?:\bpath\s*=\s*["'](?:\.\.\/)+official\/|\bofficial[-_][a-z0-9_-]*\s*=)/gi)) {
        addFinding(findings, 'TALOS-OPS-007', path, contextEvidence(text, match.index, match[0].length))
      }
    }

    if (isWeb) {
      for (const match of text.matchAll(/\bPartial\s*<\s*Order\b/g)) {
        addFinding(findings, 'TALOS-OPS-008', path, match[0])
      }
    }

    if (isService && !isRegisteredService(path, registryText)) {
      for (const match of text.matchAll(new RegExp(ANY_WRITE_PATTERN.source, 'gi'))) {
        // TALOS-OPS-009 is an authority inventory, not a formatting fingerprint.
        // Compare stable write signatures so comment/whitespace refactors cannot
        // manufacture a false "new SQL authority" while added write occurrences
        // or new service/table signatures still fail against the frozen baseline.
        addFinding(findings, 'TALOS-OPS-009', path, match[0])
      }
    }
  }

  return findings
}

function comparisonPath(path) {
  // The compatibility service was intentionally renamed after the fingerprint baseline was
  // created. Preserve the baseline allowance for unchanged legacy maintenance SQL while still
  // comparing each write signature (new or expanded writes remain findings).
  return path === ORDER_COMPATIBILITY_SUPPORT_PATH ? ORDER_SERVICE_PATH : path
}

function comparisonEvidence(entry) {
  return entry.evidence
}

function findingKey(entry) {
  return JSON.stringify([entry.rule, comparisonPath(entry.path), comparisonEvidence(entry)])
}

function counts(findings) {
  const result = new Map()
  for (const finding of findings) {
    const key = findingKey(finding)
    const current = result.get(key) ?? { count: 0, finding }
    current.count += 1
    result.set(key, current)
  }
  return result
}

export function compareFindingSets(baselineFindings, currentFindings) {
  const baseline = counts(baselineFindings)
  const current = counts(currentFindings)
  const failures = []

  for (const [key, actual] of current) {
    const permitted = baseline.get(key)?.count ?? 0
    if (actual.count > permitted) failures.push({ ...actual.finding, occurrences: actual.count - permitted })
  }

  return failures.sort((left, right) => findingKey(left).localeCompare(findingKey(right)))
}

export function checkRemovedLegacySfPaths(files) {
  return REMOVED_LEGACY_SF_PATHS
    .filter((path) => Object.hasOwn(files, path))
    .map((path) => ({
      rule: 'TALOS-OPS-011',
      path,
      evidence: 'removed legacy SF path must not be recreated',
      occurrences: 1,
    }))
}

export function checkRemovedUnmountedReservationAdapterPath(files) {
  return Object.hasOwn(files, REMOVED_UNMOUNTED_RESERVATION_ADAPTER_PATH)
    ? [{
        rule: 'TALOS-OPS-013',
        path: REMOVED_UNMOUNTED_RESERVATION_ADAPTER_PATH,
        evidence: 'removed unmounted Reservation REST adapter must not be recreated',
        occurrences: 1,
      }]
    : []
}

function isLegacyExecutionContextSurface(path) {
  return path.startsWith('backend/') && path.endsWith('.rs')
    || /^(?:module-kit|templates|scaffold|generator|codegen)\//.test(path)
}

export function checkRemovedLegacyExecutionContext(files) {
  const identifier = new RegExp(`\\b${REMOVED_LEGACY_EXECUTION_CONTEXT_IDENTIFIER}\\b`, 'g')
  const findings = []

  for (const [rawPath, text] of Object.entries(files)) {
    const path = normalizePath(rawPath)
    if (!isLegacyExecutionContextSurface(path)) continue

    for (const match of text.matchAll(identifier)) {
      addFinding(
        findings,
        'TALOS-OPS-012',
        path,
        contextEvidence(text, match.index, match[0].length),
      )
    }
  }

  return findings.map((finding) => ({ ...finding, occurrences: 1 }))
}

export function checkBoundedStaleDeadSuppressions(files) {
  const findings = []
  for (const servicePath of [ORDER_SERVICE_PATH, ORDER_COMPATIBILITY_SUPPORT_PATH]) {
    const orderService = files[servicePath]
    if (orderService === undefined) continue
    const suppressions = [
      /#!\s*\[\s*allow\s*\(\s*dead_code\s*\)\s*\]/g,
      /#!\s*\[\s*allow\s*\(\s*unused\s*\)\s*\]/g,
      /#\s*\[\s*allow\s*\(\s*dead_code\s*\)\s*\]/g,
      /#\s*\[\s*expect\s*\(\s*dead_code\s*\)\s*\]/g,
    ]
    for (const pattern of suppressions) {
      for (const match of orderService.matchAll(pattern)) {
        addFinding(findings, 'TALOS-OPS-014', servicePath, match[0])
      }
    }
  }

  const registry = files[REGISTRY_PATH]
  if (registry !== undefined) {
    for (const match of registry.matchAll(/集中管理\s+14\s+个\s+SystemModule/g)) {
      addFinding(findings, 'TALOS-OPS-014', REGISTRY_PATH, match[0])
    }
  }

  const stateMachine = files[ORDER_STATE_MACHINE_PATH]
  if (stateMachine !== undefined) {
    for (const match of stateMachine.matchAll(/#\s*\[\s*allow\s*\(\s*clippy::if_same_then_else\s*\)\s*\]/g)) {
      addFinding(findings, 'TALOS-OPS-014', ORDER_STATE_MACHINE_PATH, match[0])
    }
  }

  return findings.map((finding) => ({ ...finding, occurrences: 1 }))
}

function primitivePolicyFailure(path, evidence) {
  return { rule: 'TALOS-OPS-015', path, evidence, occurrences: 1 }
}

function hasPublicDefinition(source, symbol) {
  return new RegExp(`\\bpub\\s+(?:struct|enum|trait)\\s+${symbol}\\b`).test(source)
}

export function checkCoreExperimentalPrimitiveDisposition(files) {
  const normalizedFiles = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), source]),
  )
  const failures = []
  const cargo = normalizedFiles[CORE_CARGO_PATH] ?? ''
  const core = normalizedFiles[CORE_LIB_PATH] ?? ''
  const experimentalMod = normalizedFiles[EXPERIMENTAL_MOD_PATH] ?? ''
  const orchestration = normalizedFiles[EXPERIMENTAL_ORCHESTRATION_PATH] ?? ''

  if (!/^default\s*=\s*\[\s*\]\s*$/m.test(cargo)) {
    failures.push(primitivePolicyFailure(CORE_CARGO_PATH, 'default features must remain empty'))
  }
  if (!/^experimental-orchestration\s*=\s*\[\s*\]\s*$/m.test(cargo)) {
    failures.push(primitivePolicyFailure(CORE_CARGO_PATH, 'missing non-default experimental-orchestration feature'))
  }
  if (!/#\[cfg\(feature\s*=\s*"experimental-orchestration"\)\]\s*pub mod experimental;/m.test(core)) {
    failures.push(primitivePolicyFailure(CORE_LIB_PATH, 'experimental module must be gated by experimental-orchestration'))
  }

  for (const symbol of ORCHESTRATION_SYMBOLS) {
    if (hasPublicDefinition(core, symbol)) {
      failures.push(primitivePolicyFailure(CORE_LIB_PATH, `moved ${symbol} must not be defined at the crate root`))
    }
    if (!hasPublicDefinition(orchestration, symbol)) {
      failures.push(primitivePolicyFailure(EXPERIMENTAL_ORCHESTRATION_PATH, `experimental ${symbol} definition is missing`))
    }
  }

  if (!/pub mod orchestration;/.test(experimentalMod)
      || !ORCHESTRATION_SYMBOLS.every((symbol) => experimentalMod.includes(symbol))) {
    failures.push(primitivePolicyFailure(EXPERIMENTAL_MOD_PATH, 'experimental orchestration exports are incomplete'))
  }

  for (const match of core.matchAll(/\bpub\s+use[^;\n]*(?:ModuleOp|SagaStep|OrchestrationError|CompensationLog)[^;\n]*;/g)) {
    failures.push(primitivePolicyFailure(CORE_LIB_PATH, `unconditional root re-export is forbidden: ${match[0]}`))
  }

  for (const symbol of TRANSPORT_SYMBOLS) {
    if (!hasPublicDefinition(core, symbol)) {
      failures.push(primitivePolicyFailure(CORE_LIB_PATH, `canonical root ${symbol} definition is missing`))
    }
  }
  const experimentalTransport = normalizedFiles[EXPERIMENTAL_TRANSPORT_PATH]
  if (experimentalTransport !== undefined
      && TRANSPORT_SYMBOLS.some((symbol) => hasPublicDefinition(experimentalTransport, symbol))) {
    failures.push(primitivePolicyFailure(EXPERIMENTAL_TRANSPORT_PATH, 'canonical transport group must not be duplicated under experimental'))
  }

  for (const path of MODULE_KIT_PATHS) {
    const source = normalizedFiles[path] ?? ''
    if (!source.includes(SP_05_POLICY_MARKER)) {
      failures.push(primitivePolicyFailure(path, 'active projection is missing the SP-05 disposition marker'))
    }
    if (/\bpub\s+struct\s+(?:ModuleOp|SagaStep|OrchestrationError|CompensationLog)\b/.test(source)
        || /system_core::orchestration(?:::\{[^}]*\b(?:ModuleOp|SagaStep|OrchestrationError|CompensationLog)\b|::(?:ModuleOp|SagaStep|OrchestrationError|CompensationLog)\b)/.test(source)
        || /\bSaga::new\s*\(/.test(source)) {
      failures.push(primitivePolicyFailure(path, 'active projection presents experimental orchestration as default or operational'))
    }
    if (!source.includes('experimental-orchestration') || !source.includes('system_core::experimental')) {
      failures.push(primitivePolicyFailure(path, 'active projection must require the explicit orchestration feature and path'))
    }
    if (!source.includes('Current Rust Runtime Profile')) {
      failures.push(primitivePolicyFailure(path, 'active projection must identify current Profile authority for DataTransport'))
    }
  }

  return failures
}

function applicationBoundaryFailure(path, evidence) {
  return { rule: 'TALOS-OPS-016', path, evidence, occurrences: 1 }
}

function runtimeFile(files, path) {
  const source = files[path]
  return source === undefined ? null : runtimeSource(source)
}

function structBody(source, name) {
  return source?.match(new RegExp(`(?:pub\\s+)?struct\\s+${name}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1] ?? null
}

function structFieldNames(body) {
  if (body === null) return []
  return body.split('\n').flatMap((line) => {
    const match = line.trim().match(/^(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:/)
    return match ? [match[1]] : []
  })
}

export function checkApplicationServicesBoundary(files) {
  const normalizedFiles = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), source]),
  )
  const failures = []
  const requiredPaths = [
    APPLICATION_MOD_PATH,
    APPLICATION_CLOCK_PATH,
    APPLICATION_MODULE_CLIENT_PATH,
    APPLICATION_SERVICES_PATH,
    APP_STATE_PATH,
    APP_MAIN_PATH,
  ]

  for (const path of requiredPaths) {
    if (!Object.hasOwn(normalizedFiles, path)) {
      failures.push(applicationBoundaryFailure(path, 'required ApplicationServices foundation source is missing'))
    }
  }
  if (failures.length > 0) return failures

  const applicationMod = runtimeFile(normalizedFiles, APPLICATION_MOD_PATH)
  const clock = runtimeFile(normalizedFiles, APPLICATION_CLOCK_PATH)
  const moduleClient = runtimeFile(normalizedFiles, APPLICATION_MODULE_CLIENT_PATH)
  const services = runtimeFile(normalizedFiles, APPLICATION_SERVICES_PATH)
  const state = runtimeFile(normalizedFiles, APP_STATE_PATH)
  const main = runtimeFile(normalizedFiles, APP_MAIN_PATH)

  for (const name of ['clock', 'module_client', 'services']) {
    if (!new RegExp(`\\bmod\\s+${name}\\s*;`).test(applicationMod)) {
      failures.push(applicationBoundaryFailure(APPLICATION_MOD_PATH, `application module must declare ${name}`))
    }
  }

  if (!/pub\s+trait\s+Clock\s*:\s*Send\s*\+\s*Sync/.test(clock)
      || !/fn\s+now_utc\s*\(\s*&self\s*\)\s*->\s*DateTime\s*<\s*Utc\s*>/.test(clock)
      || !/pub\s+struct\s+SystemClock\b/.test(clock)
      || !/impl\s+Clock\s+for\s+SystemClock\b/.test(clock)
      || !/pub\s+struct\s+FixedClock\b/.test(clock)
      || !/impl\s+Clock\s+for\s+FixedClock\b/.test(clock)) {
    failures.push(applicationBoundaryFailure(APPLICATION_CLOCK_PATH, 'Clock, SystemClock, and FixedClock contracts must remain complete'))
  }

  if (!/pub\s+trait\s+ModuleClient\s*:\s*Send\s*\+\s*Sync/.test(moduleClient)
      || !/pub\s+struct\s+RegistryModuleClient\b/.test(moduleClient)
      || !/registry\s*:\s*Arc\s*<\s*ModuleRegistry\s*>/.test(moduleClient)) {
    failures.push(applicationBoundaryFailure(APPLICATION_MODULE_CLIENT_PATH, 'ModuleClient and RegistryModuleClient contracts must remain complete'))
  }
  if (!/self\s*\.\s*registry\s*\.\s*execute\s*\(/.test(moduleClient)) {
    failures.push(applicationBoundaryFailure(APPLICATION_MODULE_CLIENT_PATH, 'RegistryModuleClient must delegate through ModuleRegistry::execute'))
  }

  const serviceFields = structBody(services, 'ApplicationServices')
  const serviceFieldNames = structFieldNames(serviceFields).sort()
  const expectedServiceFields = ['clock', 'module_client', 'repository_provider', 'worker_runner']
  if (serviceFields === null
      || JSON.stringify(serviceFieldNames) !== JSON.stringify(expectedServiceFields)
      || !/\bclock\s*:\s*Arc\s*<\s*dyn\s+Clock\s*>/.test(serviceFields)
      || !/\bmodule_client\s*:\s*Arc\s*<\s*dyn\s+ModuleClient\s*>/.test(serviceFields)
      || !/\bworker_runner\s*:\s*Arc\s*<\s*dyn\s+WorkerRunner\s*>/.test(serviceFields)
      || !/\brepository_provider\s*:\s*Arc\s*<\s*dyn\s+RepositoryProvider\s*>/.test(serviceFields)) {
    failures.push(applicationBoundaryFailure(APPLICATION_SERVICES_PATH, 'ApplicationServices must own exactly Clock, ModuleClient, WorkerRunner, and RepositoryProvider handles'))
  }
  if (serviceFields !== null && /\bpub\s+(?:clock|module_client|worker_runner|repository_provider)\s*:/.test(serviceFields)) {
    failures.push(applicationBoundaryFailure(APPLICATION_SERVICES_PATH, 'ApplicationServices fields must remain private'))
  }
  if (/pub\s+fn\s+set_(?:clock|module_client|worker_runner|repository_provider)\b/.test(services)) {
    failures.push(applicationBoundaryFailure(APPLICATION_SERVICES_PATH, 'ApplicationServices must not expose mutable service setters'))
  }

  const stateFields = structBody(state, 'AppState')
  if (stateFields === null || !/\bapplication_services\s*:\s*Arc\s*<\s*ApplicationServices\s*>/.test(stateFields)) {
    failures.push(applicationBoundaryFailure(APP_STATE_PATH, 'AppState must own the ApplicationServices aggregate'))
  }
  if (stateFields !== null && /\bpub\s+application_services\s*:/.test(stateFields)) {
    failures.push(applicationBoundaryFailure(APP_STATE_PATH, 'AppState ApplicationServices aggregate must remain private'))
  }
  if (stateFields !== null && /\b(?:pub\s+)?(?:clock|module_client|worker_runner|repository_provider|scoped_repositories|scheduler|key_store)\s*:/.test(stateFields)) {
    failures.push(applicationBoundaryFailure(APP_STATE_PATH, 'AppState must not expose naked application service fields'))
  }

  for (const path of [APPLICATION_MOD_PATH, APPLICATION_MODULE_CLIENT_PATH, APPLICATION_SERVICES_PATH]) {
    const source = runtimeFile(normalizedFiles, path)
    for (const match of source.matchAll(/(?:Utc|Local|SystemTime)\s*::\s*now\s*\(/g)) {
      failures.push(applicationBoundaryFailure(path, `wall-clock read outside Clock adapter: ${match[0]}`))
    }
  }

  for (const path of [APPLICATION_MOD_PATH, APPLICATION_CLOCK_PATH, APPLICATION_MODULE_CLIENT_PATH, APPLICATION_SERVICES_PATH]) {
    const source = runtimeFile(normalizedFiles, path)
    if (/(?:self\s*\.\s*)?registry\s*\.\s*get\s*\(/.test(source)
        || /SystemModule\s*::\s*execute\s*\(/.test(source)) {
      failures.push(applicationBoundaryFailure(path, 'application code must not bypass ModuleRegistry::execute'))
    }
    if (/HashMap\s*<\s*String\b|\bdyn\s+Any\b|\bdowncast(?:_ref|_mut)?\s*\(|\bget_service\s*\(\s*["']/.test(source)) {
      failures.push(applicationBoundaryFailure(path, 'generic service-locator patterns are forbidden'))
    }
  }

  const productionCalls = main.match(/ApplicationServices\s*::\s*production\s*\(/g)?.length ?? 0
  if (!/\bmod\s+application\s*;/.test(main)
      || productionCalls !== 1
      || !/AppState\s*::\s*new\s*\([\s\S]*?application_services/.test(main)) {
    failures.push(applicationBoundaryFailure(APP_MAIN_PATH, 'bootstrap must construct ApplicationServices exactly once and pass it to AppState'))
  }

  return failures
}

function walk(directory, predicate) {
  if (!existsSync(directory)) return []
  const found = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (['target', 'node_modules', '.git', '.talos-runtime'].includes(entry.name)) continue
    const path = join(directory, entry.name)
    if (entry.isDirectory()) found.push(...walk(path, predicate))
    else if (predicate(path)) found.push(path)
  }
  return found
}

function relevantCurrentPaths(root) {
  return [
    ...walk(join(root, 'backend'), (path) => path.endsWith('.rs') || path.endsWith('Cargo.toml')),
    ...walk(join(root, 'frontend', 'src'), (path) => /\.(?:ts|tsx|vue)$/.test(path)),
    ...['module-kit', 'templates', 'scaffold', 'generator', 'codegen'].flatMap((directory) =>
      walk(join(root, directory), () => true),
    ),
  ].map((path) => normalizePath(relative(root, path)))
}

function git(root, args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
}

function baselinePaths(root, commit) {
  const output = git(root, ['ls-tree', '-r', '--name-only', commit, '--', 'backend', 'frontend/src'])
  return output
    .split(/\r?\n/)
    .filter((path) => path && (path.endsWith('.rs') || path.endsWith('Cargo.toml') || /\.(?:ts|tsx|vue)$/.test(path)))
    .map(normalizePath)
}

function readBaselineFile(root, commit, path) {
  try {
    return git(root, ['show', `${commit}:${path}`])
  } catch {
    return null
  }
}

function loadFiles(root, paths, reader) {
  const files = {}
  for (const path of [...new Set(paths)].sort()) {
    const content = reader(path)
    if (content !== null && content !== undefined) files[path] = content
  }
  return files
}

function checkProposalDocuments(root) {
  const failures = []
  const proposalDirectory = join(root, 'policy', 'qualification', 'legacy-evidence', 'docs', 'proposals', 'talos-ops-business-closure')
  const required = [
    'system-design-record.md',
    'implementation-plan.md',
    'code-disposition-inventory.md',
    'final-review-report.md',
    'phase-0-closure.md',
  ]

  for (const name of required) {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*\.md$/.test(name) || !existsSync(join(proposalDirectory, name))) {
      failures.push({ rule: 'TALOS-OPS-010', path: normalizePath(relative(root, join(proposalDirectory, name))), evidence: 'required kebab-case proposal document is missing', occurrences: 1 })
    }
  }

  const indexPath = join(root, 'policy', 'qualification', 'legacy-evidence', 'docs', 'README.md')
  if (!existsSync(indexPath)) {
    failures.push({ rule: 'TALOS-OPS-010', path: 'policy/qualification/legacy-evidence/docs/README.md', evidence: 'documentation index is missing', occurrences: 1 })
    return failures
  }

  const index = readFileSync(indexPath, 'utf8')
  for (const name of required) {
    const expected = `proposals/talos-ops-business-closure/${name}`
    if (!index.includes(expected)) failures.push({ rule: 'TALOS-OPS-010', path: 'policy/qualification/legacy-evidence/docs/README.md', evidence: `missing index link for ${name}`, occurrences: 1 })
  }

  return failures
}

export function runRepositoryCheck({ root = process.cwd(), baselineCommit = BASELINE_COMMIT } = {}) {
  git(root, ['cat-file', '-e', `${baselineCommit}^{commit}`])

  const currentPaths = relevantCurrentPaths(root)
  const oldPaths = baselinePaths(root, baselineCommit)
  const allPaths = [...new Set([...currentPaths, ...oldPaths])]

  const currentFiles = loadFiles(root, allPaths, (path) => {
    const absolute = join(root, path)
    return existsSync(absolute) ? readFileSync(absolute, 'utf8') : null
  })
  const baselineFiles = loadFiles(root, allPaths, (path) => readBaselineFile(root, baselineCommit, path))

  const baselineFindings = collectFindings(baselineFiles)
  const currentFindings = collectFindings(currentFiles)
  const failures = [
    ...compareFindingSets(baselineFindings, currentFindings),
    ...checkProposalDocuments(root),
    ...checkRemovedLegacySfPaths(currentFiles),
    ...checkRemovedUnmountedReservationAdapterPath(currentFiles),
    ...checkRemovedLegacyExecutionContext(currentFiles),
    ...checkBoundedStaleDeadSuppressions(currentFiles),
    ...checkCoreExperimentalPrimitiveDisposition(currentFiles),
    ...checkApplicationServicesBoundary(currentFiles),
  ]

  return { baselineCommit, baselineFindings, currentFindings, failures }
}

function main() {
  const result = runRepositoryCheck()

  if (process.argv.includes('--print-baseline')) {
    console.log(JSON.stringify(result.baselineFindings, null, 2))
    return
  }

  if (result.failures.length > 0) {
    console.error(`TALOS Operations boundary check failed against ${result.baselineCommit}:`)
    for (const failure of result.failures) {
      console.error(`- ${failure.rule} ${failure.path}: ${failure.evidence} (+${failure.occurrences})`)
    }
    process.exitCode = 1
    return
  }

  console.log(`TALOS Operations boundary check passed against fingerprint baseline ${result.baselineCommit}.`)
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
