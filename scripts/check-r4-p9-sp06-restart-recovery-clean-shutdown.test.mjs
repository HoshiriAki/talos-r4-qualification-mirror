#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP9Sp06Snapshot,
  validateP9Sp06Snapshot,
} from './check-r4-p9-sp06-restart-recovery-clean-shutdown.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP9Sp06Snapshot())
  mutate(snapshot)
  const errors = validateP9Sp06Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP9Sp06Snapshot(collectP9Sp06Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'production executable loses SIGTERM handling',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'tokio::signal::unix::SignalKind::terminate()',
      'tokio::signal::unix::SignalKind::interrupt()',
    )
  },
  'SignalKind::terminate()',
)

expectFailure(
  'periodic worker handle is discarded again',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'let integration_worker = integration_worker.spawn_periodic();',
      'let _integration_worker = integration_worker.spawn_periodic();',
    )
  },
  'let integration_worker = integration_worker.spawn_periodic();',
)

expectFailure(
  'qualification profile changes stop signal to SIGINT',
  snapshot => {
    snapshot.qualificationCompose += '\n    stop_signal: SIGINT\n'
  },
  'ordinary Compose SIGTERM shutdown',
)

expectFailure(
  'runner removes the deterministic pre-dispatch circuit guard',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'INSERT INTO integration_circuit_state',
      'INSERT INTO removed_circuit_guard',
    )
  },
  'INSERT INTO integration_circuit_state',
)

expectFailure(
  'runner allows an attempt before the controlled crash cut-point',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$queued_state" = "ready|0"',
      'test "$queued_state" = "ready|1"',
    )
  },
  'test "$queued_state" = "ready|0"',
)

expectFailure(
  'runner stops using the governed Integration API for durable admission',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '/api/integrations/operations',
      '/api/qualification/direct-operation',
    )
  },
  '/api/integrations/operations',
)

expectFailure(
  'runner recreates database instead of preserving PostgreSQL authority',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$DB_CONTAINER_AFTER" = "$DB_CONTAINER_BEFORE"',
      'test "$DB_CONTAINER_AFTER" != "$DB_CONTAINER_BEFORE"',
    )
  },
  'test "$DB_CONTAINER_AFTER" = "$DB_CONTAINER_BEFORE"',
)

expectFailure(
  'runner stops checking PostgreSQL system identity',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$PG_SYSTEM_ID_AFTER" = "$PG_SYSTEM_ID_BEFORE"',
      'true',
    )
  },
  'test "$PG_SYSTEM_ID_AFTER" = "$PG_SYSTEM_ID_BEFORE"',
)

expectFailure(
  'runner no longer replaces application process container',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '"${compose[@]}" rm -f app',
      'echo keep-old-app-container',
    )
  },
  '"${compose[@]}" rm -f app',
)

expectFailure(
  'runner weakens startup recovery classification',
  snapshot => {
    snapshot.runner = snapshot.runner.replaceAll(
      'worker_restarted_after_dispatch',
      'generic_restart',
    )
  },
  'worker_restarted_after_dispatch',
)

expectFailure(
  'runner stops requiring recovered runtime event',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "event_type='recovered_after_restart'",
      "event_type='claimed'",
    )
  },
  "event_type='recovered_after_restart'",
)

expectFailure(
  'runner permits SQLite state in production container',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test -z "$sqlite_files"',
      'true',
    )
  },
  'test -z "$sqlite_files"',
)

expectFailure(
  'runner retains full docker inspect output with qualification environment',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "docker inspect -f 'id={{.Id}} status={{.State.Status}} exit_code={{.State.ExitCode}} oom_killed={{.State.OOMKilled}}' \\\n  \"$APP_CONTAINER_BEFORE\" > \"$EVIDENCE_DIR/app-before-restart-state.txt\"",
      'docker inspect "$APP_CONTAINER_BEFORE" > "$EVIDENCE_DIR/app-before-restart.inspect.json"',
    )
  },
  'full docker inspect output',
)

expectFailure(
  'runner stops retaining exact PostgreSQL authority identity',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'postgres_system_identifier_before=$PG_SYSTEM_ID_BEFORE',
      'postgres_system_identifier_stable=true',
    )
  },
  'postgres_system_identifier_before=$PG_SYSTEM_ID_BEFORE',
)

expectFailure(
  'runner accepts SIGTERM-style non-graceful exit code',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$FINAL_EXIT" = "0"',
      'test "$FINAL_EXIT" = "143"',
    )
  },
  'test "$FINAL_EXIT" = "0"',
)

expectFailure(
  'SP06 final clean-environment exit disappears',
  snapshot => {
    snapshot.runner = snapshot.runner.replaceAll(
      'CLEAN_ENV_DEPLOYMENT_PASS',
      'P9_SP06_PARTIAL_PASS',
    )
  },
  'CLEAN_ENV_DEPLOYMENT_PASS',
)

expectFailure(
  'SP06 live job is removed from milestone qualification',
  snapshot => {
    snapshot.milestoneWorkflow = snapshot.milestoneWorkflow.replace(
      'p9_restart_recovery_shutdown:',
      'removed_sp06_job:',
    )
  },
  'p9_restart_recovery_shutdown:',
)

expectFailure(
  'SP06 structural gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'node scripts/check-r4-p9-sp06-restart-recovery-clean-shutdown.mjs',
      'node scripts/REMOVED-r4-p9-sp06-restart-recovery-clean-shutdown.mjs',
    )
  },
  'must run SP06 mutation and structural gates once',
)

console.log('R4-P9-SP06 restart/recovery/shutdown mutation tests passed.')
