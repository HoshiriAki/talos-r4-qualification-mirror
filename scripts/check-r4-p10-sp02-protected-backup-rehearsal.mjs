#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const PATHS = Object.freeze({
  contract: '.talos/ci/r4-p10-sp02-protected-backup-rehearsal.json',
  sp01Contract: '.talos/ci/r4-p10-sp01-backup-artifact-contract.json',
  runner: 'scripts/r4-p10-sp02-protected-backup-rehearsal.sh',
  backup: 'scripts/r4-p10-pg-backup.sh',
  verify: 'scripts/r4-p10-verify-backup.sh',
  registry: '.talos/ci/package-qualifications.json',
  workflow: '.github/workflows/exact-head-qualification.yml',
})

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP10Sp02Snapshot() {
  return {
    contract: JSON.parse(read(PATHS.contract)),
    sp01Contract: JSON.parse(read(PATHS.sp01Contract)),
    runner: read(PATHS.runner),
    backup: read(PATHS.backup),
    verify: read(PATHS.verify),
    registry: read(PATHS.registry),
    workflow: read(PATHS.workflow),
  }
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) errors.push(label + ' missing: ' + token)
  }
}

function exactArray(errors, actual, expected, label) {
  if (!Array.isArray(actual) || JSON.stringify(actual) !== JSON.stringify(expected)) {
    errors.push(label + ' does not match the frozen contract')
  }
}

