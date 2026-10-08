#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8SchedulerRecoverySnapshot,
  validateP8SchedulerRecoverySnapshot,
} from './check-r4-p8-scheduler-recovery-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8SchedulerRecoverySnapshot())
  mutate(snapshot)
  const errors = validateP8SchedulerRecoverySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8SchedulerRecoverySnapshot(
  collectP8SchedulerRecoverySnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Scheduler/Recovery mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Scheduler loses backend-neutral persistence port',
  snapshot => {
    snapshot.contract = snapshot.contract.replaceAll(
      'pub(crate) trait IntegrationSchedulerPersistence: Send + Sync',
      'pub(crate) trait RemovedSchedulerPersistence: Send + Sync',
    )
  },
  'IntegrationSchedulerPersistence',
)

expectFailure(
  'Worker regains concrete IntegrationStore scheduler authority',
  snapshot => {
    snapshot.worker = snapshot.worker.replaceAll(
      'scheduler: Arc<dyn IntegrationSchedulerPersistence>',
      'store: IntegrationStore',
    )
  },
  'must not own concrete IntegrationStore scheduler authority',
)

expectFailure(
  'PostgreSQL worker composition loses Operation Runtime',
  snapshot => {
    snapshot.worker = snapshot.worker.replaceAll(
      'OperationRuntime::from_postgres',
      'removed_operation_runtime_pg_constructor',
    )
  },
  'OperationRuntime::from_postgres',
)

expectFailure(
  'PostgreSQL worker composition loses Webhook Runtime',
  snapshot => {
    snapshot.worker = snapshot.worker.replaceAll(
      'WebhookRuntime::from_postgres',
      'removed_webhook_runtime_pg_constructor',
    )
  },
  'WebhookRuntime::from_postgres',
)

expectFailure(
  'Scheduler persistence loses SERIALIZABLE isolation',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'Startup snapshot stops capturing dispatching operations',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "FROM external_operations\n                 WHERE state='dispatching'",
      "FROM external_operations\n                 WHERE state='ready'",
    )
  },
  "WHERE state='dispatching'",
)

expectFailure(
  'Startup snapshot stops capturing processing webhooks',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "FROM webhook_inbox\n                 WHERE status='processing'",
      "FROM webhook_inbox\n                 WHERE status='verified'",
    )
  },
  "WHERE status='processing'",
)

expectFailure(
  'Snapshot consumption loses row fencing',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'FOR UPDATE',
      'NO_ROW_LOCK',
    )
  },
  'FOR UPDATE',
)

expectFailure(
  'Operation recovery stops projecting unknown outcome',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "state='unknown_outcome'",
      "state='ready'",
    )
  },
  "state='unknown_outcome'",
)

expectFailure(
  'Webhook recovery stops returning work to verified queue',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "status='verified'",
      "status='processed'",
    )
  },
  "status='verified'",
)

expectFailure(
  'Webhook recovery stops marking attempts retryable',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "state='retryable_failure'",
      "state='processed'",
    )
  },
  "state='retryable_failure'",
)

expectFailure(
  'Scheduler loses durable keyset cursor advance',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'WHERE tenant_id>$1',
      'WHERE tenant_id>=$1',
    )
  },
  'WHERE tenant_id>$1',
)

expectFailure(
  'Cursor migration allows another scheduler authority',
  snapshot => {
    snapshot.cursorMigration = snapshot.cursorMigration.replaceAll(
      "scheduler_id TEXT PRIMARY KEY CHECK (scheduler_id = 'fixture_integration_worker')",
      'scheduler_id TEXT PRIMARY KEY',
    )
  },
  'fixture_integration_worker',
)

expectFailure(
  'Recovery snapshot migration accepts arbitrary work kinds',
  snapshot => {
    snapshot.snapshotMigration = snapshot.snapshotMigration.replaceAll(
      "work_kind TEXT NOT NULL CHECK (work_kind IN ('external_operation', 'webhook'))",
      'work_kind TEXT NOT NULL',
    )
  },
  'work_kind',
)

expectFailure(
  'Live proof stops distinguishing post-snapshot operation',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'operation-after',
      'post_snapshot_operation_removed',
    )
  },
  'operation-after',
)

expectFailure(
  'Live proof stops distinguishing post-snapshot webhook',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'webhook-after',
      'post_snapshot_webhook_removed',
    )
  },
  'webhook-after',
)

expectFailure(
  'Live proof stops checking cursor keyset page',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'vec!["tenant-b".to_owned(), "tenant-c".to_owned()]',
      'vec!["tenant-a".to_owned()]',
    )
  },
  'tenant-b',
)

expectFailure(
  'Governed worker stops selecting PostgreSQL scheduler persistence',
  snapshot => {
    snapshot.governedWorker = snapshot.governedWorker.replaceAll(
      'PostgresIntegrationSchedulerPersistence::new(pool)',
      'removed_postgres_scheduler_persistence(pool)',
    )
  },
  'PostgresIntegrationSchedulerPersistence::new(pool)',
)

expectFailure(
  'Production root stops selecting PostgreSQL governed scheduler runtime',
  snapshot => {
    snapshot.main = snapshot.main.replaceAll(
      'GovernedIntegrationWorker::from_postgres_with_metrics(',
      'GovernedIntegrationWorker::new_with_metrics(',
    )
  },
  'does not select PostgreSQL integration scheduler',
)

expectFailure(
  'Scheduler Recovery production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Scheduler Recovery gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Scheduler/Recovery cutover mutation tests passed.')
