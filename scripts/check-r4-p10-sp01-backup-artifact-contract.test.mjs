#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP10Sp01Snapshot,
  validateP10Sp01Snapshot,
} from './check-r4-p10-sp01-backup-artifact-contract.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP10Sp01Snapshot())
  mutate(snapshot)
  const errors = validateP10Sp01Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP10Sp01Snapshot(collectP10Sp01Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('contract downgrades PostgreSQL', snapshot => {
  snapshot.contract.postgres.major = 17
}, 'PostgreSQL major')

expectFailure('contract loses source tree identity', snapshot => {
  snapshot.contract.metadataRequired = snapshot.contract.metadataRequired.filter(value => value !== 'source_tree')
}, 'source_tree')

expectFailure('contract permits legacy SQLite restore', snapshot => {
  snapshot.contract.legacyToolsForbidden = snapshot.contract.legacyToolsForbidden.filter(value => value !== 'scripts/dr-restore.sh')
}, 'legacy-tool denylist')

expectFailure('contract claims recovery rehearsal in SP01', snapshot => {
  snapshot.contract.claimsExcluded = snapshot.contract.claimsExcluded.filter(value => value !== 'RECOVERY_REHEARSAL_PASS')
}, 'excluded claims')

expectFailure('PG18 dump stops using custom format', snapshot => {
  snapshot.backup = snapshot.backup.replace('--format=custom', '--format=plain')
}, '--format=custom')

expectFailure('backup loses authoritative source tree metadata', snapshot => {
  snapshot.backup = snapshot.backup.replace(
    '"source_tree": os.environ["P10_META_SOURCE_TREE"]',
    '"source_tree": "transport-only"',
  )
}, '"source_tree": os.environ["P10_META_SOURCE_TREE"]')

expectFailure('backup loses repository migration manifest identity', snapshot => {
  snapshot.backup = snapshot.backup.replace(
    '"repository_manifest_sha256": os.environ["P10_META_MIGRATION_MANIFEST_SHA256"]',
    '"repository_manifest_sha256": "unchecked"',
  )
}, 'repository_manifest_sha256')

expectFailure('backup falls back to ambient GnuPG home', snapshot => {
  snapshot.backup = snapshot.backup.replace('gpg_home="$work_dir/gnupg"', 'gpg_home="$HOME/.gnupg"')
}, 'gpg_home="$work_dir/gnupg"')

expectFailure('backup encryption falls back from AES256', snapshot => {
  snapshot.backup = snapshot.backup.replace('--cipher-algo AES256', '--cipher-algo CAST5')
}, '--cipher-algo AES256')

expectFailure('backup stops immediate decrypt verification', snapshot => {
  snapshot.backup = snapshot.backup.replace('--decrypt "$protected_tmp"', '--list-only "$protected_tmp"')
}, '--decrypt "$protected_tmp"')

expectFailure('backup publishes before deleting plaintext', snapshot => {
  const marker = '# Plaintext is destroyed before the retained pair is published.'
  const remove = 'rm -f "$plaintext" "$verify_plaintext"'
  const publish = 'mv "$protected_tmp" "$protected_final"'
  const markerIndex = snapshot.backup.indexOf(marker)
  const removeIndex = snapshot.backup.indexOf(remove, markerIndex)
  assert.ok(markerIndex >= 0 && removeIndex > markerIndex)
  snapshot.backup =
    snapshot.backup.slice(0, removeIndex) +
    'echo deferred-plaintext-cleanup' +
    snapshot.backup.slice(removeIndex + remove.length)
  snapshot.backup = snapshot.backup.replace(publish, publish + '\n' + remove)
}, 'publication order')

expectFailure('verifier accepts non-SHA source identity', snapshot => {
  snapshot.verify = snapshot.verify.replace(
    're.fullmatch(r"[0-9a-f]{40}", data["source_sha"])',
    'bool(data["source_sha"])',
  )
}, 'source_sha')

expectFailure('verifier stops checking repository migration manifest', snapshot => {
  snapshot.verify = snapshot.verify.replace(
    'migration.get("repository_manifest_sha256"',
    'migration.get("removed_manifest"',
  )
}, 'repository_manifest_sha256')

expectFailure('verifier decrypts before ciphertext digest authentication', snapshot => {
  const cipherCheck = 'test "$actual_protected_sha256" = "$protected_sha256"'
  const decrypt = '--decrypt "$P10_BACKUP_ARTIFACT"'
  snapshot.verify = snapshot.verify.replace(cipherCheck, 'true')
  snapshot.verify = snapshot.verify.replace(decrypt, decrypt + '\n' + cipherCheck)
}, 'Verifier order')

expectFailure('verifier stops checking plaintext digest', snapshot => {
  snapshot.verify = snapshot.verify.replace(
    'test "$actual_plaintext_sha256" = "$plaintext_sha256"',
    'true',
  )
}, 'test "$actual_plaintext_sha256" = "$plaintext_sha256"')

expectFailure('qualification uses transport SHA as source authority', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_SHA="$(git rev-parse HEAD)"',
  )
}, 'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"')

expectFailure('qualification drops authoritative source tree', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"',
    'P10_SOURCE_TREE="$(git rev-parse HEAD^{tree})"',
  )
}, 'P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"')

expectFailure('qualification stops hashing repository migrations', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'sha256sum "$file"',
    'printf %s "$file"',
  )
}, 'sha256sum "$file"')

expectFailure('qualification delegates to SQLite legacy backup', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'bash scripts/r4-p10-pg-backup.sh',
    'bash scripts/backup.sh',
  )
}, 'scripts/r4-p10-pg-backup.sh')

expectFailure('ciphertext tamper negative path disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'tampered ciphertext unexpectedly verified',
    'ciphertext tamper ignored',
  )
}, 'tampered ciphertext unexpectedly verified')

expectFailure('wrong-key negative path disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'wrong key unexpectedly verified',
    'wrong key accepted',
  )
}, 'wrong key unexpectedly verified')

expectFailure('secret leak scan disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'database password leaked into retained evidence',
    'secret scan removed',
  )
}, 'database password leaked into retained evidence')

expectFailure('SP01 falsely claims recovery rehearsal', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'recovery_rehearsal=not_claimed',
    'recovery_rehearsal=pass',
  )
}, 'recovery_rehearsal=not_claimed')

expectFailure('workflow stops exporting authoritative package source SHA', snapshot => {
  snapshot.workflow = snapshot.workflow.replace(
    'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha',
    'TALOS_QUALIFIED_SOURCE_SHA=$actual',
  )
}, 'TALOS_QUALIFIED_SOURCE_SHA=$qualified_source_sha')

expectFailure('current-package hidden evidence upload disabled', snapshot => {
  snapshot.workflow = snapshot.workflow.replace(
    'include-hidden-files: true',
    'include-hidden-files: false',
  )
}, 'include-hidden-files: true')

expectFailure('P10 SP01 package registration removed', snapshot => {
  snapshot.registry = snapshot.registry.replace(
    '"branch_prefix": "agent/r4-p10-sp01-"',
    '"branch_prefix": "agent/r4-p10-sp99-removed-"',
  )
}, '"branch_prefix": "agent/r4-p10-sp01-"')

console.log('R4-P10-SP01 backup artifact mutation tests passed: 27 cases.')
