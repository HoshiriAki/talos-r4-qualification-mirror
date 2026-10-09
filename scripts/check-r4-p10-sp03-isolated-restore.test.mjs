#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP10Sp03Snapshot,
  validateP10Sp03Snapshot,
} from './check-r4-p10-sp03-isolated-restore.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP10Sp03Snapshot())
  mutate(snapshot)
  const errors = validateP10Sp03Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP10Sp03Snapshot(collectP10Sp03Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('contract permits same PostgreSQL authority', snapshot => {
  snapshot.contract.restoreAuthority.differentSystemIdentifier = false
}, 'fresh isolated restore authority')

expectFailure('restore tool stops binding exact source tree', snapshot => {
  snapshot.restore = snapshot.restore.replace(
    'data.get("source_tree") == os.environ["P10_EXPECTED_SOURCE_TREE"]',
    'bool(data.get("source_tree"))',
  )
}, 'source_tree')

expectFailure('restore tool stops binding repository migration manifest', snapshot => {
  snapshot.restore = snapshot.restore.replace(
    'migration.get("repository_manifest_sha256") == os.environ["P10_EXPECTED_MIGRATION_MANIFEST_SHA256"]',
    'bool(migration.get("repository_manifest_sha256"))',
  )
}, 'repository_manifest_sha256')

expectFailure('restore tool stops verifying ciphertext identity', snapshot => {
  snapshot.restore = snapshot.restore.replace(
    'actual_protected_sha256="$(sha256sum "$P10_BACKUP_ARTIFACT" | awk \'{print $1}\')"',
    'removed_protected_sha256="$(sha256sum "$P10_BACKUP_ARTIFACT" | awk \'{print $1}\')"',
  )
}, 'actual_protected_sha256=')

expectFailure('restore tool retains plaintext', snapshot => {
  snapshot.restore = snapshot.restore.replace(
    'rm -f "$plaintext"\ntest ! -e "$plaintext"',
    'echo plaintext-retained\ntest -e "$plaintext"',
  )
}, 'rm -f "$plaintext"')

expectFailure('qualification substitutes transport source SHA', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_SHA="$(git rev-parse HEAD)"',
  )
}, 'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"')

expectFailure('qualification stops hashing repository migrations', snapshot => {
  snapshot.runner = snapshot.runner.replace('sha256sum "$file"', 'printf %s "$file"')
}, 'sha256sum "$file"')

expectFailure('restore target can equal source container', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$RESTORE_CONTAINER_ID" != "$DB_CONTAINER"',
    'echo container-identity-not-checked',
  )
}, 'test "$RESTORE_CONTAINER_ID" != "$DB_CONTAINER"')

expectFailure('restore target freshness proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$RESTORE_PUBLIC_TABLES_BEFORE" = "0"',
    'echo restore-target-not-proven-empty',
  )
}, 'test "$RESTORE_PUBLIC_TABLES_BEFORE" = "0"')

expectFailure('protected verification moves after restore', snapshot => {
  const verify = 'bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/protected-verify-before-restore.log" 2>&1'
  snapshot.runner = snapshot.runner.replace(verify, 'echo verify-deferred')
  snapshot.runner = snapshot.runner.replace(
    'bash scripts/r4-p10-pg-restore.sh > "$EVIDENCE_DIR/restore.log" 2>&1',
    'bash scripts/r4-p10-pg-restore.sh > "$EVIDENCE_DIR/restore.log" 2>&1\n' + verify,
  )
}, 'SP03 order')

expectFailure('post-backup marker may appear in restored authority', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$RESTORE_POST_MARKER_COUNT" = "0"',
    'test "$RESTORE_POST_MARKER_COUNT" = "1"',
  )
}, 'test "$RESTORE_POST_MARKER_COUNT" = "0"')

expectFailure('migration probe calls production authority only once', snapshot => {
  snapshot.migrationProbe = snapshot.migrationProbe.replace(
    'let second = crate::db::run_all_pg_migrations(&pool).await?;',
    'let second = Vec::<String>::new();',
  )
}, 'expected 2 occurrences')

expectFailure('migration probe hard-codes a historical head', snapshot => {
  snapshot.migrationProbe = snapshot.migrationProbe.replace(
    'before_latest == expected_latest',
    'before_latest == "083_r4_reservation_rule_sequence_invariant"',
  )
}, 'hard-code a historical')

expectFailure('source immutability proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$SOURCE_SNAPSHOT_AFTER" = "$SOURCE_SNAPSHOT_BEFORE"',
    'echo source-not-compared',
  )
}, 'test "$SOURCE_SNAPSHOT_AFTER" = "$SOURCE_SNAPSHOT_BEFORE"')

expectFailure('SP03 starts restored application runtime', snapshot => {
  snapshot.runner += '\n"${compose[@]}" up -d --no-build db app nginx\n'
}, 'expected 1 occurrences')

expectFailure('SP03 registration disappears', snapshot => {
  snapshot.registry = snapshot.registry.replace(
    '"branch_prefix": "agent/r4-p10-sp03-"',
    '"branch_prefix": "agent/r4-p10-sp99-removed-"',
  )
}, '"branch_prefix": "agent/r4-p10-sp03-"')

expectFailure('SP03 returns to permanent Exact-Head job', snapshot => {
  snapshot.workflow += '\n  p10_sp03_isolated_restore:\n    name: P10-SP03 fresh isolated PostgreSQL 18 restore\n'
}, 'must not hard-code P10-SP03 package job')

console.log('R4-P10-SP03 isolated restore mutation tests passed: 17 cases.')
