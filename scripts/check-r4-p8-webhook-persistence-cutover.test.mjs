#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8WebhookPersistenceSnapshot,
  validateP8WebhookPersistenceSnapshot,
} from './check-r4-p8-webhook-persistence-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8WebhookPersistenceSnapshot())
  mutate(snapshot)
  const errors = validateP8WebhookPersistenceSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8WebhookPersistenceSnapshot(
  collectP8WebhookPersistenceSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Webhook persistence mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Webhook Runtime persistence port becomes public',
  snapshot => {
    snapshot.contract = snapshot.contract.replaceAll(
      'pub(crate) trait WebhookRuntimePersistence: WebhookReplayPersistence',
      'pub trait WebhookRuntimePersistence: WebhookReplayPersistence',
    )
  },
  'WebhookRuntimePersistence',
)

expectFailure(
  'Webhook Runtime regains concrete IntegrationStore ownership',
  snapshot => {
    snapshot.webhookRuntime = snapshot.webhookRuntime.replaceAll(
      'persistence: Arc<dyn WebhookRuntimePersistence>',
      'store: IntegrationStore',
    )
  },
  'must not own a concrete IntegrationStore field',
)

expectFailure(
  'Webhook Runtime implementation bypasses persistence port',
  snapshot => {
    snapshot.webhookRuntime = snapshot.webhookRuntime.replaceAll(
      'self.persistence',
      'self.store',
    )
  },
  'must not bypass persistence port',
)

expectFailure(
  'Route-facing Webhook ingress loses PostgreSQL constructor seam',
  snapshot => {
    snapshot.webhookRuntime = snapshot.webhookRuntime.replaceAll(
      'runtime: WebhookRuntime::from_postgres_with_metrics(pool, metrics)',
      'runtime: WebhookRuntime::new_REMOVED',
    )
  },
  'runtime: WebhookRuntime::from_postgres_with_metrics(pool, metrics)',
)

expectFailure(
  'Webhook Runtime loses port-only injection proof',
  snapshot => {
    snapshot.webhookRuntime = snapshot.webhookRuntime.replaceAll(
      'runtime_accepts_webhook_persistence_port_without_integration_store',
      'removed_webhook_port_proof',
    )
  },
  'runtime_accepts_webhook_persistence_port_without_integration_store',
)

expectFailure(
  'IntegrationModule Webhook admin bypasses persistence port',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'webhook_admin: Arc<dyn WebhookAdminPersistence>',
      'removed_webhook_admin_port: IntegrationStore',
    )
  },
  'webhook_admin',
)

expectFailure(
  'PostgreSQL Webhook persistence loses serializable authority',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL Webhook claim loses SKIP LOCKED fencing',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'FOR UPDATE OF i,e,b,p SKIP LOCKED',
      'FOR UPDATE OF i,e,b,p',
    )
  },
  'SKIP LOCKED',
)

expectFailure(
  'PostgreSQL Webhook claim loses tenant predicate',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'WHERE i.tenant_id=$1',
      'WHERE TRUE',
    )
  },
  'WHERE i.tenant_id=$1',
)

expectFailure(
  'PostgreSQL Webhook persistence gains SQLite coupling',
  snapshot => {
    snapshot.postgresAdapter += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite',
)

expectFailure(
  'PostgreSQL Webhook persistence crosses into Operation Runtime',
  snapshot => {
    snapshot.postgresAdapter += '\n// external_operations\n'
  },
  'crossed domain boundary',
)

expectFailure(
  'Webhook identity migration guard disappears',
  snapshot => {
    snapshot.migrationFabric = snapshot.migrationFabric.replaceAll(
      'webhook_event_identity_guard',
      'removed_webhook_identity_guard',
    )
  },
  'webhook_event_identity_guard',
)

expectFailure(
  'Live proof stops checking rejected-to-verified promotion',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'promoted_duplicate',
      'promotion_flag_removed',
    )
  },
  'promoted_duplicate',
)

expectFailure(
  'Live proof stops checking payload conflict rejection',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'IntegrationError::WebhookUnverifiable',
      'IntegrationError::Persistence',
    )
  },
  'WebhookUnverifiable',
)

expectFailure(
  'Live proof stops checking cross-tenant claim isolation',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      '.claim_next_webhook("tenant-b")?',
      '.claim_next_webhook("tenant-a")?',
    )
  },
  'tenant-b',
)

expectFailure(
  'Live proof stops checking replay audit durability',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'replay_audit_count, 1',
      'replay_audit_count, 0',
    )
  },
  'replay_audit_count, 1',
)

expectFailure(
  'Live proof stops checking endpoint disable fail-closed',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'set_webhook_endpoint_enabled',
      'removed_endpoint_enable_toggle',
    )
  },
  'set_webhook_endpoint_enabled',
)

expectFailure(
  'Governed worker stops selecting PostgreSQL webhook persistence',
  snapshot => {
    snapshot.governedWorker = snapshot.governedWorker.replaceAll(
      'WebhookRuntime::from_postgres_with_metrics(',
      'WebhookRuntime::new_with_metrics(',
    )
  },
  'does not select PostgreSQL webhook persistence',
)

expectFailure(
  'Production root stops selecting PostgreSQL webhook ingress persistence',
  snapshot => {
    snapshot.main = snapshot.main.replaceAll(
      'FixtureWebhookIngress::from_postgres_with_metrics(',
      'FixtureWebhookIngress::new_with_metrics(',
    )
  },
  'does not select PostgreSQL webhook ingress persistence',
)

expectFailure(
  'Webhook production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Webhook gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Webhook persistence cutover mutation tests passed.')
