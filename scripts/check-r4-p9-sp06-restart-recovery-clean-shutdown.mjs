#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  main: 'backend/src/main.rs',
  productionCompose: 'deploy/compose.production.yml',
  qualificationCompose: 'deploy/compose.p9-sp06.yml',
  runner: 'scripts/r4-p9-sp06-restart-recovery-clean-shutdown.sh',
  workflow: '.github/workflows/exact-head-qualification.yml',
  milestoneWorkflow: '.github/workflows/milestone-qualification.yml',
  evidence:
    'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p9-sp06-restart-recovery-clean-shutdown.md',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP9Sp06Snapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) {
      errors.push(label + ' missing: ' + token)
    }
  }
}

export function validateP9Sp06Snapshot(snapshot) {
  const errors = []

  requireTokens(errors, snapshot.main, 'Production graceful shutdown', [
    'let maintenance_worker = worker_runner.spawn_periodic();',
    'let integration_worker = integration_worker.spawn_periodic();',
    '.with_graceful_shutdown(shutdown_signal())',
    'async fn shutdown_signal()',
    'tokio::signal::ctrl_c()',
    'tokio::signal::unix::SignalKind::terminate()',
    'tokio::select!',
    'maintenance_worker.abort();',
    'integration_worker.abort();',
    'background workers stopped',
    'services::audit_service::shutdown_audit_buffer();',
    'graceful shutdown complete',
  ])
  if (snapshot.main.includes('with_graceful_shutdown(async move')) {
    errors.push('Production shutdown must use the shared SIGINT/SIGTERM signal boundary')
  }

  requireTokens(errors, snapshot.productionCompose, 'Production PostgreSQL authority', [
    'DB_BACKEND: postgres',
    'DATABASE_URL: ${TALOS_PRODUCTION_DATABASE_URL:?TALOS_PRODUCTION_DATABASE_URL is required}',
    'image: postgres:18.6-alpine3.24',
  ])
  if (/sqlite/i.test(snapshot.productionCompose)) {
    errors.push('Qualified production Compose must not introduce SQLite state')
  }

  requireTokens(errors, snapshot.qualificationCompose, 'SP06 qualification bootstrap', [
    'AUTH_BOOTSTRAP_ON_START: "true"',
    'AUTH_BOOTSTRAP_ADMIN_USERNAME: ${P9_SP06_BOOTSTRAP_USERNAME:?P9_SP06_BOOTSTRAP_USERNAME is required}',
    'AUTH_BOOTSTRAP_ADMIN_PASSWORD: ${P9_SP06_BOOTSTRAP_PASSWORD:?P9_SP06_BOOTSTRAP_PASSWORD is required}',
  ])

  requireTokens(errors, snapshot.runner, 'SP06 live restart/recovery runner', [
    '-f deploy/compose.production.yml',
    '-f deploy/compose.p9-sp06.yml',
    '/api/integrations/manifests',
    '/api/integrations/instances',
    '/api/integrations/bindings',
    '/api/integrations/operations',
    'INSERT INTO integration_circuit_state',
    "'p9-sp06-binding','open',1,'9999-12-31T23:59:59Z'",
    'test "$queued_state" = "ready|0"',
    'SELECT system_identifier FROM pg_control_system();',
    '"${compose[@]}" stop -t 20 app',
    "state='dispatching'",
    'external_operation_attempts',
    "'claimed','p9_sp06_restart_cutpoint'",
    '"${compose[@]}" rm -f app',
    '"${compose[@]}" up -d --no-build app',
    'test "$APP_CONTAINER_AFTER" != "$APP_CONTAINER_BEFORE"',
    'test "$DB_CONTAINER_AFTER" = "$DB_CONTAINER_BEFORE"',
    'test "$PG_SYSTEM_ID_AFTER" = "$PG_SYSTEM_ID_BEFORE"',
    'unknown_outcome|worker_restarted_after_dispatch',
    "event_type='recovered_after_restart'",
    "classification='worker_restarted_after_dispatch'",
    'Database profile: Postgres18',
    '! grep -F \'SQLite local database path\'',
    'test "$app_db_backend" = "postgres"',
    "find /app -maxdepth 4 -type f",
    'test -z "$sqlite_files"',
    'test "$FIRST_EXIT" = "0"',
    'test "$FIRST_OOM" = "false"',
    'test "$FINAL_EXIT" = "0"',
    'test "$FINAL_OOM" = "false"',
    "docker inspect -f 'id={{.Id}} status={{.State.Status}} exit_code={{.State.ExitCode}} oom_killed={{.State.OOMKilled}}'",
    'ready-before.json',
    'ready-after.json',
    'db_container_before=$DB_CONTAINER_BEFORE',
    'postgres_system_identifier_before=$PG_SYSTEM_ID_BEFORE',
    'application_container_before=$APP_CONTAINER_BEFORE',
    'graceful shutdown complete',
    'clean_environment_exit=CLEAN_ENV_DEPLOYMENT_PASS',
    'CLEAN_ENV_DEPLOYMENT_PASS',
  ])

  for (const forbidden of [
    'docker kill',
    'kill -s INT',
    'kill -SIGINT',
    'stop_signal: SIGINT',
    'app-before-restart.inspect.json',
    'app-final-shutdown.inspect.json',
  ]) {
    if (snapshot.runner.includes(forbidden) || snapshot.qualificationCompose.includes(forbidden)) {
      errors.push('SP06 must prove ordinary Compose SIGTERM shutdown, forbidden: ' + forbidden)
    }
  }

  const circuitGuard = snapshot.runner.indexOf('INSERT INTO integration_circuit_state')

  if (/docker inspect\s+"\$APP_CONTAINER_(?:BEFORE|AFTER)"\s*>/.test(snapshot.runner)) {
    errors.push(
      'SP06 must never retain full docker inspect output because container env contains qualification secrets',
    )
  }

  const admission = snapshot.runner.indexOf('/api/integrations/operations')
  const crashCut = snapshot.runner.indexOf("SET state='dispatching'")
  const removeApp = snapshot.runner.indexOf('"${compose[@]}" rm -f app')
  const restartApp = snapshot.runner.indexOf('"${compose[@]}" up -d --no-build app')
  const recovery = snapshot.runner.lastIndexOf("event_type='recovered_after_restart'")
  if (
    circuitGuard < 0 ||
    admission < 0 ||
    crashCut < 0 ||
    removeApp < 0 ||
    restartApp < 0 ||
    recovery < 0 ||
    !(
      circuitGuard < admission &&
      admission < crashCut &&
      crashCut < removeApp &&
      removeApp < restartApp &&
      restartApp < recovery
    )
  ) {
    errors.push(
      'SP06 runner order must be circuit guard -> governed admission -> crash cut-point -> app replacement -> startup recovery proof',
    )
  }

  requireTokens(errors, snapshot.milestoneWorkflow, 'Milestone SP06 job', [
    'p9_restart_recovery_shutdown:',
    'name: P9-SP06 restart recovery and clean-environment qualification',
    'needs: pin',
    'bash scripts/r4-p9-sp06-restart-recovery-clean-shutdown.sh',
    'p9-sp06-restart-recovery-${{ github.run_id }}-${{ github.run_attempt }}',
    '.talos-evidence/p9-sp06',
  ])

  const testCall = 'node scripts/check-r4-p9-sp06-restart-recovery-clean-shutdown.test.mjs'
  const gateCall = 'node scripts/check-r4-p9-sp06-restart-recovery-clean-shutdown.mjs'
  const testCount = snapshot.workflow.split(testCall).length - 1
  const gateCount = snapshot.workflow.split(gateCall).length - 1
  if (testCount !== 1 || gateCount !== 1) {
    errors.push(
      'Exact-Head must run SP06 mutation and structural gates once; found test=' +
        testCount +
        ', gate=' +
        gateCount,
    )
  }

  requireTokens(errors, snapshot.evidence, 'SP06 evidence contract', [
    'CLEAN_ENV_DEPLOYMENT_PASS',
    'SIGINT + SIGTERM',
    'same authoritative PostgreSQL 18 deployment state',
    'worker_restarted_after_dispatch',
    'recovered_after_restart',
    'no SQLite production state',
    'does not claim:',
    'PostgreSQL backup/restore/rollback rehearsal',
  ])

  return errors
}

function main() {
  const errors = validateP9Sp06Snapshot(collectP9Sp06Snapshot())
  if (errors.length > 0) {
    console.error('R4-P9-SP06 restart/recovery/shutdown gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P9-SP06 restart/recovery/shutdown gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
