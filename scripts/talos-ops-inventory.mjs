#!/usr/bin/env node

import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import process from 'node:process'

const root = process.cwd()
const reportDirectory = join(root, '.talos-runtime', 'reports')
const sourceRoots = ['backend/src', 'backend/official', 'backend/system', 'backend/thirdparty']
const coreTables = new Set(['orders', 'booking', 'bookings', 'reservations', 'reservation', 'payments', 'payment', 'shipments', 'shipment_records', 'webhooks', 'webhook_events'])

function walk(directory, predicate) {
  if (!existsSync(directory)) return []
  const found = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === 'target' || entry.name === 'node_modules' || entry.name === '.git') continue
    const path = join(directory, entry.name)
    if (entry.isDirectory()) found.push(...walk(path, predicate))
    else if (predicate(path)) found.push(path)
  }
  return found
}

const rustFiles = sourceRoots.flatMap((directory) => walk(join(root, directory), (path) => path.endsWith('.rs')))
const byRelativePath = new Map(rustFiles.map((path) => [relative(root, path).replaceAll('\\', '/'), readFileSync(path, 'utf8')]))
const routesModule = byRelativePath.get('backend/src/routes/mod.rs') ?? ''
const servicesModule = byRelativePath.get('backend/src/services/mod.rs') ?? ''
const assembler = byRelativePath.get('backend/src/registry/assembler.rs') ?? ''

function pathText(path) {
  return byRelativePath.get(path) ?? ''
}

function textReferences(needles, ownPaths = []) {
  const own = new Set(ownPaths)
  return [...byRelativePath]
    .filter(([path, text]) => !own.has(path) && needles.some((needle) => text.includes(needle)))
    .map(([path]) => path)
    .sort()
}

