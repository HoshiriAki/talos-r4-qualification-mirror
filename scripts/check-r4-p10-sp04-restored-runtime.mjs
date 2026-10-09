#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const PATHS = Object.freeze({
  contract: '.talos/ci/r4-p10-sp04-restored-runtime.json',
  runner: 'scripts/r4-p10-sp04-restored-runtime.sh',
  backup: 'scripts/r4-p10-pg-backup.sh',
  verify: 'scripts/r4-p10-verify-backup.sh',
  restore: 'scripts/r4-p10-pg-restore.sh',
  migrationProbe: 'backend/src/db/p10_restore_qualification.rs',
  productionCompose: 'deploy/compose.production.yml',
  authOverlay: 'deploy/compose.p9-sp03.yml',
  registry: '.talos/ci/package-qualifications.json',
  workflow: '.github/workflows/exact-head-qualification.yml',
})

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP10Sp04Snapshot() {
  return {
    contract: JSON.parse(read(PATHS.contract)),
    runner: read(PATHS.runner),
    backup: read(PATHS.backup),
    verify: read(PATHS.verify),
    restore: read(PATHS.restore),
    migrationProbe: read(PATHS.migrationProbe),
    productionCompose: read(PATHS.productionCompose),
    authOverlay: read(PATHS.authOverlay),
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

export function validateP10Sp04Snapshot(snapshot) {
  const errors = []
  const contract = snapshot.contract ?? {}

  requireShellSyntax(errors, snapshot.runner, 'SP04 qualification runner')

  if (contract.schema !== 'talos.p10.restored-runtime-contract/v1') errors.push('SP04 machine contract schema mismatch')
  if (contract.scope !== 'R4-P10-SP04') errors.push('SP04 machine contract scope mismatch')
  if (contract.exitGate !== 'P10_RESTORED_RUNTIME_PASS') errors.push('SP04 machine contract exit gate mismatch')
  exactArray(
    errors,
    contract.dependsOn,
    ['P10_BACKUP_ARTIFACT_CONTRACT_PASS', 'P10_PROTECTED_BACKUP_REHEARSAL_PASS', 'P10_ISOLATED_RESTORE_PASS'],
    'SP04 dependency gates',
  )

  const source = contract.sourceArtifact ?? {}
  if (
    source.exactSourceSha !== true ||
    source.exactSourceTree !== true ||
    source.repositoryMigrationManifest !== true ||
    source.protectedVerificationBeforeRestore !== true
  ) errors.push('SP04 exact source artifact contract weakened')

  const runtime = contract.restoredRuntime ?? {}
  if (
    runtime.postgresMajor !== 18 ||
    runtime.sourceDatabaseDisconnectedBeforeRuntime !== true ||
    runtime.restoredDatabaseBoundToProductionAlias !== true ||
    runtime.restoredAppFreshContainer !== true ||
    runtime.readinessRequiresPostgres !== true ||
    runtime.sqliteProductionStateForbidden !== true
  ) errors.push('SP04 restored runtime authority contract weakened')

  const identity = contract.identityContinuity ?? {}
  if (
    identity.platformIdentityStable !== true ||
    identity.tenantAuthenticationRequired !== true ||
    identity.preBackupMachineCredentialUsable !== true
  ) errors.push('SP04 identity continuity contract weakened')

  const business = contract.businessEvidence ?? {}
  if (
    business.restoredExistingReadRequired !== true ||
    business.restoredNewTenantWriteRequired !== true ||
    business.tenantIsolationRequired !== true ||
    business.canonicalAuditRequired !== true ||
    business.sourceCorrelationAbsent !== true ||
    business.sourceNewWriteAbsent !== true
  ) errors.push('SP04 restored business evidence contract weakened')

  const recovery = contract.recoveryState ?? {}
  if (
    recovery.integrationOperationGoverned !== true ||
    recovery.integrationCircuitGoverned !== true ||
    recovery.preBackupMarkerPresent !== true ||
    recovery.postBackupMarkerAbsent !== true ||
    recovery.sourceAuthorityUnchanged !== true
  ) errors.push('SP04 recovery state contract weakened')

  if (
    contract.evidence?.finalSecretScanRequired !== true ||
    contract.evidence?.finalLogsBeforeSecretScan !== true ||
    contract.evidence?.runtimeSecretsRetained !== false
  ) errors.push('SP04 retained evidence contract weakened')

  for (const claim of [
    'P10_ROLLBACK_FORWARD_FIX_PASS',
    'RECOVERY_REHEARSAL_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
    'RTO_VERIFIED',
    'RPO_VERIFIED',
  ]) {
    if (!contract.claimsExcluded?.includes(claim)) errors.push('SP04 excluded claim missing: ' + claim)
  }

  requireTokens(errors, snapshot.productionCompose, 'Production Compose contract', [
    'DB_BACKEND: postgres',
    'DATABASE_URL: ${TALOS_PRODUCTION_DATABASE_URL:?TALOS_PRODUCTION_DATABASE_URL is required}',
    'ipv4_address: 172.29.0.10',
    'ipv4_address: 172.29.0.20',
    'ipv4_address: 172.29.0.30',
  ])
  requireTokens(errors, snapshot.authOverlay, 'Qualification auth overlay', [
    'AUTH_BOOTSTRAP_ON_START: "true"',
    'AUTH_BOOTSTRAP_ADMIN_USERNAME: ${P9_SP03_BOOTSTRAP_USERNAME:?P9_SP03_BOOTSTRAP_USERNAME is required}',
    'AUTH_BOOTSTRAP_ADMIN_PASSWORD: ${P9_SP03_BOOTSTRAP_PASSWORD:?P9_SP03_BOOTSTRAP_PASSWORD is required}',
  ])

  requireTokens(errors, snapshot.runner, 'SP04 live restored runtime', [
    ': "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"',
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'transport-sha.txt',
    'source-tree-sha.txt',
    'backend/src/db/migrations/postgres',
    'sha256sum "$file"',
    "done | sha256sum | awk '{print $1}'",
    'migration_registry_head="${migration_head%.sql}"',
    'deploy/compose.production.yml',
    'deploy/compose.p9-sp03.yml',
    '"${compose[@]}" stop -t 20 nginx app',
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"',
    'P10_MIGRATION_HEAD="$migration_head"',
    'P10_OPERATOR_ID="${GITHUB_ACTOR:-local-operator}"',
    'bash scripts/r4-p10-pg-backup.sh',
    'bash scripts/r4-p10-verify-backup.sh',
    'P10_EXPECTED_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_EXPECTED_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_EXPECTED_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"',
    'P10_EXPECTED_MIGRATION_HEAD="$migration_head"',
    'bash scripts/r4-p10-pg-restore.sh',
    'TALOS_P10_EXPECTED_MIGRATION_ID="$migration_registry_head"',
    'restored_pg18_migration_chain_is_idempotent',
    'test "$SOURCE_SNAPSHOT_AFTER" = "$SOURCE_SNAPSHOT_BEFORE"',
    'docker network disconnect "$DB_NETWORK" "$DB_CONTAINER"',
    'docker network connect --alias db --ip 172.29.0.10 "$DB_NETWORK" "$RESTORE_CONTAINER"',
    'test "$SOURCE_DB_BACKEND_ATTACHED" = "no"',
    'test "$RESTORE_DB_BACKEND_ATTACHED" = "yes"',
    'export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${RESTORE_PASSWORD}@db:5432/talos"',
    '"${compose[@]}" up -d --no-deps --force-recreate app nginx',
    'test "$RESTORED_APP_CONTAINER" != "$APP_CONTAINER"',
    'ready-restored.json',
    'Database profile: Postgres18',
    'PostgreSQL connection established',
    'No pending PG migrations',
    '/auth/platform/login',
    '/auth/login',
    '/auth/me',
    'SOURCE_IDENTITY_ID=',
    'RESTORED_IDENTITY_ID=',
    'test "$RESTORED_IDENTITY_ID" = "$SOURCE_IDENTITY_ID"',
    'restored-existing-a.json',
    'restored-existing-b.json',
    'Authorization: Bearer $MACHINE_SECRET',
    '/api/machine/v1/tenants/',
    'RUNTIME_CORRELATION_ID=',
    'RESTORED_MACHINE_AUDIT_NEW=',
    'RESTORED_COMMAND_AUDIT_NEW=',
    'SOURCE_RUNTIME_CORRELATION_COUNT=',
    'test "$SOURCE_RUNTIME_CORRELATION_COUNT" = "0"',
    'test "$SOURCE_NEW_DEVICE_COUNT" = "0"',
    'RESTORED_RUNNING_WORKFLOW=',
    'RESTORED_PROCESSING_OUTBOX=',
    'test "$RESTORED_OPERATION_AFTER_RUNTIME" = "ready|0"',
    'test "$RESTORED_CIRCUIT_AFTER_RUNTIME" = "open"',
    'test "$RESTORED_PRE_MARKER_AFTER_RUNTIME" = "1"',
    'test "$RESTORED_POST_MARKER_AFTER_RUNTIME" = "0"',
    'test "$RESTORE_IDENTITY_COUNT_AFTER_RUNTIME" = "$RESTORE_IDENTITY_COUNT_BEFORE_RUNTIME"',
    'RESTORED_SQLITE_FILES=',
    'test -z "$RESTORED_SQLITE_FILES"',
    'graceful shutdown complete',
    'scan_retained_evidence()',
    'forbidden runtime material retained in SP04 evidence',
    'if ! scan_retained_evidence; then',
    'evidence_scan=pass',
    'P10_SP04_RESTORED_RUNTIME',
    'P10_RESTORED_RUNTIME_PASS',
  ])

  requireExactCount(errors, snapshot.runner, '"${compose[@]}" up -d --no-build db app nginx', 1, 'SP04 source stack startup')
  requireExactCount(errors, snapshot.runner, '"${compose[@]}" up -d --no-deps --force-recreate app nginx', 1, 'SP04 restored runtime startup')

  const restore = snapshot.runner.indexOf('bash scripts/r4-p10-pg-restore.sh')
  const migrationProbe = snapshot.runner.indexOf('restored_pg18_migration_chain_is_idempotent')
  const sourceImmutable = snapshot.runner.indexOf('test "$SOURCE_SNAPSHOT_AFTER" = "$SOURCE_SNAPSHOT_BEFORE"')
  const disconnectSource = snapshot.runner.indexOf('docker network disconnect "$DB_NETWORK" "$DB_CONTAINER"')
  const connectRestore = snapshot.runner.indexOf('docker network connect --alias db --ip 172.29.0.10 "$DB_NETWORK" "$RESTORE_CONTAINER"')
  const runtimeStart = snapshot.runner.indexOf('"${compose[@]}" up -d --no-deps --force-recreate app nginx')
  const runtimeReady = snapshot.runner.indexOf('ready-restored.json')
  const platformLogin = snapshot.runner.indexOf('restored_platform_login_status=')
  const runtimeWrite = snapshot.runner.indexOf('runtime_write_status=')
  const sourceCorrelation = snapshot.runner.indexOf('SOURCE_RUNTIME_CORRELATION_COUNT=')
  const recoveryState = snapshot.runner.indexOf('RESTORED_OPERATION_AFTER_RUNTIME=')
  const sqliteAbsence = snapshot.runner.indexOf('RESTORED_SQLITE_FILES=')
  if (!(restore >= 0 && restore < migrationProbe && migrationProbe < sourceImmutable && sourceImmutable < disconnectSource && disconnectSource < connectRestore && connectRestore < runtimeStart && runtimeStart < runtimeReady && runtimeReady < platformLogin && platformLogin < runtimeWrite && runtimeWrite < sourceCorrelation && sourceCorrelation < recoveryState && recoveryState < sqliteAbsence)) {
    errors.push('SP04 order must be restore -> migration probe -> source immutability -> source disconnect -> restored db bind -> exact app start -> readiness -> auth -> write -> source absence -> recovery invariants -> SQLite absence')
  }

  const cleanupStart = snapshot.runner.indexOf('cleanup() {')
  const cleanupLogs = snapshot.runner.indexOf('compose-down.log', cleanupStart)
  const finalScan = snapshot.runner.indexOf('if ! scan_retained_evidence; then', cleanupStart)
  const runtimeRemoval = snapshot.runner.indexOf('rm -rf "$RUNTIME_DIR"', cleanupStart)
  if (!(cleanupStart >= 0 && cleanupStart < cleanupLogs && cleanupLogs < finalScan && finalScan < runtimeRemoval)) {
    errors.push('SP04 cleanup must collect final logs -> scan retained evidence -> remove runtime secrets')
  }

  for (const forbidden of [
    'r4-p10-sp03-isolated-restore.sh',
    'r4-p9-sp03-deployed-auth.sh',
    'r4-p9-sp04-business-evidence.sh',
    'P10_ROLLBACK_FORWARD_FIX_PASS',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ]) {
    if (snapshot.runner.includes(forbidden)) errors.push('SP04 runner crosses package or milestone authority: ' + forbidden)
  }

  requireTokens(errors, snapshot.backup, 'Current SP01 backup dependency', [
    ': "${P10_SOURCE_TREE:?P10_SOURCE_TREE is required}"',
    ': "${P10_MIGRATION_MANIFEST_SHA256:?P10_MIGRATION_MANIFEST_SHA256 is required}"',
    'talos.p10.backup-metadata/v1',
  ])
  requireTokens(errors, snapshot.restore, 'Current SP03 restore dependency', [
    'P10_EXPECTED_SOURCE_SHA',
    'P10_EXPECTED_SOURCE_TREE',
    'P10_EXPECTED_MIGRATION_MANIFEST_SHA256',
    'P10_EXPECTED_MIGRATION_HEAD',
    'pg_restore',
    '--exit-on-error',
    'plaintext_removed=true',
  ])
  requireTokens(errors, snapshot.migrationProbe, 'Current SP03 migration dependency', [
    'TALOS_P10_EXPECTED_MIGRATION_ID',
    'before_latest == expected_latest',
    'crate::db::run_all_pg_migrations(&pool).await?',
    'first.is_empty()',
    'second.is_empty()',
  ])

  requireTokens(errors, snapshot.registry, 'P10-SP04 package registration', [
    '"branch_prefix": "agent/r4-p10-sp04-"',
    '"scripts/check-r4-p10-sp04-restored-runtime.test.mjs"',
    '"scripts/check-r4-p10-sp04-restored-runtime.mjs"',
    '"qualification_script": "scripts/r4-p10-sp04-restored-runtime.sh"',
    '"label": "p10-sp04-restored-runtime"',
  ])

  requireTokens(errors, snapshot.workflow, 'Tiered Exact-Head dispatcher', [
    'Current package qualification',
    'ci-package-qualification.mjs',
    'ci-run-package-qualification.mjs',
    'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha',
    'TALOS_QUALIFIED_SOURCE_TREE_SHA=$qualified_source_tree',
  ])
  for (const forbidden of [
    'p10_sp04_restored_runtime:',
    'name: P10-SP04 restored exact-application runtime',
    'bash scripts/r4-p10-sp04-restored-runtime.sh',
  ]) {
    if (snapshot.workflow.includes(forbidden)) errors.push('Exact-Head must not hard-code P10-SP04 package job: ' + forbidden)
  }

  return errors
}

function main() {
  const errors = validateP10Sp04Snapshot(collectP10Sp04Snapshot())
  if (errors.length > 0) {
    console.error('R4-P10-SP04 restored runtime gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P10-SP04 restored runtime gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
