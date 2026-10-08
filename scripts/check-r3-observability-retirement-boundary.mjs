#!/usr/bin/env node
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const root = path.resolve(process.env.TALOS_R3_OBS_ROOT || process.cwd())
const failures = []
const read = (relative) => {
  const file = path.join(root, relative)
  if (!fs.existsSync(file)) {
    failures.push(`missing ${relative}`)
    return ''
  }
  return fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n')
}
const requireTokens = (relative, tokens) => {
  const text = read(relative)
  for (const token of tokens) {
    if (!text.includes(token)) failures.push(`${relative}: missing ${JSON.stringify(token)}`)
  }
  return text
}
const requireAbsent = (relative) => {
  if (fs.existsSync(path.join(root, relative))) failures.push(`${relative}: retired source remains`)
}

const metrics = requireTokens('backend/src/observability.rs', [
  'pub(crate) trait MetricsSink',
  'RuntimeMetrics',
  'NoopMetrics',
  'talos_registry_commands_total',
  'talos_integration_operation_events_total',
  'talos_integration_attempts_total',
  'talos_financial_foundation_events_total',
  'talos_webhook_events_total',
  'talos_recovery_events_total',
  'talos_r3_settlement_commands_total',
  'UnknownOutcome',
  'ManualResolutionRequired',
  'IntegrationEvent::Admitted',
  'AttemptDispatchClass',
  'SettlementBlocker',
  'classify_settlement_error',
  'FinancialEvent',
  'AtomicU64',
  'Ordering::Relaxed',
  'const REGISTRY_MODULES',
  'const HTTP_ROUTES',
  'const INTEGRATION_EVENTS',
  'const WEBHOOK_OUTCOMES',
  'registry_metric_module',
  'registry_metric_command',
  'metrics_use_only_bounded_labels_and_never_render_secret_or_ids',
])
if (!/fn push_metric\(\s*output: &mut String,\s*name: &'static str,\s*labels: &\[\(&'static str, &'static str\)\]/s.test(metrics)) {
  failures.push('backend/src/observability.rs: exporter must accept only static label names and values')
}
for (const forbidden of ['tenant_id', 'request_id', 'operation_id', 'raw_payload', 'authorization', 'provider_event_id']) {
  if (new RegExp(`\\(\\s*"${forbidden}"\\s*,`).test(metrics)) {
    failures.push(`backend/src/observability.rs: forbidden rendered metric label ${forbidden}`)
  }
}
if (metrics.includes('Mutex<')) {
  failures.push('backend/src/observability.rs: metrics event path must not use a global mutex')
}

const registry = requireTokens('backend/src/registry/mod.rs', [
  'new_with_audit_sink_and_metrics',
  'registry_attempt',
  'registry_result',
  'classify_registry_error',
  'ResultClass::SystemError',
])
if (!/if let Err\(error\) = self\.audit_sink\.append\(attempt\)[\s\S]*?registry_result\([\s\S]*?ResultClass::SystemError[\s\S]*?return Err\(error\)/.test(registry)) {
  failures.push('backend/src/registry/mod.rs: attempt-audit failure must settle a system-error metric before returning')
}
for (const relative of ['backend/src/registry/mod.rs', 'backend/src/integration/worker.rs', 'backend/src/application/workers.rs']) {
  const source = read(relative).split('#[cfg(test)]', 1)[0]
  if (source.includes('error = %error') || source.includes('error = ?error')) {
    failures.push(`${relative}: raw error was added to an observability boundary`)
  }
}
requireTokens('backend/src/registry/assembler.rs', ['assemble_with_metrics', 'MetricsSink'])
requireTokens('backend/src/registry/factory.rs', [
  'r3_repository_provider',
  'SqliteRepositoryProvider::new_with_metrics',
  'R3SettlementModule::new(r3_repository_provider)',
])
const main = requireTokens('backend/src/main.rs', [
  'RuntimeMetrics::default',
  'assemble_with_metrics',
  'new_with_metrics',
  'FixtureWebhookIngress::new_with_metrics',
  'routes::create_router(state, metrics)',
])
if ((main.match(/RuntimeMetrics::default\(\)/g) || []).length !== 1) {
  failures.push('backend/src/main.rs: production composition must create exactly one RuntimeMetrics sink')
}
const state = requireTokens('backend/src/state.rs', [
  'pub struct AppState',
  'integration_webhook_ingress',
])
// TALOS-OPS-016 owns the exact AppState field baseline. This gate protects the
// observability escape hatch without duplicating that authority.
if (/\bmetrics\s*:\s*Arc\s*<\s*RuntimeMetrics\s*>/.test(state)) {
  failures.push('backend/src/state.rs: RuntimeMetrics must stay outside AppState at the composition seam')
}
const metricsRoute = requireTokens('backend/src/routes/metrics.rs', [
  'Extension(metrics)', 'metrics.render()', 'PlatformUser', 'MetricsAccess',
  'MetricsScrapeAuthority', 'authority.allows(parts)',
  'PlatformCapability::PlatformOperationsManage', 'text/plain; version=0.0.4',
  'metrics_route_denies_untrusted_tenant_authorities_and_allows_platform_operations',
])
if (!/impl FromRequestParts<Arc<crate::state::AppState>> for MetricsAccess[\s\S]*?authority\.allows\(parts\)[\s\S]*?return Ok\(Self\);[\s\S]*?PlatformUser::from_request_parts\(parts, state\)[\s\S]*?has_platform_capability\(PlatformCapability::PlatformOperationsManage\)[\s\S]*?return Err\(StatusCode::FORBIDDEN\);/.test(metricsRoute)) {
  failures.push('backend/src/routes/metrics.rs: metrics access must require exact scrape authority or platform operations capability')
}
if (!/async fn metrics\([\s\S]*?_access: MetricsAccess[\s\S]*?metrics\.render\(\)/.test(metricsRoute)) {
  failures.push('backend/src/routes/metrics.rs: metrics handler must require the MetricsAccess extractor')
}
const middleware = requireTokens('backend/src/middleware/observability.rs', ['http_request'])
const middlewareRuntime = middleware.split('#[cfg(test)]', 1)[0]
for (const token of ['/api/integrations/webhooks/[REDACTED]', '/api/v3/rental-settlement/commands/[COMMAND]']) {
  if (!middlewareRuntime.includes(token)) failures.push(`backend/src/middleware/observability.rs: missing ${JSON.stringify(token)}`)
}
if (middlewareRuntime.includes('debug=secret')) {
  failures.push('backend/src/middleware/observability.rs: test-only secret leaked into runtime middleware')
}
requireTokens('backend/src/integration/runtime.rs', [
  'new_with_metrics',
  'IntegrationEvent::Claimed',
  'IntegrationEvent::UnknownOutcome',
  'IntegrationEvent::Rejected',
  'IntegrationEvent::RetryScheduled',
  'integration_attempt',
  'attempt_elapsed',
  'claim_error_metric',
  'IntegrationEvent::Deferred',
  'IntegrationEvent::BlockedConfiguration',
  'IntegrationEvent::SystemFailure',
  'IntegrationEvent::NonRetryableFailure',
  'AttemptOutcome::PersistenceFailure',
])
const runtime = read('backend/src/integration/runtime.rs')
if (/IntegrationEvent::ManualResolutionRequired,\s*OperationStateClass::NonRetryableFailure/.test(runtime)) {
  failures.push('backend/src/integration/runtime.rs: NonRetryableFailure must not be manual resolution')
}
if (!/if let Err\(error\) =[\s\S]*?record_runtime_outcome\(&claimed, outcome, &self\.policy\)[\s\S]*?IntegrationEvent::SystemFailure,[\s\S]*?OperationStateClass::Dispatching,[\s\S]*?AttemptOutcome::PersistenceFailure,[\s\S]*?dispatch_class,[\s\S]*?return Err\(error\);/s.test(runtime)) {
  failures.push('backend/src/integration/runtime.rs: outcome persistence failure must settle system metrics while durable state remains dispatching')
}
requireTokens('backend/src/integration/store.rs', [
  'with_key_store_and_metrics',
  'IntegrationEvent::Admitted',
  'IntegrationEvent::ReconciliationStarted',
  'IntegrationEvent::ReconciliationResolved',
  'FinancialEvent::DepositRecorded',
  'FinancialEvent::RefundReconciliation',
])
const worker = requireTokens('backend/src/integration/worker.rs', [
  'RecoverySource::StartupSnapshot',
  'RecoverySource::TenantPage',
  'RecoverySource::ScheduledPage',
  'RecoveryOutcome::Started',
  'RecoveryOutcome::Failed',
  'RecoveryOutcome::Recovered',
])
if (!/run_startup[\s\S]*?RecoveryOutcome::Started[\s\S]*?RecoveryOutcome::Failed/.test(worker)
  || !/run_scheduled_page[\s\S]*?RecoveryOutcome::Started[\s\S]*?RecoveryOutcome::(Empty|Failed)/.test(worker)) {
  failures.push('backend/src/integration/worker.rs: startup and scheduled lifecycle must settle started recovery metrics')
}
const webhook = requireTokens('backend/src/integration/webhook.rs', [
  'WebhookOutcome::Pending',
  'WebhookOutcome::SystemFailure',
  'WebhookEvent::Verified',
  'WebhookEvent::Duplicate',
  'WebhookEvent::DeadLettered',
  'WebhookEvent::Replayed',
  'WebhookEvent::SystemFailure',
  'persist_processing_terminal',
])
if (/WebhookEvent::Received\s*,\s*WebhookOutcome::Accepted/.test(webhook)) {
  failures.push('backend/src/integration/webhook.rs: receipt is classified accepted before verification')
}
if (!/fn persist_processing_terminal\([\s\S]*?match persistence \{[\s\S]*?Ok\(\(\)\) => \{[\s\S]*?webhook\(event, outcome\)[\s\S]*?Err\(error\) => \{[\s\S]*?WebhookEvent::SystemFailure,[\s\S]*?WebhookOutcome::SystemFailure[\s\S]*?Err\(error\)/s.test(webhook)) {
  failures.push('backend/src/integration/webhook.rs: every started webhook must settle a bounded processing terminal')
}
requireTokens('backend/src/integration/settlement_admission.rs', ['created_new: bool', 'created_new: false', 'created_new: true'])
const r3SettlementRepository = requireTokens('backend/src/repositories/r3_settlement.rs', [
  'let (effect, created_new) = self.session.write_immediate',
  'if created_new',
  'IntegrationEvent::Admitted',
  'OperationStateClass::Ready',
])
if (!/let \(effect, created_new\) = self\.session\.write_immediate[\s\S]*?\}\)\?;[\s\S]*?if created_new/.test(r3SettlementRepository)) {
  failures.push('backend/src/repositories/r3_settlement.rs: R3 admission metric must occur only after transaction commit')
}
requireTokens('backend/src/routes/r3_settlement.rs', [
  'SettlementOutcome',
  'SettlementBlocker',
  'classify_settlement_error',
  'Extension(metrics)',
  'metrics.settlement',
])
requireTokens('backend/src/application/workers.rs', [
  'new_with_metrics',
  'WorkflowEvent::StepProcessed',
  'WorkflowEvent::StepCompleted',
  'WorkflowEvent::RetryScheduled',
  'WorkflowEvent::CompensationScheduled',
  'WorkflowEvent::ManualResolutionRequired',
  'WorkflowEvent::OutboxDelivered',
  'WorkflowEvent::InboxConsumed',
])

const routes = requireTokens('backend/src/routes/mod.rs', [
  'metrics::metrics_routes()', 'r3_settlement::r3_settlement_routes()',
  'create_router(state: Arc<AppState>, metrics: Arc<RuntimeMetrics>)',
  'Extension(metrics.clone())',
])
if (!/from_fn_with_state\(\s*metrics,/.test(routes)) failures.push('backend/src/routes/mod.rs: observability middleware is not composed with the shared sink')
for (const forbidden of ['debug_data', 'pub mod sync', 'sync::sync_routes', 'debug_routes', 'sync_routes']) {
  if (routes.includes(forbidden)) failures.push(`backend/src/routes/mod.rs: retired runtime surface ${forbidden}`)
}
requireAbsent('backend/src/routes/debug_data.rs')
requireAbsent('backend/src/routes/sync.rs')
requireAbsent('scripts/fetch-server-data.mjs')
requireTokens('scripts/sync-from-remote-db.js', ['TALOS_ALLOW_LEGACY_LOCAL_DB_SYNC=1'])
for (const relative of ['backend/src/main.rs', 'backend/src/config.rs']) {
  if (read(relative).includes('REMOTE_SYNC_')) failures.push(`${relative}: retired remote-sync runtime residue remains`)
}
const legacyScrape = requireTokens('scripts/scrape-server-data.mjs', [
  'TALOS_ALLOW_LEGACY_SCRAPE=1',
  '无法解析 session cookie',
])
if (legacyScrape.includes('无法解析 cookie: ${setCookie}') || legacyScrape.includes('JSON.stringify(loginResult.data)')) {
  failures.push('scripts/scrape-server-data.mjs: legacy diagnostic output leaks response or cookie data')
}

requireTokens('frontend/src/composables/useOrdersWorkbench.ts', ['deviceSerialNo'])
requireTokens('backend/src/routes/orders.rs', ['/users'])
requireTokens('backend/src/routes/backup.rs', ['/api/tenant/backup-status', 'AdminUser'])
requireTokens('policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r3-sp-26-observability.md', [
  'TALOS-OPS-R3-SP-26',
  '## TALOS Conformance',
  'TALOS-OBS-001',
  'UnknownOutcome',
  'SP-21',
  'SP-22',
])
requireTokens('policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r3-sp-27-debug-sync-retirement.md', [
  'TALOS-OPS-R3-SP-27',
  '## Inventory and classification',
  '| Surface | Location | Classification | Action | Evidence | Future status |',
  'REMOVE',
  'RESTRICT',
  'KEEP_WITH_PROFILE',
  '/users',
  'deviceSerialNo',
  '/api/tenant/backup-status',
  '## TALOS Conformance',
])
requireTokens('package.json', [
  'quality:r3-observability-retirement:test',
  'quality:r3-observability-retirement',
])

if (failures.length) {
  process.stderr.write('TALOS-OPS-R3 observability/retirement boundary failed:\n')
  for (const failure of failures) process.stderr.write(`- ${failure}\n`)
  process.exit(1)
}
process.stdout.write('TALOS-OPS-R3 observability/retirement boundary passed.\n')
