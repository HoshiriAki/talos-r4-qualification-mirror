#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const PATHS = Object.freeze({
  contract: '.talos/ci/r4-p10-sp05-rollback-forward-fix.json',
  runner: 'scripts/r4-p10-sp05-rollback-forward-fix.sh',
  compatibility: 'scripts/r4-p10-schema-compatibility.mjs',
  probe: 'backend/src/integration/p10_recovery_qualification.rs',
  integrationMod: 'backend/src/integration/mod.rs',
  runtimePg: 'backend/src/integration/operation_runtime_postgres.rs',
  operation: 'backend/src/integration/operation.rs',
  overlay: 'deploy/compose.p10-sp05.yml',
  registry: '.talos/ci/package-qualifications.json',
  workflow: '.github/workflows/exact-head-qualification.yml',
  dockerfile: 'Dockerfile',
  cargoToml: 'backend/Cargo.toml',
  cargoLock: 'backend/Cargo.lock',
  mainRs: 'backend/src/main.rs',
  p9DbMod: '.talos/ci/fixtures/p10-sp05/p9-db-mod.rs',
  p9IntegrationMod: '.talos/ci/fixtures/p10-sp05/p9-integration-mod.rs',
})

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP10Sp05Snapshot() {
  return {
    contract: JSON.parse(read(PATHS.contract)),
    runner: read(PATHS.runner),
    compatibility: read(PATHS.compatibility),
    probe: read(PATHS.probe),
    integrationMod: read(PATHS.integrationMod),
    runtimePg: read(PATHS.runtimePg),
    operation: read(PATHS.operation),
    overlay: read(PATHS.overlay),
    registry: read(PATHS.registry),
    workflow: read(PATHS.workflow),
    dockerfile: read(PATHS.dockerfile),
    cargoToml: read(PATHS.cargoToml),
    cargoLock: read(PATHS.cargoLock),
    mainRs: read(PATHS.mainRs),
    p9DbMod: read(PATHS.p9DbMod),
    p9IntegrationMod: read(PATHS.p9IntegrationMod),
  }
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) errors.push(label + ' missing: ' + token)
  }
}

function shellSyntax(errors, source) {
  const result = spawnSync('bash', ['-n'], { input: source, encoding: 'utf8' })
  if (result.error) {
    errors.push('SP05 shell syntax could not execute: ' + result.error.message)
  } else if (result.status !== 0) {
    errors.push('SP05 shell syntax invalid: ' + String(result.stderr || '').trim().replaceAll('\n', ' '))
  }
}

function gitBlobSha(content) {
  const bytes = Buffer.from(content, 'utf8')
  return createHash('sha1')
    .update(Buffer.from('blob ' + bytes.length + '\0', 'utf8'))
    .update(bytes)
    .digest('hex')
}

function exactArray(errors, actual, expected, label) {
  if (!Array.isArray(actual) || JSON.stringify(actual) !== JSON.stringify(expected)) {
    errors.push(label + ' does not match the frozen contract')
  }
}