export function validateP10Sp02Snapshot(snapshot) {
  const errors = []
  const contract = snapshot.contract ?? {}

  if (contract.schema !== 'talos.p10.protected-backup-rehearsal-contract/v1') {
    errors.push('SP02 machine contract schema mismatch')
  }
  if (contract.scope !== 'R4-P10-SP02') errors.push('SP02 machine contract scope mismatch')
  if (contract.exitGate !== 'P10_PROTECTED_BACKUP_REHEARSAL_PASS') {
    errors.push('SP02 machine contract exit gate mismatch')
  }
  exactArray(errors, contract.dependsOn, ['P10_BACKUP_ARTIFACT_CONTRACT_PASS'], 'SP02 dependency gate')

  if (
    contract.sourceProfile?.postgresMajor !== 18 ||
    contract.sourceProfile?.productionLike !== true
  ) {
    errors.push('SP02 source profile must remain production-like PostgreSQL 18')
  }
  exactArray(
    errors,
    contract.sourceProfile?.composeFiles,
    ['deploy/compose.production.yml', 'deploy/compose.p9-sp03.yml'],
    'SP02 compose profile',
  )

  const identity = contract.authoritativeIdentity ?? {}
  if (
    identity.sourceShaEnv !== 'TALOS_QUALIFIED_SOURCE_SHA' ||
    identity.sourceTreeEnv !== 'TALOS_QUALIFIED_SOURCE_TREE_SHA' ||
    identity.migrationManifest !== 'per-file-sha256' ||
    identity.migrationHeadRequired !== true ||
    identity.transportShaIsNotSourceAuthority !== true
  ) {
    errors.push('SP02 authoritative source identity contract weakened')
  }

  const cut = contract.cutSafety ?? {}
  exactArray(errors, cut.stopBeforeCut, ['nginx', 'app'], 'SP02 quiesce process set')
  exactArray(
    errors,
    cut.requiredZeroStates,
    ['external_operations.dispatching', 'workflow_steps.running', 'domain_outbox.processing'],
    'SP02 effect-safety zero-state set',
  )
  if (
    cut.preBackupMarker !== 'pre_backup' ||
    cut.postBackupMarker !== 'post_backup' ||
    cut.postMarkerAfterProtectedVerification !== true ||
    cut.ciphertextStableAfterPostMarker !== true
  ) {
    errors.push('SP02 backup-cut marker contract weakened')
  }

  if (
    contract.protectedArtifact?.backupTool !== 'scripts/r4-p10-pg-backup.sh' ||
    contract.protectedArtifact?.verifyTool !== 'scripts/r4-p10-verify-backup.sh' ||
    contract.protectedArtifact?.plaintextRetained !== false ||
    contract.protectedArtifact?.runtimeSecretsRetained !== false
  ) {
    errors.push('SP02 protected artifact contract weakened')
  }

  for (const claim of [
    'P10_ISOLATED_RESTORE_PASS',
    'RECOVERY_REHEARSAL_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
    'RTO_VERIFIED',
    'RPO_VERIFIED',
  ]) {
    if (!contract.claimsExcluded?.includes(claim)) {
      errors.push('SP02 excluded claim missing: ' + claim)
    }
  }

  if (
    snapshot.sp01Contract?.schema !== 'talos.p10.backup-artifact-contract/v1' ||
    snapshot.sp01Contract?.exitGate !== 'P10_BACKUP_ARTIFACT_CONTRACT_PASS'
  ) {
    errors.push('SP02 must depend on the qualified SP01 backup artifact contract')
  }

  requireTokens(errors, snapshot.runner, 'SP02 live qualification', [
    ': "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"',
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'transport-sha.txt',
    'source-sha.txt',
    'source-tree-sha.txt',
    'backend/src/db/migrations/postgres',
    'sha256sum "$file"',
    "done | sha256sum | awk '{print $1}'",
    'migration-manifest.txt',
    'deploy/compose.production.yml',
    'deploy/compose.p9-sp03.yml',
    '/auth/platform/login',
    '/api/tenants',
    '/api/machine/v1/tenants/',
    'device.create_device',
    '/api/integrations/manifests',
    '/api/integrations/instances',
    '/api/integrations/bindings',
    '/api/integrations/operations',
    'integration_circuit_state',
    'state || \'|\' || attempt_count',
    '"ready|0"',
    '"${compose[@]}" stop -t 20 nginx app',
    'workflow_instances',
    'workflow_steps',
    'domain_outbox',
    'domain_inbox',
    'maxwell.rental.v1',
    'p10_recovery_markers',
    "'pre_backup'",
    "'post_backup'",
    "SELECT COUNT(*) FROM external_operations WHERE state='dispatching'",
    "SELECT COUNT(*) FROM workflow_steps WHERE state='running'",
    "SELECT COUNT(*) FROM domain_outbox WHERE state='processing'",
    'test "$DISPATCHING_COUNT" = "0"',
    'test "$RUNNING_WORKFLOW_COUNT" = "0"',
    'test "$PROCESSING_OUTBOX_COUNT" = "0"',
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"',
    'P10_MIGRATION_HEAD="$migration_head"',
    'P10_OPERATOR_ID="${GITHUB_ACTOR:-local-operator}"',
    'bash scripts/r4-p10-pg-backup.sh',
    'bash scripts/r4-p10-verify-backup.sh',
    'CIPHERTEXT_SHA_BEFORE=',
    'CIPHERTEXT_SHA_AFTER=',
    'test "$CIPHERTEXT_SHA_AFTER" = "$CIPHERTEXT_SHA_BEFORE"',
    'post_backup_marker_before_cut=',
    'post_backup_marker_after_cut=',
    'runtime secret leaked into retained SP02 evidence',
    'rm -f "$BACKUP_KEY_FILE" "$PGPASS_FILE"',
    'P10_PROTECTED_BACKUP_REHEARSAL_PASS',
  ])

  for (const forbidden of [
    'scripts/backup.sh',
    'scripts/db-backup.js',
    'scripts/dr-restore.sh',
    'pg_restore --',
    '--symmetric',
    '--cipher-algo',
    'P10_ISOLATED_RESTORE_PASS',
    'RECOVERY_REHEARSAL_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ]) {
    if (snapshot.runner.includes(forbidden)) {
      errors.push('SP02 runner crosses authority boundary or duplicates SP01 tooling: ' + forbidden)
    }
  }

  const quiesce = snapshot.runner.indexOf('"${compose[@]}" stop -t 20 nginx app')
  const preMarker = snapshot.runner.indexOf("VALUES (:'pre_marker','pre_backup'")
  const backup = snapshot.runner.indexOf('bash scripts/r4-p10-pg-backup.sh')
  const firstVerify = snapshot.runner.indexOf('bash scripts/r4-p10-verify-backup.sh')
  const postMarker = snapshot.runner.indexOf("VALUES (:'post_marker','post_backup'")
  const secondVerify = snapshot.runner.indexOf(
    'bash scripts/r4-p10-verify-backup.sh',
    firstVerify + 1,
  )
  if (!(
    quiesce >= 0 &&
    quiesce < preMarker &&
    preMarker < backup &&
    backup < firstVerify &&
    firstVerify < postMarker &&
    postMarker < secondVerify
  )) {
    errors.push('SP02 cut order must be quiesce -> pre marker -> protected backup -> verify -> post marker -> reverify')
  }

  requireTokens(errors, snapshot.backup, 'Qualified SP01 backup dependency', [
    ': "${P10_SOURCE_TREE:?P10_SOURCE_TREE is required}"',
    ': "${P10_MIGRATION_MANIFEST_SHA256:?P10_MIGRATION_MANIFEST_SHA256 is required}"',
    ': "${P10_MIGRATION_HEAD:?P10_MIGRATION_HEAD is required}"',
    ': "${P10_OPERATOR_ID:?P10_OPERATOR_ID is required}"',
    'talos.p10.backup-metadata/v1',
    '--format=custom',
    '--no-owner',
    '--no-acl',
    '--symmetric',
    '--cipher-algo AES256',
    'published=true',
  ])
  requireTokens(errors, snapshot.verify, 'Qualified SP01 verifier dependency', [
    'talos.p10.backup-metadata/v1',
    'actual_protected_sha256=',
    '--decrypt "$P10_BACKUP_ARTIFACT"',
    'pg_restore --list /backup/verified.dump',
  ])

  requireTokens(errors, snapshot.registry, 'P10-SP02 package registration', [
    '"branch_prefix": "agent/r4-p10-sp02-"',
    '"scripts/check-r4-p10-sp02-protected-backup-rehearsal.test.mjs"',
    '"scripts/check-r4-p10-sp02-protected-backup-rehearsal.mjs"',
    '"qualification_script": "scripts/r4-p10-sp02-protected-backup-rehearsal.sh"',
    '"label": "p10-sp02-protected-backup-rehearsal"',
  ])

  requireTokens(errors, snapshot.workflow, 'Tiered Exact-Head dispatcher', [
    'Current package qualification',
    'ci-package-qualification.mjs',
    'ci-run-package-qualification.mjs',
    'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha',
    'TALOS_QUALIFIED_SOURCE_TREE_SHA=$qualified_source_tree',
  ])
  for (const forbidden of [
    'p10_sp02_protected_backup_rehearsal:',
    'name: P10-SP02 production-like protected backup rehearsal',
    'bash scripts/r4-p10-sp02-protected-backup-rehearsal.sh',
  ]) {
    if (snapshot.workflow.includes(forbidden)) {
      errors.push('Exact-Head must not hard-code P10-SP02 package job: ' + forbidden)
    }
  }

  return errors
}

function main() {
  const errors = validateP10Sp02Snapshot(collectP10Sp02Snapshot())
  if (errors.length > 0) {
    console.error('R4-P10-SP02 protected backup rehearsal gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P10-SP02 protected backup rehearsal gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
