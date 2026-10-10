#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const PATHS = Object.freeze({
  contract: '.talos/ci/r4-p10-sp06-full-recovery-closure.json',
  runner: 'scripts/r4-p10-sp06-full-recovery-closure.sh',
  compatibility: 'scripts/r4-p10-schema-compatibility.mjs',
  probe: 'backend/src/integration/p10_recovery_qualification.rs',
  milestone: '.github/workflows/milestone-qualification.yml',
  exactHead: '.github/workflows/exact-head-qualification.yml',
  registry: '.talos/ci/package-qualifications.json',
})

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP10Sp06Snapshot() {
  return {
    contract: JSON.parse(read(PATHS.contract)),
    runner: read(PATHS.runner),
    compatibility: read(PATHS.compatibility),
    probe: read(PATHS.probe),
    milestone: read(PATHS.milestone),
    exactHead: read(PATHS.exactHead),
    registry: read(PATHS.registry),
  }
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) errors.push(label + ' missing: ' + token)
  }
}

function shellSyntax(errors, source) {
  const result = spawnSync('bash', ['-n'], { input: source, encoding: 'utf8' })
  if (result.error) errors.push('SP06 shell syntax could not execute: ' + result.error.message)
  else if (result.status !== 0) errors.push('SP06 shell syntax invalid: ' + String(result.stderr || '').trim().replaceAll('\n', ' '))
}

