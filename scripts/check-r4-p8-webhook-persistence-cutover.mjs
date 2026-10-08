#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  integrationMod: 'backend/src/integration/mod.rs',
  contract: 'backend/src/integration/webhook_persistence_contract.rs',
  sqliteAdapter: 'backend/src/integration/webhook_persistence_sqlite.rs',
  postgresAdapter: 'backend/src/integration/webhook_persistence_postgres.rs',
  webhookRuntime: 'backend/src/integration/webhook.rs',
  governedWorker: 'backend/src/integration/governed_worker.rs',
  integrationModule: 'backend/src/integration/module.rs',
  liveQualification: 'backend/src/integration/webhook_persistence_postgres_tests.rs',
  migrationFabric: 'backend/src/db/migrations/postgres/059_integration_fabric.sql',
  migrationRuntime: 'backend/src/db/migrations/postgres/060_integration_runtime.sql',
  main: 'backend/src/main.rs',
}

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

export function collectP8WebhookPersistenceSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8WebhookPersistenceSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) mod webhook_persistence_contract;',
    'pub(crate) mod webhook_persistence_postgres;',
    'pub(crate) mod webhook_persistence_sqlite;',
    'mod webhook_persistence_postgres_tests;',
  ]) {
    if (!snapshot.integrationMod.includes(token)) {
      errors.push(`Webhook persistence module wiring missing: ${token}`)
    }
  }

  for (const token of [
    'pub(crate) trait WebhookReplayPersistence: Send + Sync',
    'pub(crate) trait WebhookRuntimePersistence: WebhookReplayPersistence',
    'pub(crate) trait WebhookAdminPersistence: WebhookReplayPersistence',
    'pub struct WebhookEndpointSummary',
    'pub struct WebhookDeadLetterSummary',
    'fn resolve_webhook_endpoint(',
    'fn record_verified_webhook(',
    'fn record_rejected_webhook(',
    'fn claim_next_webhook(',
    'fn complete_webhook(',
    'fn retry_webhook(',
    'fn dead_letter_webhook(',
    'fn register_webhook_endpoint(',
    'fn list_webhook_endpoints(',
    'fn set_webhook_endpoint_enabled(',
    'fn list_webhook_dead_letters(',
    'fn replay_webhook(',
  ]) {
    if (!snapshot.contract.includes(token)) {
      errors.push(`Webhook persistence contract missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.contract)) {
    errors.push('Webhook persistence contract must remain backend-neutral')
  }

  for (const token of [
    'impl WebhookReplayPersistence for IntegrationStore',
    'impl WebhookRuntimePersistence for IntegrationStore',
    'impl WebhookAdminPersistence for IntegrationStore',
  ]) {
    if (!snapshot.sqliteAdapter.includes(token)) {
      errors.push(`SQLite Webhook compatibility adapter missing: ${token}`)
    }
  }

  for (const token of [
    'persistence: Arc<dyn WebhookRuntimePersistence>',
    'from_persistence(',
    'from_postgres(',
    'PostgresWebhookPersistence::new(pool)',
    'runtime: WebhookRuntime::from_postgres_with_metrics(pool, metrics)',
    'runtime_accepts_webhook_persistence_port_without_integration_store',
    '.resolve_webhook_endpoint(',
    '.record_verified_webhook(',
    '.claim_next_webhook(',
    '.complete_webhook(',
    '.dead_letter_webhook(',
  ]) {
    if (!snapshot.webhookRuntime.includes(token)) {
      errors.push(`Webhook Runtime dependency inversion missing: ${token}`)
    }
  }
  const runtimeStruct = between(
    snapshot.webhookRuntime,
    'pub struct WebhookRuntime {',
    'const FIXTURE_WEBHOOK_SIGNATURE',
  )
  if (runtimeStruct.includes('store: IntegrationStore')) {
    errors.push('Webhook Runtime must not own a concrete IntegrationStore field')
  }

  const runtimeImpl = between(
    snapshot.webhookRuntime,
    'impl WebhookRuntime {',
    'fn webhook_failure_outcome',
  )
  if (runtimeImpl.includes('self.store')) {
    errors.push('Webhook Runtime must not bypass persistence port through IntegrationStore')
  }

  for (const token of [
    'webhook_admin: Arc<dyn WebhookAdminPersistence>',
    'new_with_webhook_admin(',
    'new_with_postgres_webhook(',
    '.webhook_admin',
    '.register_webhook_endpoint(',
    '.list_webhook_endpoints(',
    '.set_webhook_endpoint_enabled(',
    '.list_webhook_dead_letters(',
    '.replay_webhook(',
  ]) {
    if (!snapshot.integrationModule.includes(token)) {
      errors.push(`Webhook admin dependency inversion missing: ${token}`)
    }
  }

  for (const token of [
    'impl WebhookReplayPersistence for PostgresWebhookPersistence',
    'impl WebhookRuntimePersistence for PostgresWebhookPersistence',
    'impl WebhookAdminPersistence for PostgresWebhookPersistence',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY',
    'WHERE i.tenant_id=$1',
    'FOR UPDATE OF i,e,b,p SKIP LOCKED',
    'ON CONFLICT (tenant_id,endpoint_id,provider_event_id) DO NOTHING',
    'WebhookUnverifiable',
    'webhook event id reused with different payload',
    'webhook_processing_attempts',
    'webhook_dead_letters',
    'webhook_replay_audit',
    "status='processing'",
    "status='dead_letter'",
    "status='verified'",
    'WHERE b.tenant_id=$1 AND b.id=$2',
  ]) {
    if (!snapshot.postgresAdapter.includes(token)) {
      errors.push(`PostgreSQL Webhook persistence invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresAdapter)) {
    errors.push('PostgreSQL Webhook persistence must not depend on SQLite runtime types')
  }
  for (const forbidden of [
    'integration_scheduler_cursor',
    'integration_startup_recovery_snapshot',
    'integration_deposits',
    'refund_intents',
    'external_operations',
  ]) {
    if (snapshot.postgresAdapter.includes(forbidden)) {
      errors.push(`PostgreSQL Webhook persistence crossed domain boundary: ${forbidden}`)
    }
  }

  for (const token of [
    'webhook_event_identity_guard',
    'webhook event id reused with different payload',
    'CREATE TABLE webhook_processing_attempts',
    'CREATE TABLE webhook_replay_audit',
  ]) {
    const source = token.startsWith('CREATE TABLE webhook_') && token.includes('processing')
      ? snapshot.migrationRuntime
      : token === 'CREATE TABLE webhook_replay_audit'
        ? snapshot.migrationRuntime
        : snapshot.migrationFabric
    if (!source.includes(token)) {
      errors.push(`Webhook migration authority missing: ${token}`)
    }
  }

  for (const token of [
    'live_pg18_webhook_persistence_preserves_identity_claim_retry_dead_letter_and_replay',
    'PostgresWebhookPersistence::new',
    'register_webhook_endpoint(',
    'promoted_duplicate',
    'WebhookUnverifiable',
    'expected_hash',
    '.claim_next_webhook("tenant-b")?',
    'attempt_number, 1',
    'application_retry',
    'attempt_number, 2',
    'complete_webhook',
    '"dead_letter"',
    'list_webhook_dead_letters',
    'replay_webhook',
    'replay_audit_count, 1',
    'set_webhook_endpoint_enabled',
    'BindingUnavailable',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Webhook PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'FixtureWebhookIngress::from_postgres_with_metrics(',
    'WebhookRuntime::from_postgres_with_metrics(',
  ]) {
    if (!snapshot.main.includes(token) && !snapshot.webhookRuntime.includes(token)) {
      errors.push(`Webhook PostgreSQL composition seam missing: ${token}`)
    }
  }
  if (!snapshot.governedWorker.includes('WebhookRuntime::from_postgres_with_metrics(')) {
    errors.push('Governed Integration worker does not select PostgreSQL webhook persistence')
  }
  if (!snapshot.main.includes(
    'FixtureWebhookIngress::from_postgres_with_metrics(',
  )) {
    errors.push('Production composition root does not select PostgreSQL webhook ingress persistence')
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Webhook production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Webhook cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8WebhookPersistenceSnapshot(
    collectP8WebhookPersistenceSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Webhook persistence cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Webhook persistence PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