function tables(path) {
  const text = pathText(path).split(/#\[cfg\(test\)\]/, 1)[0]
  const read = new Set()
  const write = new Set()
  for (const match of text.matchAll(/\b(?:FROM|JOIN)\s+([a-zA-Z_][\w]*)/gi)) read.add(match[1])
  for (const match of text.matchAll(/\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+([a-zA-Z_][\w]*)/gi)) write.add(match[1])
  return { read: [...read].sort(), write: [...write].sort() }
}

function routeState(file, routeFunction) {
  const moduleName = file.split('/').at(-1).replace(/\.rs$/, '')
  const source = pathText(file)
  return {
    compiled: routesModule.includes(`pub mod ${moduleName};`),
    registered: routesModule.includes(`${routeFunction}(`),
    declared_routes: [...source.matchAll(/\.route\(\s*"([^"]+)"/g)].map((match) => match[1]),
  }
}

const components = [
  { id: 'order-route', path: 'backend/src/routes/orders.rs', kind: 'route', routeFunction: 'orders::order_routes', needles: ['order_service::', 'FeatureOrder'] },
  { id: 'order-compatibility-support', path: 'backend/src/services/order_compatibility_support.rs', kind: 'service', needles: ['order_compatibility_support::'] },
  { id: 'official-order-module', path: 'backend/official/order/src/order.rs', kind: 'module', needles: ['FeatureOrder', 'official_order'] },
  { id: 'booking', path: 'backend/src/routes/booking.rs', kind: 'route', routeFunction: 'booking::booking_routes', needles: ['"booking"', 'FeatureBooking'] },
  { id: 'reservation-route', path: 'backend/src/routes/reservation.rs', kind: 'route', routeFunction: 'reservation::reservation_routes', needles: ['"reservation"', 'FeatureReservation'] },
  { id: 'reservation-module', path: 'backend/system/admin/src/reservation.rs', kind: 'module', needles: ['FeatureReservation'] },
  { id: 'legacy-sf-route', path: 'backend/src/routes/sf_express.rs', kind: 'route', routeFunction: 'sf_express::sf_express_routes', needles: ['sf_express_routes', 'SfProvider'] },
  { id: 'legacy-sf-service', path: 'backend/src/services/sf_express.rs', kind: 'service', needles: ['SfProvider', 'sf_express::'] },
  { id: 'thirdparty-sf-connector', path: 'backend/thirdparty/sf-express/src/lib.rs', kind: 'module', needles: ['FeatureSfExpress', 'sf_express'] },
  { id: 'webhooks', path: 'backend/src/routes/webhooks.rs', kind: 'route', routeFunction: 'webhooks::sf_express_routes', needles: ['webhook', 'FeatureSfExpress'] },
  { id: 'api-keys', path: 'backend/src/services/api_key_service.rs', kind: 'service', needles: ['validate_api_key', 'api_key_service::'] },
  { id: 'metrics', path: 'backend/src/routes/metrics.rs', kind: 'route', routeFunction: 'metrics::metrics_routes', needles: ['metrics_routes'] },
  { id: 'time-helpers', path: 'backend/src/utils/time.rs', kind: 'utility', needles: ['shanghai_now'] },
  { id: 'application-services', path: 'backend/src/application/services.rs', kind: 'application', needles: ['ApplicationServices'] },
  { id: 'application-clock', path: 'backend/src/application/clock.rs', kind: 'application', needles: ['Clock', 'SystemClock', 'FixedClock'] },
  { id: 'application-module-client', path: 'backend/src/application/module_client.rs', kind: 'application', needles: ['ModuleClient', 'RegistryModuleClient'] },
  { id: 'application-workers', path: 'backend/src/application/workers.rs', kind: 'application', needles: ['WorkerRunner', 'WorkerContextFactory', 'MaintenanceWorkerRunner'] },
  { id: 'app-state-compatibility', path: 'backend/src/state.rs', kind: 'application', needles: ['application_services', 'pool', 'registry', 'http_client'] },
  { id: 'bootstrap-worker-runner', path: 'backend/src/main.rs', kind: 'application', needles: ['ApplicationServices::production', 'WorkerRunner', 'spawn_periodic'] },
  { id: 'repository-scope-core-contract', path: 'backend/system/core/src/repository.rs', kind: 'core', needles: ['RepositoryScope', 'data_scope'] },
  { id: 'repository-binding', path: 'backend/src/repositories/contracts/binding.rs', kind: 'repository', needles: ['RepositoryBinding', 'RepositoryAccess', 'from_execution'] },
  { id: 'repository-provider', path: 'backend/src/repositories/contracts/provider.rs', kind: 'repository', needles: ['RepositoryProvider', 'ScopedRepositories', 'bind'] },
  { id: 'repository-error', path: 'backend/src/repositories/contracts/error.rs', kind: 'repository', needles: ['RepositoryError', 'REPOSITORY_PREVIEW_WRITE_DENIED'] },
  { id: 'sqlite-repository-provider', path: 'backend/src/repositories/sqlite/provider.rs', kind: 'repository', needles: ['SqliteRepositoryProvider', 'Pool<SqliteConnectionManager>'] },
  { id: 'sqlite-scoped-session', path: 'backend/src/repositories/sqlite/session.rs', kind: 'repository', needles: ['SqliteRepositorySession', 'PreviewWriteDenied'] },
  { id: 'registry-descriptor-catalog', path: 'backend/src/registry/descriptors.rs', kind: 'registry', needles: ['ModuleDescriptor', 'ModuleFactoryId', 'registry_key', 'metadata_name', 'schema_name'] },
  { id: 'registry-module-factory', path: 'backend/src/registry/factory.rs', kind: 'registry', needles: ['ModuleFactory', 'ConstructionReceipt', 'validate_descriptor_projection'] },
  { id: 'registry-provider-config', path: 'backend/src/registry/provider_config.rs', kind: 'registry', needles: ['ProviderDeploymentConfig', 'from_env', 'init_payload'] },
  { id: 'registry-assembler', path: 'backend/src/registry/assembler.rs', kind: 'registry', needles: ['ModuleRegistry::assemble', 'ModuleFactory::new'] },
  { id: 'core-transport-contract', path: 'backend/system/core/src/lib.rs', kind: 'core', needles: ['DataTransport', 'TransportMetadata', 'TransportPriority', 'TransportOptions', 'TransportMessage', 'TransportHealth'] },
  { id: 'experimental-orchestration-primitives', path: 'backend/system/core/src/experimental/orchestration.rs', kind: 'core', needles: ['ModuleOp', 'SagaStep', 'OrchestrationError', 'CompensationLog'] },
]

const componentReport = components.map((component) => {
  const source = pathText(component.path)
  const moduleName = component.path.split('/').at(-1).replace(/\.rs$/, '')
  const relation = component.kind === 'route'
    ? routeState(component.path, component.routeFunction)
    : component.kind === 'service'
      ? { compiled: servicesModule.includes(`pub mod ${moduleName};`), registered: false }
      : { compiled: source.length > 0, registered: component.needles.some((needle) => assembler.includes(needle)) }
  return {
    id: component.id,
    path: component.path,
    exists: source.length > 0,
    ...relation,
    // Static references are intentionally not presented as runtime callers:
    // registration and reachability require source-specific review.
    text_references: textReferences(component.needles, [component.path]),
    tables: tables(component.path),
    tenant_scope_markers: /tenant[_ -]?id|DataScope|data_scope|tenant\(/i.test(source),
    registry_markers: /registry\.execute|SystemModule|Feature[A-Z]/.test(source),
    preview_simulation_markers: /preview|simulation/i.test(source),
  }
})

const directRouteWrites = []
const localShanghaiHelpers = []
const i64OrderIds = []
const deadCodeSuppressions = []
for (const [path, text] of byRelativePath) {
  // Exclude test-only fixtures from the operational inventory.
  const runtimeText = text.split(/#\[cfg\(test\)\]/, 1)[0]
  const lines = runtimeText.split(/\r?\n/)
  lines.forEach((line, index) => {
    const write = line.match(/\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+([a-zA-Z_][\w]*)/i)
    if (path.startsWith('backend/src/routes/') && write && coreTables.has(write[1].toLowerCase())) {
      directRouteWrites.push({ path, line: index + 1, table: write[1], text: line.trim() })
    }
    if (/\bfn\s+[a-zA-Z_]*shanghai_now[a-zA-Z_]*/.test(line)) localShanghaiHelpers.push({ path, line: index + 1, text: line.trim() })
    if (/\border_id\s*:\s*i64\b/.test(line)) i64OrderIds.push({ path, line: index + 1, text: line.trim() })
    if (/allow\(dead_code\)/.test(line)) deadCodeSuppressions.push({ path, line: index + 1, text: line.trim() })
  })
}

const report = {
  generated_at: new Date().toISOString(),
  scope: 'repository-local static inventory; it does not prove runtime execution',
  components: componentReport,
  findings: { direct_route_writes: directRouteWrites, local_shanghai_helpers: localShanghaiHelpers, i64_order_id_definitions: i64OrderIds, dead_code_suppressions: deadCodeSuppressions },
}

if (!process.argv.includes('--no-write')) {
  mkdirSync(reportDirectory, { recursive: true })
  writeFileSync(join(reportDirectory, 'talos-ops-inventory.json'), `${JSON.stringify(report, null, 2)}\n`)
}

console.log(`TALOS_OPS_INVENTORY components=${componentReport.length} route_writes=${directRouteWrites.length} local_shanghai_helpers=${localShanghaiHelpers.length} i64_order_ids=${i64OrderIds.length} dead_code_suppressions=${deadCodeSuppressions.length}`)
console.log(JSON.stringify(report, null, 2))
