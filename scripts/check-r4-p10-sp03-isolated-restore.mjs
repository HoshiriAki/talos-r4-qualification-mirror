#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const PATHS = Object.freeze({
  contract: '.talos/ci/r4-p10-sp03-isolated-restore.json',
  runner: 'scripts/r4-p10-sp03-isolated-restore.sh',
  restore: 'scripts/r4-p10-pg-restore.sh',
  verify: 'scripts/r4-p10-verify-backup.sh',
  migrationProbe: 'backend/src/db/p10_restore_qualification.rs',
  dbMod: 'backend/src/db/mod.rs',
  registry: '.talos/ci/package-qualifications.json',
  workflow: '.github/workflows/exact-head-qualification.yml',
})

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP10Sp03Snapshot() {
  return {
    contract: JSON.parse(read(PATHS.contract)),
    runner: read(PATHS.runner),
    restore: read(PATHS.restore),
    verify: read(PATHS.verify),
    migrationProbe: read(PATHS.migrationProbe),
    dbMod: read(PATHS.dbMod),
    registry: read(PATHS.registry),
    workflow: read(PATHS.workflow),
  }
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) errors.push(label + ' missing: ' + token)
  }
}

function requireExactCount(errors, source, token, count, label) {
  const actual = source.split(token).length - 1
  if (actual !== count) errors.push(label + ' expected ' + count + ' occurrences of ' + token + ', got ' + actual)
}

function requireShellSyntax(errors, source, label) {
  const result = spawnSync('bash', ['-n'], { input: source, encoding: 'utf8' })
  if (result.error) {
    errors.push(label + ' syntax check could not execute: ' + result.error.message)
    return
  }
  if (result.status !== 0) {
    const detail = String(result.stderr || '').trim().replaceAll('\n', ' ')
    errors.push(label + ' shell syntax invalid' + (detail ? ': ' + detail : ''))
  }
}

function exactArray(errors, actual, expected, label) {
  if (!Array.isArray(actual) || JSON.stringify(actual) !== JSON.stringify(expected)) {
    errors.push(label + ' does not match the frozen contract')
  }
}

