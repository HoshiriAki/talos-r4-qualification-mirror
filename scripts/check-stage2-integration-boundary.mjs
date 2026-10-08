#!/usr/bin/env node

import { readFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const scriptPath = fileURLToPath(import.meta.url)
const repositoryRoot = path.resolve(path.dirname(scriptPath), '..')

const requiredPaths = {
  module: 'backend/src/integration/module.rs',
  types: 'backend/src/integration/types.rs',
  catalog: 'backend/src/integration/catalog.rs',
  operation: 'backend/src/integration/operation.rs',
  store: 'backend/src/integration/store.rs',
  runtime: 'backend/src/integration/runtime.rs',
  transport: 'backend/src/integration/transport.rs',
  webhook: 'backend/src/integration/webhook.rs',
  worker: 'backend/src/integration/worker.rs',
  keystore: 'backend/src/integration/keystore.rs',
  routes: 'backend/src/routes/integrations.rs',
  observability: 'backend/src/middleware/observability.rs',
  viteProxy: 'backend/src/middleware/vite_proxy.rs',
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
  providerConfig: 'backend/src/registry/provider_config.rs',
  authorityMigration: 'backend/src/db/migrations/059_integration_fabric.sql',
  pgAuthorityMigration: 'backend/src/db/migrations/postgres/059_integration_fabric.sql',
  migration: 'backend/src/db/migrations/060_integration_runtime.sql',
  pgMigration: 'backend/src/db/migrations/postgres/060_integration_runtime.sql',
  schedulerMigration: 'backend/src/db/migrations/061_integration_scheduler_cursor.sql',
  pgSchedulerMigration: 'backend/src/db/migrations/postgres/061_integration_scheduler_cursor.sql',
  recoveryMigration: 'backend/src/db/migrations/062_integration_startup_recovery_snapshot.sql',
  pgRecoveryMigration: 'backend/src/db/migrations/postgres/062_integration_startup_recovery_snapshot.sql',
}

function requireText(errors, source, token, label) {
  if (!source.includes(token)) errors.push(`${label}: missing ${JSON.stringify(token)}`)
}

function forbidText(errors, source, token, label) {
  if (source.includes(token)) errors.push(`${label}: forbidden ${JSON.stringify(token)}`)
}

export async function loadStage2IntegrationSources(root = repositoryRoot) {
  const pairs = await Promise.all(
    Object.entries(requiredPaths).map(async ([key, relativePath]) => [
      key,
      await readFile(path.join(root, relativePath), 'utf8'),
    ]),
  )
  return Object.fromEntries(pairs)
}

export function validateStage2IntegrationBoundary(sources) {
  const errors = []

  for (const [label, source] of Object.entries(sources)) {
    if (typeof source !== 'string' || source.trim() === '') {
      errors.push(`${label}: missing source`)
    }
  }
  if (errors.length > 0) return errors

  for (const token of [
    'IntegrationModule',
    'ConfigValueType',
    'NonSecretConfigValue',
    'plan_refund_operation',
    'record_deposit_received',
    'reconcile_refund',
    'list_instances',
    'list_bindings',
    'create_webhook_endpoint',
    'list_webhook_endpoints',
    'list_webhook_dead_letters',
    'replay_webhook',
    'configured_secret_names',
    'Self::tenant_id(ctx)',
  ]) {
    requireText(errors, sources.module, token, 'integration module')
  }
  for (const token of [
    'SecretValueType',
    'SecretPurpose',
    'ApiVersion',
    'CompatibilityRule',
    'API versions must refer to declared capabilities',
    'compatibility rules must refer to declared capabilities',
  ]) {
    requireText(errors, sources.types, token, 'integration manifest types')
  }
  for (const token of ['schema_ref', 'BTreeMap<String, String>']) {
    forbidText(errors, sources.types, token, 'integration manifest types')
  }
  if (/struct\s+\w+Input\s*\{[^}]*tenant_id/s.test(sources.module)) {
    errors.push('integration module: request DTOs must not accept raw tenant_id authority')
  }

  for (const token of [
    'ExecutionMode::Normal',
    'RequiredSecretMissing',
    'config_revision != binding.config_revision',
  ]) {
    requireText(errors, sources.catalog, token, 'provider catalog')
  }

  for (const token of [
    'EffectIntent',
    'canonical_hash',
    'binding_revision',
    'request_fingerprint',
    'Sha256::digest',
  ]) {
    requireText(errors, sources.operation, token, 'external effect intent')
  }

  for (const token of [
    'UnknownOutcome',
    'recover_after_restart',
    'claim_next_operation',
    'record_runtime_outcome',
  ]) {
    requireText(errors, sources.runtime, token, 'external operation runtime')
  }
  for (const token of [
    'IntegrationError::CircuitOpen',
    'IntegrationError::RateLimited',
    'IntegrationError::ConcurrencyLimited',
    'if readiness != "fixture"',
    "state = 'unknown_outcome'",
    'validate_instance_against_manifest',
    'IntegrationError::BindingRevisionStale',
    'manifest_readiness != "fixture"',
    'provider manifest versions are immutable',
    'JOIN provider_manifests m',
    'list_webhook_dead_letters',
    'next_tenant_work_page',
    'integration_scheduler_cursor',
    'INTEGRATION_TENANT_PAGE_SIZE',
    'WHERE tenant_id > ?1',
    'begin_startup_recovery_snapshot',
    'recover_next_startup_snapshot_page',
    'integration_startup_recovery_snapshot',
  ]) {
    requireText(errors, sources.store, token, 'external operation persistence')
  }

  for (const token of [
    'WebhookVerifier',
    'WebhookApplicationPort',
    'record_rejected_webhook',
    'claim_next_webhook',
    'dead_letter_webhook',
    'replay',
    'FixtureWebhookIngress',
    'MAX_WEBHOOK_ATTEMPTS',
    'retry_budget_exhausted',
  ]) {
    requireText(errors, sources.webhook, token, 'webhook runtime')
  }

  for (const token of [
    'EgressPolicy',
    'Policy::none()',
    'read_limited_body',
    'Remote429',
    'ResponseTooLarge',
    'TransportCancellation',
    'LegacyHttpClientAdapter',
  ]) {
    requireText(errors, sources.transport, token, 'external transport')
  }
  for (const token of ['Scoped', 'Repository', 'orders', 'payments', 'shipments']) {
    forbidText(errors, sources.transport, token, 'external transport')
  }

  for (const token of [
    'std::env::var(variable)',
    'SecretAccessAudit',
    'SecretRegistrationScope',
    'allowed_capabilities',
    'allowed_purposes',
    'purpose: SecretPurpose',
    'INTEGRATION_FIXTURE_KEYSTORE_REGISTRATIONS',
  ]) {
    requireText(errors, sources.keystore, token, 'keystore capability/purpose ACL')
  }
  forbidText(errors, sources.providerConfig, 'std::env::var', 'legacy provider configuration')

  for (const token of ['IntegrationStore', '.pool', 'INSERT INTO', 'UPDATE ', 'DELETE FROM']) {
    forbidText(errors, sources.routes, token, 'integration HTTP route')
  }
  requireText(errors, sources.routes, '.execute("integration"', 'integration HTTP route')
  for (const token of [
    'get(list_instances)',
    'get(list_bindings)',
    'get(list_webhook_endpoints)',
    'get(list_webhook_dead_letters)',
  ]) {
    requireText(errors, sources.routes, token, 'integration HTTP route')
  }
  for (const token of [
    'FixtureIntegrationWorker',
    'MAX_WEBHOOKS_PER_TENANT_TICK',
    'begin_startup_recovery_snapshot',
    'recover_next_startup_snapshot_page',
    'run_scheduled_page',
    'self.run_scheduled_page().await',
  ]) {
    requireText(errors, sources.worker, token, 'fixture worker')
  }
  requireText(errors, sources.observability, '/api/integrations/webhooks/[REDACTED]', 'request observability')
  forbidText(errors, sources.observability, 'req.uri().clone()', 'request observability')
  requireText(errors, sources.viteProxy, 'preserves_webhook_bearer_path', 'Vite proxy bearer redaction')
  requireText(errors, sources.routes, 'fixture_webhook_rate_limit', 'webhook ingress rate limit')
  requireText(errors, sources.routes, 'ConnectInfo<SocketAddr>', 'webhook ingress peer identity')
  requireText(errors, sources.state, 'integration_webhook_ingress', 'webhook ingress composition')
  requireText(errors, sources.state, 'FixtureWebhookRateLimiter', 'webhook ingress rate limiter')
  for (const token of [
    'configured_fixture_key_store',
    'pg_pool.is_none().then(||',
    'IntegrationModule::new_with_postgres_persistence(',
    'assemble_with_metrics_audit_sink_integration_and_staff_module(',
  ]) {
    requireText(errors, sources.main, token, 'integration composition root')
  }

  for (const token of [
    'provider_instance_revision_guard',
    'provider_binding_identity_immutable',
    'external_operation_intent_immutable',
    'webhook_event_payload_conflict',
    'integration_deposit_received_state_guard',
    'integration_refund_operation_atomic_link',
  ]) {
    requireText(errors, sources.authorityMigration, token, 'sqlite stage 2 authority hardening')
  }
  for (const token of [
    'provider_instance_revision_guard',
    'provider_binding_identity_immutable',
    'external_operation_intent_immutable',
    'integration_webhook_identity_guard',
    'integration_deposit_received_state_guard',
    'integration_refund_operation_atomic_link',
  ]) {
    requireText(errors, sources.pgAuthorityMigration, token, 'postgres stage 2 authority hardening')
  }

  const runtimeHardeningTokens = [
    'external_operation_runtime_events',
    'webhook_processing_attempts',
    'webhook_replay_audit',
    'integration_deposit_reconciliations',
    'integration_instance_invalidates_queued_operations',
    'integration_binding_invalidates_queued_operations',
    'integration_manifest_invalidates_queued_operations',
    'manual_resolution_required',
    'integration_circuit_open',
    'integration_half_open_single_probe',
  ]
  for (const token of runtimeHardeningTokens) {
    requireText(errors, sources.migration, token, 'sqlite stage 2 runtime migration')
    requireText(errors, sources.pgMigration, token, 'postgres stage 2 runtime migration')
  }

  for (const [label, source] of [
    ['sqlite integration scheduler migration', sources.schedulerMigration],
    ['postgres integration scheduler migration', sources.pgSchedulerMigration],
  ]) {
    for (const token of ['integration_scheduler_cursor', 'fixture_integration_worker']) {
      requireText(errors, source, token, label)
    }
  }

  for (const [label, source] of [
    ['sqlite integration startup recovery migration', sources.recoveryMigration],
    ['postgres integration startup recovery migration', sources.pgRecoveryMigration],
  ]) {
    for (const token of [
      'integration_startup_recovery_snapshot',
      'fixture_integration_worker',
      "'external_operation'",
      "'webhook'",
    ]) {
      requireText(errors, source, token, label)
    }
  }

  return errors
}

async function main() {
  const errors = validateStage2IntegrationBoundary(await loadStage2IntegrationSources())
  if (errors.length > 0) {
    console.error('Stage 2 Integration Fabric boundary validation failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exit(1)
  }
  console.log('Stage 2 Integration Fabric boundary validation passed.')
}

if (path.resolve(process.argv[1] ?? '') === scriptPath) {
  main()
}