export function validateP10Sp06Snapshot(snapshot) {
  const errors = []
  const contract = snapshot.contract || {}
  shellSyntax(errors, snapshot.runner)

  if (contract.schema !== 'talos.p10.full-recovery-closure-contract/v2') errors.push('SP06 machine contract schema mismatch')
  if (contract.scope !== 'R4-P10-SP06') errors.push('SP06 machine contract scope mismatch')
  if (contract.exitGate !== 'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS') errors.push('SP06 final exit gate mismatch')
  if (contract.singleChain !== true) errors.push('SP06 must remain one measured recovery chain')
  if (JSON.stringify(contract.dependsOn) !== JSON.stringify(['P10_ROLLBACK_FORWARD_FIX_PASS'])) {
    errors.push('SP06 dependency gate mismatch')
  }

  const identity = contract.authoritativeIdentity || {}
  if (
    identity.sourceShaEnv !== 'TALOS_QUALIFIED_SOURCE_SHA' ||
    identity.sourceTreeEnv !== 'TALOS_QUALIFIED_SOURCE_TREE_SHA' ||
    identity.migrationManifest !== 'per-file-sha256' ||
    identity.transportShaIsNotSourceAuthority !== true
  ) errors.push('SP06 authoritative source identity contract weakened')

  const measured = contract.measuredRecovery || {}
  for (const key of ['backupDurationMs','restoreDurationMs','runtimeRecoveryMs','fullRehearsalMs','controlledDataLossWindowMs']) {
    if (measured[key] !== true) errors.push('SP06 recovery measurement missing: ' + key)
  }
  if (measured.controlledMissingMarkers !== 1 || measured.interpretation !== 'measured_rehearsal_not_production_slo') {
    errors.push('SP06 RTO/RPO interpretation contract weakened')
  }

  const rollback = contract.rollback || {}
  if (
    rollback.sourceCommit !== '2f414df89769ceedce482c4c25a24f02a24344b5' ||
    rollback.mode !== 'runtime-build-projection' ||
    rollback.privateGitObjectRequired !== false ||
    rollback.maxMigration !== '083_r4_reservation_rule_sequence_invariant' ||
    rollback.compatibleRequired !== true ||
    rollback.incompatiblePreviousMigrationBlocked !== true ||
    rollback.destructiveDowngradeForbidden !== true
  ) errors.push('SP06 rollback authority contract weakened')

  const effect = contract.effectSafety || {}
  for (const key of ['drainBeforeCut','dispatchCrashCut','canonicalBindingFreeze','unknownOutcomeAfterRestart','blindRetryForbidden','reconciliationRequired','attemptCountStable']) {
    if (effect[key] !== true) errors.push('SP06 effect-safety contract weakened: ' + key)
  }
  if (effect.compensation !== 'not_applicable_nonfinancial_fixture') errors.push('SP06 compensation boundary changed')

  const milestone = contract.milestone || {}
  if (
    milestone.qualificationSet !== 'p10' ||
    milestone.sameFrozenSha !== true ||
    milestone.requiresP9HistoricalDeploymentEvidence !== true ||
    milestone.finalGate !== 'P10_FROZEN_RECOVERY_EVIDENCE=PASS'
  ) errors.push('SP06 milestone closure contract weakened')

  requireTokens(errors, snapshot.runner, 'SP06 full recovery runner', [
    ': "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"',
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'source-migration-manifest.txt',
    'sha256sum "$file"',
    'P9_ROLLBACK_SOURCE_SHA=2f414df89769ceedce482c4c25a24f02a24344b5',
    'P9_ROLLBACK_MAX_MIGRATION=083_r4_reservation_rule_sequence_invariant',
    'P9_BUILD_CONTEXT="$RUNTIME_DIR/p9-build-projection"',
    'P9_DB_MOD_FIXTURE=',
    'P9_INTEGRATION_MOD_FIXTURE=',
    'docker build -t "$P9_IMAGE" "$P9_BUILD_CONTEXT"',
    'bash scripts/r4-p10-pg-backup.sh',
    'bash scripts/r4-p10-verify-backup.sh',
    'bash scripts/r4-p10-pg-restore.sh',
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_MIGRATION_MANIFEST_SHA256="$source_migration_manifest_sha256"',
    'P10_MIGRATION_HEAD="$source_migration_head"',
    'P10_EXPECTED_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_EXPECTED_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_EXPECTED_MIGRATION_MANIFEST_SHA256="$source_migration_manifest_sha256"',
    'P10_EXPECTED_MIGRATION_HEAD="$source_migration_head"',
    'docker exec -i "$RESTORE_CONTAINER" psql -U talos -d talos -Atc "SELECT 1"',
    'P10_SP06_RESTORE_DATABASE_READY database=talos probe=$RESTORE_DATABASE_READY',
    'test "$RESTORE_DATABASE_READY" = "1"',
    'TALOS_P10_EXPECTED_MIGRATION_ID="$source_migration_registry_head"',
    'unset TALOS_P10_RESTORE_DATABASE_URL TALOS_P10_EXPECTED_MIGRATION_ID',
    'RECOVERY_START_NS=',
    'RUNTIME_RECOVERY_MS=',
    'CONTROLLED_DATA_LOSS_WINDOW_MS=',
    'controlled_missing_markers=1',
    'DRAIN_DISPATCHING=',
    "SET state='dispatching',attempt_count=1",
    'p10_sp06_controlled_crash_cut',
    'binding-disabled.json',
    'unknown_outcome|1|worker_restarted_after_dispatch',
    'ROLLBACK_SCHEMA_INCOMPATIBLE',
    'compose[@]}" up -d --no-deps --force-recreate app nginx',
    'TALOS_P10_RECOVERY_EVIDENCE_REF="p10-sp06-provider-query-confirmed-effect"',
    'TALOS_P10_RECOVERY_ACTOR_REF="p10-sp06-recovery-operator"',
    'P10_EFFECT_RECONCILIATION',
    'effect_confirmed|p10-sp06-recovery-operator',
    'FROM api_clients WHERE tenant_id=',
    'FULL_REHEARSAL_MS=',
    'rto_rpo_scope=measured_rehearsal_not_production_slo',
    'scan_retained_evidence()',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ])

  for (const forbidden of [
    'git worktree add',
    'git cat-file -e "$P9_ROLLBACK',
    'P9_WORKTREE=',
    'DELETE FROM schema_migrations',
    'ALTER TABLE DROP',
    'bash scripts/r4-p10-sp01-backup-artifact-contract.sh',
    'bash scripts/r4-p10-sp02-protected-backup-rehearsal.sh',
    'bash scripts/r4-p10-sp03-isolated-restore.sh',
    'bash scripts/r4-p10-sp04-restored-runtime.sh',
    'bash scripts/r4-p10-sp05-rollback-forward-fix.sh',
  ]) {
    if (snapshot.runner.includes(forbidden)) errors.push('SP06 crosses clean-history or single-chain boundary: ' + forbidden)
  }

  const backup = snapshot.runner.indexOf('bash scripts/r4-p10-pg-backup.sh')
  const recoveryStart = snapshot.runner.indexOf('RECOVERY_START_NS=')
  const restore = snapshot.runner.indexOf('bash scripts/r4-p10-pg-restore.sh')
  const runtimeReady = snapshot.runner.indexOf('RUNTIME_RECOVERY_MS=')
  const drain = snapshot.runner.indexOf('DRAIN_DISPATCHING=')
  const crash = snapshot.runner.indexOf("SET state='dispatching',attempt_count=1")
  const disable = snapshot.runner.indexOf('binding-disabled.json')
  const rollbackRun = snapshot.runner.indexOf('--network-alias app')
  const unknown = snapshot.runner.indexOf('unknown_outcome|1|worker_restarted_after_dispatch')
  const incompatible = snapshot.runner.indexOf('ROLLBACK_SCHEMA_INCOMPATIBLE')
  const forwardFix = snapshot.runner.indexOf('forward-fix-ready.json')
  const reconcile = snapshot.runner.indexOf('p10_unknown_outcome_requires_reconciliation_before_retry')
  const full = snapshot.runner.indexOf('FULL_REHEARSAL_MS=')
  if (!(backup >= 0 && backup < recoveryStart && recoveryStart < restore && restore < runtimeReady && runtimeReady < drain && drain < crash && crash < disable && disable < rollbackRun && rollbackRun < unknown && unknown < incompatible && incompatible < forwardFix && forwardFix < reconcile && reconcile < full)) {
    errors.push('SP06 order must be backup -> recovery start -> restore -> runtime -> drain -> crash -> freeze -> rollback -> UnknownOutcome -> incompatible block -> forward-fix -> reconciliation -> measured closure')
  }

  requireTokens(errors, snapshot.probe, 'SP06 reconciliation probe', [
    'TALOS_P10_RECOVERY_EVIDENCE_REF',
    'TALOS_P10_RECOVERY_ACTOR_REF',
    'p10-sp05-provider-query-confirmed-effect',
    'p10-sp05-recovery-operator',
    'begin_reconciliation',
    'resolve_reconciliation',
    'blind_retry=false',
  ])

  requireTokens(errors, snapshot.milestone, 'P10 same-SHA milestone', [
    'options:\n          - p9\n          - p10',
    'p10_full_recovery:',
    "if: inputs.qualification_set == 'p10'",
    'TALOS_QUALIFIED_SOURCE_SHA=$actual',
    'TALOS_QUALIFIED_SOURCE_TREE_SHA=$tree',
    'bash scripts/r4-p10-sp06-full-recovery-closure.sh',
    'p10_evidence_gate:',
    '- p9_evidence_gate',
    '- p10_full_recovery',
    'P10_FROZEN_RECOVERY_EVIDENCE=PASS',
  ])

  if (snapshot.exactHead.includes('bash scripts/r4-p10-sp06-full-recovery-closure.sh')) {
    errors.push('SP06 full recovery must not become a permanent Exact-Head job')
  }

  requireTokens(errors, snapshot.registry, 'P10-SP06 package registration', [
    '"branch_prefix": "agent/r4-p10-sp06-"',
    '"scripts/check-r4-p10-sp06-full-recovery-closure.test.mjs"',
    '"scripts/check-r4-p10-sp06-full-recovery-closure.mjs"',
    '"qualification_script": "scripts/r4-p10-sp06-full-recovery-closure.sh"',
    '"label": "p10-sp06-full-recovery-closure"',
  ])

  return errors
}

function main() {
  const errors = validateP10Sp06Snapshot(collectP10Sp06Snapshot())
  if (errors.length > 0) {
    console.error('R4-P10-SP06 full recovery closure gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P10-SP06 full recovery closure gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
