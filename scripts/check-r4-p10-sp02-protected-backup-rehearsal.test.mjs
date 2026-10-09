#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP10Sp02Snapshot,
  validateP10Sp02Snapshot,
} from './check-r4-p10-sp02-protected-backup-rehearsal.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP10Sp02Snapshot())
  mutate(snapshot)
  const errors = validateP10Sp02Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP10Sp02Snapshot(collectP10Sp02Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('contract downgrades PostgreSQL source profile', snapshot => {
  snapshot.contract.sourceProfile.postgresMajor = 17
}, 'production-like PostgreSQL 18')

expectFailure('contract loses authoritative source tree identity', snapshot => {
  snapshot.contract.authoritativeIdentity.sourceTreeEnv = 'TRANSPORT_TREE'
}, 'authoritative source identity')

expectFailure('contract permits restore in SP02', snapshot => {
  snapshot.contract.claimsExcluded = snapshot.contract.claimsExcluded.filter(value => value !== 'P10_ISOLATED_RESTORE_PASS')
}, 'P10_ISOLATED_RESTORE_PASS')

expectFailure('SP02 stops using production deployment profile', snapshot => {
  snapshot.runner = snapshot.runner.replace('deploy/compose.production.yml', 'deploy/compose.dev.yml')
}, 'deploy/compose.production.yml')

expectFailure('qualification substitutes mirror transport SHA for source authority', snapshot => {
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

expectFailure('qualification stops per-file migration hashing', snapshot => {
  snapshot.runner = snapshot.runner.replace('sha256sum "$file"', 'printf %s "$file"')
}, 'sha256sum "$file"')

expectFailure('canonical machine write disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace('/api/machine/v1/tenants/', '/api/test-only/')
}, '/api/machine/v1/tenants/')

expectFailure('Integration operation admission disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace('/api/integrations/operations', '/api/removed/operations')
}, '/api/integrations/operations')

expectFailure('backup begins before quiescence', snapshot => {
  const backup = 'bash scripts/r4-p10-pg-backup.sh'
  snapshot.runner = snapshot.runner.replace(backup, 'echo backup-deferred')
  snapshot.runner = snapshot.runner.replace(
    '"${compose[@]}" stop -t 20 nginx app',
    backup + '\n"${compose[@]}" stop -t 20 nginx app',
  )
}, 'cut order')

expectFailure('dispatching-effect quiescence check disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$DISPATCHING_COUNT" = "0"',
    'echo dispatching-not-checked',
  )
}, 'test "$DISPATCHING_COUNT" = "0"')

expectFailure('SP02 reimplements encryption instead of SP01 tooling', snapshot => {
  snapshot.runner += '\ngpg --symmetric --cipher-algo AES256 fixture.dump\n'
}, '--symmetric')

expectFailure('SP01 backup dependency disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'bash scripts/r4-p10-pg-backup.sh',
    'echo backup-skipped',
  )
}, 'bash scripts/r4-p10-pg-backup.sh')

expectFailure('ciphertext stability proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$CIPHERTEXT_SHA_AFTER" = "$CIPHERTEXT_SHA_BEFORE"',
    'echo ciphertext-not-compared',
  )
}, 'test "$CIPHERTEXT_SHA_AFTER" = "$CIPHERTEXT_SHA_BEFORE"')

expectFailure('runtime secret leakage scan disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'runtime secret leaked into retained SP02 evidence',
    'secret scan removed',
  )
}, 'runtime secret leaked into retained SP02 evidence')

expectFailure('SP02 registration disappears', snapshot => {
  snapshot.registry = snapshot.registry.replace(
    '"branch_prefix": "agent/r4-p10-sp02-"',
    '"branch_prefix": "agent/r4-p10-sp99-removed-"',
  )
}, '"branch_prefix": "agent/r4-p10-sp02-"')

expectFailure('SP02 returns to a permanent Exact-Head job', snapshot => {
  snapshot.workflow += '\n  p10_sp02_protected_backup_rehearsal:\n    name: P10-SP02 production-like protected backup rehearsal\n'
}, 'must not hard-code P10-SP02 package job')

console.log('R4-P10-SP02 protected backup rehearsal mutation tests passed: 17 cases.')