export function validateP10Sp03Snapshot(snapshot) {
  const errors = []
  const contract = snapshot.contract ?? {}

  requireShellSyntax(errors, snapshot.runner, 'SP03 qualification runner')
  requireShellSyntax(errors, snapshot.restore, 'SP03 restore tool')

  if (contract.schema !== 'talos.p10.isolated-restore-contract/v1') errors.push('SP03 machine contract schema mismatch')
  if (contract.scope !== 'R4-P10-SP03') errors.push('SP03 machine contract scope mismatch')
  if (contract.exitGate !== 'P10_ISOLATED_RESTORE_PASS') errors.push('SP03 machine contract exit gate mismatch')
  exactArray(
    errors,
    contract.dependsOn,
    ['P10_BACKUP_ARTIFACT_CONTRACT_PASS', 'P10_PROTECTED_BACKUP_REHEARSAL_PASS'],
    'SP03 dependency gates',
  )

  const artifact = contract.sourceArtifact ?? {}
  if (
    artifact.exactSourceSha !== true ||
    artifact.exactSourceTree !== true ||
    artifact.repositoryMigrationManifest !== true ||
    artifact.protectedVerificationBeforeRestore !== true ||
    artifact.priorRuntimeKeyReuseForbidden !== true
  ) {
    errors.push('SP03 source artifact binding contract weakened')
  }

  const authority = contract.restoreAuthority ?? {}
  if (
    authority.postgresMajor !== 18 ||
    authority.freshDatabaseRequired !== true ||
    authority.differentContainerIdentity !== true ||
    authority.differentSystemIdentifier !== true ||
    authority.plaintextOnlyInPrivateRuntime !== true ||
    authority.sourceDatabaseUsedAsRestoreInput !== false
  ) {
    errors.push('SP03 fresh isolated restore authority contract weakened')
  }

  const invariants = contract.invariants ?? {}
  if (
    invariants.preBackupMarkerPresent !== true ||
    invariants.postBackupMarkerAbsent !== true ||
    invariants.sourceAuthorityUnchanged !== true ||
    invariants.productionMigrationAuthorityRuns !== 2 ||
    invariants.productionMigrationExecutionsExpected !== 0
  ) {
    errors.push('SP03 restored invariant contract weakened')
  }

  for (const claim of [
    'P10_RESTORED_RUNTIME_PASS',
    'P10_ROLLBACK_FORWARD_FIX_PASS',
    'RECOVERY_REHEARSAL_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
    'RTO_VERIFIED',
    'RPO_VERIFIED',
  ]) {
    if (!contract.claimsExcluded?.includes(claim)) errors.push('SP03 excluded claim missing: ' + claim)
  }

  requireTokens(errors, snapshot.restore, 'SP03 restore tool', [
    'P10_RESTORE_NETWORK',
    'P10_RESTORE_HOST',
    'P10_RESTORE_DATABASE',
    'P10_RESTORE_PGPASS_FILE',
    'P10_EXPECTED_SOURCE_SHA',
    'P10_EXPECTED_SOURCE_TREE',
    'P10_EXPECTED_MIGRATION_MANIFEST_SHA256',
    'P10_EXPECTED_MIGRATION_HEAD',
    'data.get("source_sha") == os.environ["P10_EXPECTED_SOURCE_SHA"]',
    'data.get("source_tree") == os.environ["P10_EXPECTED_SOURCE_TREE"]',
    'migration.get("repository_manifest_sha256") == os.environ["P10_EXPECTED_MIGRATION_MANIFEST_SHA256"]',
    'migration.get("repository_head") == os.environ["P10_EXPECTED_MIGRATION_HEAD"]',
    'gpg_home="$work_dir/gnupg"',
    'chmod 700 "$gpg_home"',
    'actual_protected_sha256="$(sha256sum "$P10_BACKUP_ARTIFACT" | awk \'{print $1}\')"',
    'test "$actual_protected_sha256" = "$protected_sha256"',
    'actual_plaintext_sha256="$(sha256sum "$plaintext" | awk \'{print $1}\')"',
    'test "$actual_plaintext_sha256" = "$plaintext_sha256"',
    'pg_restore',
    '--exit-on-error',
    '--no-owner',
    '--no-acl',
    'rm -f "$plaintext"\ntest ! -e "$plaintext"',
    'plaintext_removed=true',
  ])
  for (const forbidden of ['scripts/backup.sh', 'scripts/db-backup.js', 'scripts/dr-restore.sh', 'sqlite']) {
    if (snapshot.restore.toLowerCase().includes(forbidden.toLowerCase())) {
      errors.push('SP03 restore tool crosses PostgreSQL recovery authority: ' + forbidden)
    }
  }

  requireTokens(errors, snapshot.migrationProbe, 'SP03 migration idempotency probe', [
    'TALOS_P10_RESTORE_DATABASE_URL',
    'TALOS_P10_EXPECTED_MIGRATION_ID',
    'crate::db::run_all_pg_migrations(&pool).await?',
    'before_latest == expected_latest',
    'first.is_empty()',
    'second.is_empty()',
    'P10_RESTORE_MIGRATION_IDEMPOTENT',
  ])
  requireExactCount(
    errors,
    snapshot.migrationProbe,
    'crate::db::run_all_pg_migrations(&pool).await?',
    2,
    'SP03 migration probe production-authority invocation',
  )
  if (snapshot.migrationProbe.includes('"083_r4_reservation_rule_sequence_invariant"')) {
    errors.push('SP03 migration probe must not hard-code a historical latest migration id')
  }
  requireTokens(errors, snapshot.dbMod, 'SP03 migration probe wiring', [
    '#[cfg(all(test, feature = "postgres"))]\nmod p10_restore_qualification;',
  ])

  requireTokens(errors, snapshot.runner, 'SP03 live restore qualification', [
    ': "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"',
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'transport-sha.txt',
    'source-tree-sha.txt',
    'backend/src/db/migrations/postgres',
    'sha256sum "$file"',
    "done | sha256sum | awk '{print $1}'",
    'migration_registry_head="${migration_head%.sql}"',
    'deploy/compose.production.yml',
    '/api/machine/v1/tenants/',
    '/api/integrations/operations',
    '"${compose[@]}" stop -t 20 nginx app',
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"',
    'P10_MIGRATION_HEAD="$migration_head"',
    'P10_OPERATOR_ID="${GITHUB_ACTOR:-local-operator}"',
    'bash scripts/r4-p10-pg-backup.sh',
    'bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/protected-verify-before-restore.log"',
    'docker network create "$RESTORE_NETWORK"',
    'docker run -d',
    '-p 127.0.0.1::5432',
    'RESTORE_SYSTEM_ID=',
    'test "$RESTORE_CONTAINER_ID" != "$DB_CONTAINER"',
    'test "$RESTORE_SYSTEM_ID" != "$PG_SYSTEM_ID"',
    'RESTORE_PUBLIC_TABLES_BEFORE=',
    'RESTORE_SCHEMA_MIGRATIONS_ABSENT_BEFORE=',
    'test "$RESTORE_PUBLIC_TABLES_BEFORE" = "0"',
    'test "$RESTORE_SCHEMA_MIGRATIONS_ABSENT_BEFORE" = "t"',
    'P10_EXPECTED_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_EXPECTED_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_EXPECTED_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"',
    'P10_EXPECTED_MIGRATION_HEAD="$migration_head"',
    'bash scripts/r4-p10-pg-restore.sh',
    'test "$RESTORE_PRE_MARKER_COUNT" = "1"',
    'test "$RESTORE_POST_MARKER_COUNT" = "0"',
    'TALOS_P10_EXPECTED_MIGRATION_ID="$migration_registry_head"',
    'restored_pg18_migration_chain_is_idempotent',
    'SOURCE_SNAPSHOT_AFTER=',
    'test "$SOURCE_SNAPSHOT_AFTER" = "$SOURCE_SNAPSHOT_BEFORE"',
    'runtime secret leaked into retained SP03 evidence',
    'P10_ISOLATED_RESTORE_PASS',
  ])

  if (snapshot.runner.includes('--no-default-features')) {
    errors.push('SP03 migration probe must use the already-qualified dual-feature talos-backend test target')
  }

  const backup = snapshot.runner.indexOf('bash scripts/r4-p10-pg-backup.sh')
  const postMarker = snapshot.runner.indexOf("VALUES (:'post_marker','post_backup'")
  const verifyBeforeRestore = snapshot.runner.indexOf('bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/protected-verify-before-restore.log"')
  const restore = snapshot.runner.indexOf('bash scripts/r4-p10-pg-restore.sh')
  const invariant = snapshot.runner.indexOf('RESTORE_POST_MARKER_COUNT=')
  const migrationProbe = snapshot.runner.indexOf('restored_pg18_migration_chain_is_idempotent')
  const sourceAfter = snapshot.runner.indexOf('SOURCE_SNAPSHOT_AFTER=')
  if (!(backup >= 0 && backup < postMarker && postMarker < verifyBeforeRestore && verifyBeforeRestore < restore && restore < invariant && invariant < migrationProbe && migrationProbe < sourceAfter)) {
    errors.push('SP03 order must be backup -> source post marker -> verify -> fresh restore -> restored invariants -> migration probe -> source immutability')
  }

  requireExactCount(
    errors,
    snapshot.runner,
    '"${compose[@]}" up -d --no-build db app nginx',
    1,
    'SP03 source application startup',
  )
  if (snapshot.runner.indexOf('"${compose[@]}" up -d --no-build db app nginx', restore) >= 0) {
    errors.push('SP03 must not restart the TALOS application after restore; restored runtime belongs to SP04')
  }

  for (const forbidden of [
    'P10_RESTORED_RUNTIME_PASS',
    'P10_ROLLBACK_FORWARD_FIX_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
    'r4-p10-sp02-protected-backup-rehearsal.sh',
  ]) {
    if (snapshot.runner.includes(forbidden)) errors.push('SP03 runner crosses its package boundary: ' + forbidden)
  }

  requireTokens(errors, snapshot.verify, 'Qualified SP01 verifier dependency', [
    'talos.p10.backup-metadata/v1',
    'actual_protected_sha256=',
    '--decrypt "$P10_BACKUP_ARTIFACT"',
    'pg_restore --list /backup/verified.dump',
  ])

  requireTokens(errors, snapshot.registry, 'P10-SP03 package registration', [
    '"branch_prefix": "agent/r4-p10-sp03-"',
    '"scripts/check-r4-p10-sp03-isolated-restore.test.mjs"',
    '"scripts/check-r4-p10-sp03-isolated-restore.mjs"',
    '"qualification_script": "scripts/r4-p10-sp03-isolated-restore.sh"',
    '"label": "p10-sp03-isolated-restore"',
  ])
  requireTokens(errors, snapshot.workflow, 'Tiered Exact-Head dispatcher', [
    'Current package qualification',
    'ci-package-qualification.mjs',
    'ci-run-package-qualification.mjs',
    'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha',
    'TALOS_QUALIFIED_SOURCE_TREE_SHA=$qualified_source_tree',
  ])
  for (const forbidden of [
    'p10_sp03_isolated_restore:',
    'name: P10-SP03 fresh isolated PostgreSQL 18 restore',
    'bash scripts/r4-p10-sp03-isolated-restore.sh',
  ]) {
    if (snapshot.workflow.includes(forbidden)) errors.push('Exact-Head must not hard-code P10-SP03 package job: ' + forbidden)
  }

  return errors
}

function main() {
  const errors = validateP10Sp03Snapshot(collectP10Sp03Snapshot())
  if (errors.length > 0) {
    console.error('R4-P10-SP03 isolated restore gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P10-SP03 isolated restore gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
