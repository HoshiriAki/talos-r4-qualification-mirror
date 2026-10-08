#!/usr/bin/env node

import process from 'node:process'

import {
  loadStage2IntegrationSources,
  validateStage2IntegrationBoundary,
} from './check-stage2-integration-boundary.mjs'

const sources = await loadStage2IntegrationSources()
const errors = validateStage2IntegrationBoundary(sources)
if (errors.length > 0) {
  console.error(`expected baseline to pass: ${errors.join('; ')}`)
  process.exit(1)
}

function expectFailure(label, changed, expectedFragment) {
  const failures = validateStage2IntegrationBoundary(changed)
  if (!failures.some((error) => error.includes(expectedFragment))) {
    console.error(`expected ${label} to be rejected: ${failures.join('; ')}`)
    process.exit(1)
  }
}

expectFailure(
  'direct route SQL mutation',
  { ...sources, routes: `${sources.routes}\n// INSERT INTO provider_instances` },
  'integration HTTP route',
)

expectFailure(
  'UnknownOutcome removal',
  { ...sources, runtime: sources.runtime.replaceAll('UnknownOutcome', 'UnknownResult') },
  'external operation runtime',
)

expectFailure(
  'secret-safe projection removal',
  { ...sources, module: sources.module.replaceAll('configured_secret_names', 'unsafe_secret_projection') },
  'integration module',
)

expectFailure(
  'typed secret metadata removal',
  { ...sources, types: sources.types.replaceAll('SecretValueType', 'SecretKind') },
  'integration manifest types',
)

expectFailure(
  'webhook ingress rate limit removal',
  { ...sources, routes: sources.routes.replaceAll('fixture_webhook_rate_limit', 'unsafe_rate_limiter') },
  'webhook ingress rate limit',
)

expectFailure(
  'canonical effect intent removal',
  { ...sources, operation: sources.operation.replaceAll('EffectIntent', 'UntrustedRequestMarker') },
  'external effect intent',
)

expectFailure(
  'keystore capability grant removal',
  { ...sources, keystore: sources.keystore.replaceAll('allowed_capabilities', 'ambient_capabilities') },
  'keystore capability/purpose ACL',
)

expectFailure(
  'webhook retry budget removal',
  { ...sources, webhook: sources.webhook.replaceAll('MAX_WEBHOOK_ATTEMPTS', 'UNBOUNDED_WEBHOOK_ATTEMPTS') },
  'webhook runtime',
)

expectFailure(
  'sqlite provider revision guard removal',
  { ...sources, authorityMigration: sources.authorityMigration.replaceAll('provider_instance_revision_guard', 'removed_revision_guard') },
  'sqlite stage 2 authority hardening',
)

expectFailure(
  'postgres provider revision guard removal',
  { ...sources, pgAuthorityMigration: sources.pgAuthorityMigration.replaceAll('provider_instance_revision_guard', 'removed_revision_guard') },
  'postgres stage 2 authority hardening',
)

expectFailure(
  'sqlite binding identity guard removal',
  { ...sources, authorityMigration: sources.authorityMigration.replaceAll('provider_binding_identity_immutable', 'removed_binding_guard') },
  'sqlite stage 2 authority hardening',
)

expectFailure(
  'postgres binding identity guard removal',
  { ...sources, pgAuthorityMigration: sources.pgAuthorityMigration.replaceAll('provider_binding_identity_immutable', 'removed_binding_guard') },
  'postgres stage 2 authority hardening',
)

expectFailure(
  'sqlite refund atomic admission removal',
  { ...sources, authorityMigration: sources.authorityMigration.replaceAll('integration_refund_operation_atomic_link', 'removed_refund_atomic_link') },
  'sqlite stage 2 authority hardening',
)

expectFailure(
  'postgres refund atomic admission removal',
  { ...sources, pgAuthorityMigration: sources.pgAuthorityMigration.replaceAll('integration_refund_operation_atomic_link', 'removed_refund_atomic_link') },
  'postgres stage 2 authority hardening',
)

