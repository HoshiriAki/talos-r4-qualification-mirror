#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const PATHS = Object.freeze({
  contract: '.talos/ci/r4-p10-sp01-backup-artifact-contract.json',
  handoff: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p8f-performance-recovery-handoff.md',
  backup: 'scripts/r4-p10-pg-backup.sh',
  verify: 'scripts/r4-p10-verify-backup.sh',
  runner: 'scripts/r4-p10-sp01-backup-artifact-contract.sh',
  workflow: '.github/workflows/exact-head-qualification.yml',
  registry: '.talos/ci/package-qualifications.json',
})

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP10Sp01Snapshot() {
  return {
    contract: JSON.parse(read(PATHS.contract)),
    handoff: read(PATHS.handoff),
    backup: read(PATHS.backup),
    verify: read(PATHS.verify),
    runner: read(PATHS.runner),
    workflow: read(PATHS.workflow),
    registry: read(PATHS.registry),
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

export function validateP10Sp01Snapshot(snapshot) {
  const errors = []
  const contract = snapshot.contract ?? {}

  if (contract.schema !== 'talos.p10.backup-artifact-contract/v1') {
    errors.push('machine contract schema mismatch')
  }
  if (contract.scope !== 'R4-P10-SP01') errors.push('machine contract scope mismatch')
  if (contract.exitGate !== 'P10_BACKUP_ARTIFACT_CONTRACT_PASS') {
    errors.push('machine contract exit gate mismatch')
  }
  if (contract.postgres?.major !== 18) errors.push('machine contract PostgreSQL major must be 18')
  if (contract.postgres?.clientImage !== 'postgres:18-alpine@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2') {
    errors.push('machine contract PostgreSQL client image must remain digest-pinned')
  }
  exactArray(errors, contract.postgres?.dumpFlags, ['--format=custom', '--no-owner', '--no-acl'], 'machine contract dump flags')

  if (
    contract.integrity?.plaintextChecksum !== 'sha256' ||
    contract.integrity?.protectedChecksum !== 'sha256' ||
    contract.integrity?.immediateDecryptRoundTripRequired !== true ||
    contract.integrity?.pgRestoreListRequired !== true
  ) {
    errors.push('machine contract integrity policy weakened')
  }

  const encryption = contract.encryption ?? {}
  if (
    encryption.policyId !== 'gnupg-symmetric-aes256-s2k3-sha512' ||
    encryption.cipher !== 'AES256' ||
    encryption.s2kMode !== 3 ||
    encryption.s2kDigest !== 'SHA512' ||
    encryption.ephemeralPrivateHomeRequired !== true ||
    encryption.plaintextRetained !== false
  ) {
    errors.push('machine contract encryption policy weakened')
  }

  exactArray(
    errors,
    contract.legacyToolsForbidden,
    ['scripts/backup.sh', 'scripts/db-backup.js', 'scripts/dr-restore.sh'],
    'machine contract legacy-tool denylist',
  )
  exactArray(
    errors,
    contract.claimsExcluded,
    ['RECOVERY_REHEARSAL_PASS', 'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS', 'RTO_VERIFIED', 'RPO_VERIFIED'],
    'machine contract excluded claims',
  )
  for (const field of [
    'source_sha',
    'source_tree',
    'source.database',
    'source.user',
    'source.server_version_num',
    'source.pg_is_in_recovery',
    'migration.count',
    'migration.latest_id',
    'migration.repository_manifest_sha256',
    'migration.repository_head',
    'created_at',
    'dump.plaintext_bytes',
    'dump.plaintext_sha256',
    'protected_artifact.bytes',
    'protected_artifact.sha256',
    'protected_artifact.encryption_policy',
    'operator_id',
    'run_id',
  ]) {
    if (!contract.metadataRequired?.includes(field)) {
      errors.push('machine contract metadata field missing: ' + field)
    }
  }

  requireTokens(errors, snapshot.handoff, 'P8-F recovery handoff projection', [
    'scripts/backup.sh',
    'scripts/db-backup.js',
    'scripts/dr-restore.sh',
    'must **not** be used as PostgreSQL 18 production backup/restore evidence',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ])

  requireTokens(errors, snapshot.backup, 'PG18 backup tool', [
    ': "${P10_SOURCE_SHA:?P10_SOURCE_SHA is required}"',
    ': "${P10_SOURCE_TREE:?P10_SOURCE_TREE is required}"',
    ': "${P10_MIGRATION_MANIFEST_SHA256:?P10_MIGRATION_MANIFEST_SHA256 is required}"',
    ': "${P10_MIGRATION_HEAD:?P10_MIGRATION_HEAD is required}"',
    ': "${P10_OPERATOR_ID:?P10_OPERATOR_ID is required}"',
    ': "${P10_PGPASS_FILE:?P10_PGPASS_FILE is required}"',
    ': "${P10_BACKUP_KEY_FILE:?P10_BACKUP_KEY_FILE is required}"',
    ': "${P10_PG_CLIENT_IMAGE:?P10_PG_CLIENT_IMAGE is required}"',
    'gpg_home="$work_dir/gnupg"',
    'chmod 700 "$gpg_home"',
    'gpgconf --homedir "$gpg_home" --kill gpg-agent',
    'PGPASSFILE=/run/secrets/p10-pgpass',
    'pg_dump',
    '--format=custom',
    '--no-owner',
    '--no-acl',
    'sha256sum "$plaintext"',
    '--symmetric',
    '--cipher-algo AES256',
    '--s2k-mode 3',
    '--s2k-digest-algo SHA512',
    '--passphrase-file "$P10_BACKUP_KEY_FILE"',
    'sha256sum "$protected_tmp"',
    '--decrypt "$protected_tmp"',
    'pg_restore --list /backup/verified.dump',
    'talos.p10.backup-metadata/v1',
    'gnupg-symmetric-aes256-s2k3-sha512',
    '"source_tree": os.environ["P10_META_SOURCE_TREE"]',
    '"repository_manifest_sha256": os.environ["P10_META_MIGRATION_MANIFEST_SHA256"]',
    '"repository_head": os.environ["P10_META_MIGRATION_HEAD"]',
    '"operator_id": os.environ["P10_META_OPERATOR_ID"]',
    'rm -f "$plaintext" "$verify_plaintext"',
    'mv "$protected_tmp" "$protected_final"',
    'mv "$metadata_tmp" "$metadata_final"',
    'published=true',
  ])

  for (const forbidden of contract.legacyToolsForbidden ?? []) {
    if (snapshot.backup.includes(forbidden) || snapshot.runner.includes(forbidden)) {
      errors.push('PG18 P10 tooling must not invoke legacy recovery asset: ' + forbidden)
    }
  }
  for (const forbidden of ['Backup (unencrypted)', 'unencrypted fallback']) {
    if (snapshot.backup.includes(forbidden)) errors.push('PG18 backup tool exposes plaintext fallback: ' + forbidden)
  }

  const cleanupMarker = '# Plaintext is destroyed before the retained pair is published.'
  const markerIndex = snapshot.backup.indexOf(cleanupMarker)
  const cleanupIndex = markerIndex < 0 ? -1 : snapshot.backup.indexOf('rm -f "$plaintext" "$verify_plaintext"', markerIndex)
  const order = [
    snapshot.backup.indexOf('source_identity='),
    snapshot.backup.indexOf('pg_dump'),
    snapshot.backup.indexOf('plaintext_sha256='),
    snapshot.backup.indexOf('--symmetric'),
    snapshot.backup.indexOf('protected_sha256='),
    snapshot.backup.indexOf('--decrypt "$protected_tmp"'),
    snapshot.backup.indexOf('pg_restore --list /backup/verified.dump'),
    snapshot.backup.indexOf('json.dump(metadata'),
    cleanupIndex,
    snapshot.backup.indexOf('mv "$protected_tmp" "$protected_final"'),
  ]
  if (order.some(value => value < 0) || order.some((value, index) => index > 0 && value <= order[index - 1])) {
    errors.push('PG18 backup publication order must be identity -> dump -> digest -> encrypt -> digest -> decrypt -> format verify -> metadata -> plaintext removal -> publish')
  }

  requireTokens(errors, snapshot.verify, 'Protected backup verifier', [
    'talos.p10.backup-metadata/v1',
    'gnupg-symmetric-aes256-s2k3-sha512',
    're.fullmatch(r"[0-9a-f]{40}", data["source_sha"])',
    're.fullmatch(r"[0-9a-f]{40}", data["source_tree"])',
    'migration.get("repository_manifest_sha256"',
    'migration.get("repository_head")',
    'actual_protected_sha256=',
    'test "$actual_protected_sha256" = "$protected_sha256"',
    '--decrypt "$P10_BACKUP_ARTIFACT"',
    'actual_plaintext_sha256=',
    'test "$actual_plaintext_sha256" = "$plaintext_sha256"',
    'pg_restore --list /backup/verified.dump',
    'gpg_home="$work_dir/gnupg"',
    'gpgconf --homedir "$gpg_home" --kill gpg-agent',
  ])

  const cipherIndex = snapshot.verify.indexOf('test "$actual_protected_sha256" = "$protected_sha256"')
  const decryptIndex = snapshot.verify.indexOf('--decrypt "$P10_BACKUP_ARTIFACT"')
  const plaintextIndex = snapshot.verify.indexOf('test "$actual_plaintext_sha256" = "$plaintext_sha256"')
  const formatIndex = snapshot.verify.indexOf('pg_restore --list /backup/verified.dump')
  if (!(cipherIndex >= 0 && cipherIndex < decryptIndex && decryptIndex < plaintextIndex && plaintextIndex < formatIndex)) {
    errors.push('Verifier order must be ciphertext digest -> decrypt -> plaintext digest -> pg_restore format verification')
  }

  requireTokens(errors, snapshot.runner, 'SP01 live qualification', [
    ': "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"',
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'transport-sha.txt',
    'source-tree-sha.txt',
    'backend/src/db/migrations/postgres',
    'migration_manifest_sha256="$(',
    'migration-manifest.txt',
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"',
    'P10_MIGRATION_HEAD="$migration_head"',
    'P10_OPERATOR_ID="${GITHUB_ACTOR:-local-operator}"',
    'postgres:18-alpine@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2',
    'CREATE TABLE schema_migrations',
    'p10_sp01_fixture',
    'bash scripts/r4-p10-pg-backup.sh',
    'bash scripts/r4-p10-verify-backup.sh',
    'tampered ciphertext unexpectedly verified',
    "data['dump']['plaintext_sha256']='0'*64",
    'wrong key unexpectedly verified',
    'database password leaked into retained evidence',
    'backup key leaked into retained evidence',
    'qualification-status.txt',
    'recovery_rehearsal=not_claimed',
    'rto_rpo=not_measured',
    'P10_BACKUP_ARTIFACT_CONTRACT_PASS',
  ])

  for (const claim of contract.claimsExcluded ?? []) {
    if (snapshot.runner.includes('echo "' + claim + '"') || snapshot.runner.includes("echo '" + claim + "'")) {
      errors.push('SP01 runner must not claim later P10 exit evidence: ' + claim)
    }
  }

  requireTokens(errors, snapshot.workflow, 'Exact-Head package routing', [
    "'agent/r4-p*-*'",
    'Current package qualification',
    'ci-package-qualification.mjs',
    'ci-run-package-qualification.mjs',
    'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha',
    'TALOS_QUALIFIED_SOURCE_TREE_SHA=$qualified_source_tree',
  ])
  const uploadStart = snapshot.workflow.indexOf('- name: Upload current package evidence')
  const uploadEnd = snapshot.workflow.indexOf('- name: Record no package-specific qualification')
  const uploadBlock = uploadStart >= 0 && uploadEnd > uploadStart ? snapshot.workflow.slice(uploadStart, uploadEnd) : ''
  requireTokens(errors, uploadBlock, 'Exact-Head package evidence transport', [
    'path: .talos-evidence',
    'include-hidden-files: true',
    'if-no-files-found: error',
  ])
  for (const forbidden of ['p10_backup_artifact_contract:', 'name: P10-SP01 protected backup artifact contract']) {
    if (snapshot.workflow.includes(forbidden)) errors.push('Exact-Head must not hard-code P10-SP01 job: ' + forbidden)
  }

  requireTokens(errors, snapshot.registry, 'P10-SP01 package registration', [
    '"version": 2',
    '"branch_prefix": "agent/r4-p10-sp01-"',
    '"scripts/check-r4-p10-sp01-backup-artifact-contract.test.mjs"',
    '"scripts/check-r4-p10-sp01-backup-artifact-contract.mjs"',
    '"qualification_script": "scripts/r4-p10-sp01-backup-artifact-contract.sh"',
    '"label": "p10-sp01-backup-artifact"',
  ])

  return errors
}

function main() {
  const errors = validateP10Sp01Snapshot(collectP10Sp01Snapshot())
  if (errors.length > 0) {
    console.error('R4-P10-SP01 backup artifact gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P10-SP01 backup artifact gate passed.')
}

if (path.resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) main()
