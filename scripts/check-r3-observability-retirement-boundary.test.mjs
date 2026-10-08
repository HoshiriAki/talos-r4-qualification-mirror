#!/usr/bin/env node
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { spawnSync } from 'node:child_process'

const root = process.cwd()
const checker = path.join(root, 'scripts/check-r3-observability-retirement-boundary.mjs')
const files = [
  'backend/src/observability.rs', 'backend/src/registry/mod.rs', 'backend/src/registry/assembler.rs', 'backend/src/registry/factory.rs',
  'backend/src/main.rs', 'backend/src/config.rs', 'backend/src/state.rs', 'backend/src/routes/metrics.rs', 'backend/src/middleware/observability.rs',
  'backend/src/integration/runtime.rs', 'backend/src/integration/worker.rs', 'backend/src/integration/webhook.rs',
  'backend/src/integration/store.rs', 'backend/src/integration/settlement_admission.rs',
  'backend/src/repositories/r3_settlement.rs',
  'backend/src/routes/r3_settlement.rs', 'backend/src/application/workers.rs', 'backend/src/routes/mod.rs',
  'backend/src/routes/orders.rs', 'backend/src/routes/backup.rs', 'frontend/src/composables/useOrdersWorkbench.ts',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r3-sp-26-observability.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r3-sp-27-debug-sync-retirement.md', 'package.json',
  'scripts/sync-from-remote-db.js', 'scripts/scrape-server-data.mjs',
]
function run(fixtureRoot = root) {
  return spawnSync(process.execPath, [checker], {
    cwd: root,
    env: { ...process.env, TALOS_R3_OBS_ROOT: fixtureRoot },
    encoding: 'utf8',
  })
}
function fixture() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'talos-r3-obs-'))
  for (const relative of files) {
    const target = path.join(temp, relative)
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.copyFileSync(path.join(root, relative), target)
  }
  return temp
}
function expectFailure(name, relative, search, replacement, evidence) {
  const temp = fixture()
  try {
    const file = path.join(temp, relative)
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll(search, replacement))
    const result = run(temp)
    if (result.status === 0 || !`${result.stdout}${result.stderr}`.includes(evidence)) {
      throw new Error(`${name} did not fail closed\n${result.stdout}\n${result.stderr}`)
    }
  } finally {
    fs.rmSync(temp, { recursive: true, force: true })
  }
}

const baseline = run()
if (baseline.status !== 0) throw new Error(`baseline observability gate failed\n${baseline.stdout}\n${baseline.stderr}`)
expectFailure('injectable metrics boundary', 'backend/src/observability.rs', 'pub(crate) trait MetricsSink', 'trait RemovedMetricsSink', 'MetricsSink')
expectFailure('lock-free metrics lanes', 'backend/src/observability.rs', 'AtomicU64', 'Mutex<', 'global mutex')
expectFailure('R3 route metric', 'backend/src/routes/r3_settlement.rs', 'metrics.settlement', 'metrics.removed', 'metrics.settlement')
expectFailure('R3 terminal blocker classifier', 'backend/src/routes/r3_settlement.rs', 'classify_settlement_error', 'removed_settlement_classifier', 'classify_settlement_error')
expectFailure('AppState metrics escape hatch', 'backend/src/state.rs', 'application_services: Arc<ApplicationServices>,', 'metrics: Arc<RuntimeMetrics>,\n    application_services: Arc<ApplicationServices>,', 'RuntimeMetrics must stay outside AppState')
expectFailure('one shared production metrics composition', 'backend/src/main.rs', 'routes::create_router(state, metrics)', 'routes::create_router(state, Arc::new(RuntimeMetrics::default()))', 'production composition must create exactly one RuntimeMetrics sink')
expectFailure('metrics platform capability boundary', 'backend/src/routes/metrics.rs', 'PlatformCapability::PlatformOperationsManage', 'PlatformCapability::TenantDiagnosticsRead', 'metrics access must require exact scrape authority or platform operations capability')
expectFailure('metrics scrape authority boundary', 'backend/src/routes/metrics.rs', 'authority.allows(parts)', 'true', 'metrics access must require exact scrape authority or platform operations capability')
expectFailure('metrics handler extractor boundary', 'backend/src/routes/metrics.rs', '_access: MetricsAccess', '_access: ()', 'metrics handler must require the MetricsAccess extractor')
expectFailure('registry audit terminal result', 'backend/src/registry/mod.rs', 'ResultClass::SystemError', 'ResultClass::BusinessError', 'ResultClass::SystemError')
expectFailure('webhook neutral receipt', 'backend/src/integration/webhook.rs', 'WebhookOutcome::Pending', 'WebhookOutcome::Accepted', 'receipt is classified accepted before verification')
expectFailure('webhook processing persistence terminal', 'backend/src/integration/webhook.rs', 'fn persist_processing_terminal', 'fn removed_processing_terminal', 'every started webhook must settle a bounded processing terminal')
expectFailure('recovery terminal failure', 'backend/src/integration/worker.rs', 'RecoveryOutcome::Failed', 'RecoveryOutcome::Empty', 'lifecycle must settle started recovery metrics')
expectFailure('non-retryable terminal classification', 'backend/src/integration/runtime.rs', 'IntegrationEvent::NonRetryableFailure', 'IntegrationEvent::ManualResolutionRequired', 'NonRetryableFailure must not be manual resolution')
expectFailure('operation outcome persistence terminal', 'backend/src/integration/runtime.rs', 'AttemptOutcome::PersistenceFailure', 'AttemptOutcome::Succeeded', 'outcome persistence failure must settle system metrics')
expectFailure('post-commit R3 admission', 'backend/src/repositories/r3_settlement.rs', 'if created_new', 'if false', 'after transaction commit')
expectFailure('R3 idempotent admission marker', 'backend/src/integration/settlement_admission.rs', 'created_new: false', 'created_new: true', 'created_new: false')
expectFailure('R3 metrics provider wiring', 'backend/src/registry/factory.rs', 'R3SettlementModule::new(r3_repository_provider)', 'R3SettlementModule::new(repository_provider.clone())', 'r3_repository_provider')
expectFailure('webhook redaction', 'backend/src/middleware/observability.rs', '[REDACTED]', '[LEAKED]', '[REDACTED]')
expectFailure('retired sync route', 'backend/src/routes/mod.rs', 'pub mod api_keys;', 'pub mod sync;\npub mod api_keys;', 'retired runtime surface')
expectFailure('SP-27 classification', 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r3-sp-27-debug-sync-retirement.md', 'KEEP_WITH_PROFILE', 'UNCLASSIFIED', 'KEEP_WITH_PROFILE')
expectFailure('legacy sync restriction', 'scripts/sync-from-remote-db.js', 'TALOS_ALLOW_LEGACY_LOCAL_DB_SYNC=1', 'LEGACY_SYNC_REMOVED', 'TALOS_ALLOW_LEGACY_LOCAL_DB_SYNC=1')
const crlf = fixture()
try {
  for (const relative of files) {
    const file = path.join(crlf, relative)
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n').replaceAll('\n', '\r\n'))
  }
  const result = run(crlf)
  if (result.status !== 0) throw new Error(`CRLF parity failed\n${result.stdout}\n${result.stderr}`)
} finally {
  fs.rmSync(crlf, { recursive: true, force: true })
}
process.stdout.write('TALOS-OPS-R3 observability/retirement negative fixtures and CRLF parity passed.\n')