expectFailure(
  'sqlite webhook payload conflict guard removal',
  { ...sources, authorityMigration: sources.authorityMigration.replaceAll('webhook_event_payload_conflict', 'removed_webhook_identity_guard') },
  'sqlite stage 2 authority hardening',
)

expectFailure(
  'postgres webhook payload conflict guard removal',
  { ...sources, pgAuthorityMigration: sources.pgAuthorityMigration.replaceAll('integration_webhook_identity_guard', 'removed_webhook_identity_guard') },
  'postgres stage 2 authority hardening',
)

expectFailure(
  'sqlite provider invalidation removal',
  { ...sources, migration: sources.migration.replaceAll('integration_instance_invalidates_queued_operations', 'removed_instance_invalidation') },
  'sqlite stage 2 runtime migration',
)

expectFailure(
  'postgres provider invalidation removal',
  { ...sources, pgMigration: sources.pgMigration.replaceAll('integration_binding_invalidates_queued_operations', 'removed_binding_invalidation') },
  'postgres stage 2 runtime migration',
)

expectFailure(
  'sqlite half-open probe guard removal',
  { ...sources, migration: sources.migration.replaceAll('integration_half_open_single_probe', 'removed_half_open_guard') },
  'sqlite stage 2 runtime migration',
)

expectFailure(
  'postgres circuit scheduling projection removal',
  { ...sources, pgMigration: sources.pgMigration.replaceAll('integration_circuit_open', 'removed_circuit_projection') },
  'postgres stage 2 runtime migration',
)

expectFailure(
  'sqlite circuit scheduling projection removal',
  { ...sources, migration: sources.migration.replaceAll('integration_circuit_open', 'removed_circuit_projection') },
  'sqlite stage 2 runtime migration',
)

expectFailure(
  'postgres half-open probe guard removal',
  { ...sources, pgMigration: sources.pgMigration.replaceAll('integration_half_open_single_probe', 'removed_half_open_guard') },
  'postgres stage 2 runtime migration',
)

expectFailure(
  'postgres integration composition eagerly reconstructs SQLite compatibility store',
  {
    ...sources,
    main: sources.main.replaceAll(
      'pg_pool.is_none().then(||',
      'Some(',
    ),
  },
  'integration composition root',
)

expectFailure(
  'registry authority route dispatch removal',
  { ...sources, routes: sources.routes.replaceAll('.execute("integration"', '.execute("unsafe-integration"') },
  'integration HTTP route',
)

expectFailure(
  'persisted tenant scheduler cursor removal',
  { ...sources, store: sources.store.replaceAll('integration_scheduler_cursor', 'removed_scheduler_cursor') },
  'external operation persistence',
)

expectFailure(
  'startup recovery snapshot capture removal',
  { ...sources, worker: sources.worker.replaceAll('begin_startup_recovery_snapshot', 'removed_startup_snapshot') },
  'fixture worker',
)

expectFailure(
  'startup recovery page drain removal',
  { ...sources, store: sources.store.replaceAll('recover_next_startup_snapshot_page', 'removed_startup_page_drain') },
  'external operation persistence',
)

expectFailure(
  'sqlite scheduler migration removal',
  { ...sources, schedulerMigration: sources.schedulerMigration.replaceAll('integration_scheduler_cursor', 'removed_scheduler_cursor') },
  'sqlite integration scheduler migration',
)

expectFailure(
  'postgres scheduler migration removal',
  { ...sources, pgSchedulerMigration: sources.pgSchedulerMigration.replaceAll('integration_scheduler_cursor', 'removed_scheduler_cursor') },
  'postgres integration scheduler migration',
)

expectFailure(
  'sqlite startup recovery migration removal',
  { ...sources, recoveryMigration: sources.recoveryMigration.replaceAll('integration_startup_recovery_snapshot', 'removed_startup_recovery_snapshot') },
  'sqlite integration startup recovery migration',
)

expectFailure(
  'postgres startup recovery migration removal',
  { ...sources, pgRecoveryMigration: sources.pgRecoveryMigration.replaceAll('integration_startup_recovery_snapshot', 'removed_startup_recovery_snapshot') },
  'postgres integration startup recovery migration',
)

console.log('Stage 2 Integration Fabric boundary regression tests passed.')