export function validateP10Sp05Snapshot(snapshot) {
  const errors = []
  const contract = snapshot.contract ?? {}
  shellSyntax(errors, snapshot.runner)

  if (contract.schema !== 'talos.p10.rollback-forward-fix-contract/v2') errors.push('SP05 machine contract schema mismatch')
  if (contract.scope !== 'R4-P10-SP05') errors.push('SP05 machine contract scope mismatch')
  if (contract.exitGate !== 'P10_ROLLBACK_FORWARD_FIX_PASS') errors.push('SP05 machine contract exit gate mismatch')
  exactArray(errors, contract.dependsOn, ['P10_RESTORED_RUNTIME_PASS'], 'SP05 dependency gate')

  const identity = contract.authoritativeIdentity ?? {}
  if (
    identity.sourceShaEnv !== 'TALOS_QUALIFIED_SOURCE_SHA' ||
    identity.sourceTreeEnv !== 'TALOS_QUALIFIED_SOURCE_TREE_SHA' ||
    identity.repositoryMigrationManifest !== 'per-file-sha256' ||
    identity.transportShaIsNotSourceAuthority !== true
  ) errors.push('SP05 authoritative source identity contract weakened')

  const rollback = contract.rollbackCandidate ?? {}
  if (
    rollback.sourceCommit !== '2f414df89769ceedce482c4c25a24f02a24344b5' ||
    rollback.maxMigration !== '083_r4_reservation_rule_sequence_invariant' ||
    rollback.mode !== 'runtime-build-projection' ||
    rollback.privateGitObjectRequired !== false
  ) errors.push('SP05 rollback candidate authority contract weakened')

  const expectedBlobs = rollback.immutableBuildInputs ?? {}
  const actualBlobs = {
    'Dockerfile': gitBlobSha(snapshot.dockerfile),
    'backend/Cargo.toml': gitBlobSha(snapshot.cargoToml),
    'backend/Cargo.lock': gitBlobSha(snapshot.cargoLock),
    'backend/src/main.rs': gitBlobSha(snapshot.mainRs),
    '.talos/ci/fixtures/p10-sp05/p9-db-mod.rs': gitBlobSha(snapshot.p9DbMod),
    '.talos/ci/fixtures/p10-sp05/p9-integration-mod.rs': gitBlobSha(snapshot.p9IntegrationMod),
  }
  for (const [file, actual] of Object.entries(actualBlobs)) {
    if (expectedBlobs[file] !== actual) errors.push('SP05 rollback build projection blob mismatch: ' + file)
  }

  exactArray(
    errors,
    rollback.excludedTestOnlyFiles,
    ['backend/src/db/p10_restore_qualification.rs', 'backend/src/integration/p10_recovery_qualification.rs'],
    'SP05 rollback projection test-only exclusions',
  )

  const schema = contract.schemaCompatibility ?? {}
  if (
    schema.compatibleCandidateMax !== 'derived-from-projection' ||
    schema.restoredLatest !== 'derived-from-live-postgres' ||
    schema.incompatibleCase !== 'previous-migration' ||
    schema.destructiveDowngradeForbidden !== true ||
    schema.incompatibleCandidateProcessStarted !== false
  ) errors.push('SP05 schema compatibility contract weakened')

  const effect = contract.effectSafety ?? {}
  for (const key of [
    'drainBeforeCut',
    'canonicalBindingFreeze',
    'dispatchCrashCut',
    'unknownOutcomeAfterRestart',
    'blindRetryForbidden',
    'reconciliationRequired',
    'attemptCountStable',
  ]) {
    if (effect[key] !== true) errors.push('SP05 effect-safety contract weakened: ' + key)
  }
  if (effect.compensation !== 'not_applicable_nonfinancial_fixture') errors.push('SP05 compensation boundary changed')

  const forward = contract.forwardFix ?? {}
  if (
    forward.currentExactApplicationRestart !== true ||
    forward.tenantReadWriteRequired !== true ||
    forward.sqliteProductionStateForbidden !== true
  ) errors.push('SP05 forward-fix contract weakened')

  for (const claim of [
    'P10_MEASURED_FULL_REHEARSAL_PASS',
    'RECOVERY_REHEARSAL_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
    'RTO_VERIFIED',
    'RPO_VERIFIED',
  ]) {
    if (!contract.claimsExcluded?.includes(claim)) errors.push('SP05 excluded claim missing: ' + claim)
  }

  requireTokens(errors, snapshot.compatibility, 'Schema compatibility gate', [
    'ROLLBACK_SCHEMA_COMPATIBLE',
    'ROLLBACK_SCHEMA_INCOMPATIBLE',
    'candidate_process_started=false',
    'schema_downgrade_attempted=false',
    'process.exit(42)',
  ])

  requireTokens(errors, snapshot.probe, 'Effect reconciliation probe', [
    'p10_unknown_outcome_requires_reconciliation_before_retry',
    'PostgresOperationRuntimePersistence::new',
    'claim_next_operation',
    'begin_reconciliation',
    'resolve_reconciliation',
    'unknown_outcome',
    'worker_restarted_after_dispatch',
    'effect_confirmed',
    'blind_retry=false',
    'compensation=not_applicable_nonfinancial_fixture',
  ])
  requireTokens(errors, snapshot.integrationMod, 'SP05 probe module wiring', [
    '#[cfg(all(test, feature = "postgres"))]',
    'mod p10_recovery_qualification;',
  ])
  requireTokens(errors, snapshot.runtimePg, 'Production recovery authority', [
    "SET state='unknown_outcome'",
    "classification='worker_restarted_after_dispatch'",
    'fn begin_reconciliation(',
    'fn resolve_reconciliation(',
  ])
  requireTokens(errors, snapshot.operation, 'UnknownOutcome state-machine boundary', [
    'OperationState::UnknownOutcome',
    'OperationState::Reconciling',
  ])
  requireTokens(errors, snapshot.overlay, 'SP05 PG qualification overlay', [
    '127.0.0.1:15432:5432',
  ])

  requireTokens(errors, snapshot.runner, 'SP05 live rehearsal', [
    ': "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"',
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'source-migration-manifest.txt',
    'sha256sum "$file"',
    "done | sha256sum | awk '{print $1}'",
    'P9_ROLLBACK_SOURCE_SHA=2f414df89769ceedce482c4c25a24f02a24344b5',
    'P9_ROLLBACK_MAX_MIGRATION=083_r4_reservation_rule_sequence_invariant',
    'P9_BUILD_CONTEXT=',
    'git hash-object Dockerfile',
    'git hash-object backend/Cargo.toml',
    'git hash-object backend/Cargo.lock',
    'git hash-object backend/src/main.rs',
    'git hash-object "$P9_DB_MOD_FIXTURE"',
    'git hash-object "$P9_INTEGRATION_MOD_FIXTURE"',
    'cp -a backend "$P9_BUILD_CONTEXT/backend"',
    'cp "$P9_DB_MOD_FIXTURE" "$P9_BUILD_CONTEXT/backend/src/db/mod.rs"',
    'cp "$P9_INTEGRATION_MOD_FIXTURE" "$P9_BUILD_CONTEXT/backend/src/integration/mod.rs"',
    'rm -f "$P9_BUILD_CONTEXT/backend/src/db/p10_restore_qualification.rs"',
    'rm -f "$P9_BUILD_CONTEXT/backend/src/integration/p10_recovery_qualification.rs"',
    'rollback_candidate_max=',
    'test "$rollback_candidate_max" = "$P9_ROLLBACK_MAX_MIGRATION"',
    '--candidate-ref "$P9_ROLLBACK_SOURCE_SHA"',
    '--candidate-max "$rollback_candidate_max"',
    '--restored-latest "$LATEST_MIGRATION"',
    'docker build -t "$P9_IMAGE" "$P9_BUILD_CONTEXT"',
    'for _ in $(seq 1 30); do',
    'DRAIN_DISPATCHING=',
    'DRAIN_WORKFLOW=',
    'DRAIN_OUTBOX=',
    'P10_SP05_DEFERRED_OPERATION operation=%s circuit=%s',
    "test \"$DEFERRED_OPERATION\" = 'ready|0|9999-12-31T23:59:59Z'",
    "test \"$CIRCUIT_STATE\" = 'open|9999-12-31T23:59:59Z'",
    'P10_SP05_DRAIN dispatching=%s workflow_running=%s outbox_processing=%s',
    'test "$DRAIN_DISPATCHING" = 0',
    "SET state='dispatching',attempt_count=1",
    "'claimed','p10_sp05_controlled_crash_cut'",
    '--data "$(binding_body false)"',
    'provider_binding_history WHERE tenant_id=',
    "action='disabled'",
    'compose stop -t 20 nginx app',
    '--network-alias app',
    '-e DATABASE_URL="$TALOS_PRODUCTION_DATABASE_URL"',
    'unknown_outcome|1|worker_restarted_after_dispatch',
    'incompatible_candidate_max=',
    '--candidate-ref qualification-incompatible-previous-migration',
    'test "$BLOCK_STATUS" = 42',
    'ROLLBACK_SCHEMA_INCOMPATIBLE',
    'compose up -d --no-deps --force-recreate app nginx',
    'TALOS_P10_RECOVERY_DATABASE_URL=',
    'p10_unknown_outcome_requires_reconciliation_before_retry',
    'P10_EFFECT_RECONCILIATION',
    `test "$FINAL_OPERATION" = 'resolved|1'`,
    `test "$FINAL_RECONCILIATION" = 'effect_confirmed|p10-sp05-recovery-operator'`,
    'test "$FINAL_ATTEMPTS" = 1',
    'test "$FINAL_BINDING" = f',
    'scan_evidence()',
    'if ! scan_evidence; then',
    'source_sha=$TALOS_QUALIFIED_SOURCE_SHA',
    'source_tree=$TALOS_QUALIFIED_SOURCE_TREE_SHA',
    'rollback_projection=runtime_build_projection',
    'P10_ROLLBACK_FORWARD_FIX_PASS',
  ])

  for (const forbidden of [
    'git worktree add',
    'git cat-file -e "$P9_ROLLBACK',
    'P9_WORKTREE=',
    'test "$LATEST_MIGRATION" = 083_',
    '--candidate-max 083_',
    '--candidate-max 082_',
    'ALTER TABLE DROP',
    'DROP COLUMN',
    'DELETE FROM schema_migrations',
    'r4-p10-sp04-restored-runtime.sh',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ]) {
    if (snapshot.runner.includes(forbidden)) errors.push('SP05 crosses authority or private-history boundary: ' + forbidden)
  }

  const drain = snapshot.runner.indexOf('DRAIN_DISPATCHING=')
  const crashCut = snapshot.runner.indexOf("SET state='dispatching',attempt_count=1")
  const disable = snapshot.runner.indexOf('--data "$(binding_body false)"')
  const stopCurrent = snapshot.runner.indexOf('compose stop -t 20 nginx app')
  const rollbackRun = snapshot.runner.indexOf('--network-alias app')
  const unknown = snapshot.runner.indexOf('unknown_outcome|1|worker_restarted_after_dispatch')
  const incompatible = snapshot.runner.indexOf('--candidate-ref qualification-incompatible-previous-migration')
  const forwardFix = snapshot.runner.indexOf('compose up -d --no-deps --force-recreate app nginx')
  const reconcile = snapshot.runner.indexOf('p10_unknown_outcome_requires_reconciliation_before_retry')
  if (!(
    drain >= 0 &&
    drain < crashCut &&
    crashCut < disable &&
    disable < stopCurrent &&
    stopCurrent < rollbackRun &&
    rollbackRun < unknown &&
    unknown < incompatible &&
    incompatible < forwardFix &&
    forwardFix < reconcile
  )) {
    errors.push('SP05 order must be drain -> dispatch cut -> binding freeze -> rollback -> UnknownOutcome -> incompatible block -> forward-fix -> reconciliation')
  }

  const cleanupStart = snapshot.runner.indexOf('cleanup() {')
  const cleanupScan = snapshot.runner.indexOf('if ! scan_evidence; then', cleanupStart)
  const cleanupRuntimeRemoval = snapshot.runner.indexOf('rm -rf "$RUNTIME_DIR"', cleanupStart)
  if (!(cleanupStart >= 0 && cleanupStart < cleanupScan && cleanupScan < cleanupRuntimeRemoval)) {
    errors.push('SP05 cleanup must fail-close on final evidence scan before runtime secret removal')
  }

  requireTokens(errors, snapshot.registry, 'P10-SP05 package registration', [
    '"branch_prefix": "agent/r4-p10-sp05-"',
    '"scripts/check-r4-p10-sp05-rollback-forward-fix.test.mjs"',
    '"scripts/check-r4-p10-sp05-rollback-forward-fix.mjs"',
    '"qualification_script": "scripts/r4-p10-sp05-rollback-forward-fix.sh"',
    '"label": "p10-sp05-rollback-forward-fix"',
  ])
  requireTokens(errors, snapshot.workflow, 'Tiered Exact-Head dispatcher', [
    'Current package qualification',
    'ci-package-qualification.mjs',
    'ci-run-package-qualification.mjs',
    'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha',
    'TALOS_QUALIFIED_SOURCE_TREE_SHA=$qualified_source_tree',
  ])
  for (const forbidden of [
    'p10_sp05_rollback_forward_fix:',
    'bash scripts/r4-p10-sp05-rollback-forward-fix.sh',
  ]) {
    if (snapshot.workflow.includes(forbidden)) errors.push('Exact-Head must not hard-code P10-SP05 package job: ' + forbidden)
  }

  return errors
}

function main() {
  const errors = validateP10Sp05Snapshot(collectP10Sp05Snapshot())
  if (errors.length > 0) {
    console.error('R4-P10-SP05 rollback/forward-fix gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P10-SP05 rollback/forward-fix gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
