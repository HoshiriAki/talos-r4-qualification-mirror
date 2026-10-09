#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP10Sp04Snapshot,
  validateP10Sp04Snapshot,
} from './check-r4-p10-sp04-restored-runtime.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP10Sp04Snapshot())
  mutate(snapshot)
  const errors = validateP10Sp04Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(errors.some(error => error.includes(needle)), name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors))
}

const baseline = validateP10Sp04Snapshot(collectP10Sp04Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('contract permits source database during restored runtime', snapshot => {
  snapshot.contract.restoredRuntime.sourceDatabaseDisconnectedBeforeRuntime = false
}, 'restored runtime authority')

expectFailure('contract loses exact source tree', snapshot => {
  snapshot.contract.sourceArtifact.exactSourceTree = false
}, 'exact source artifact')

expectFailure('qualification substitutes transport source SHA', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"',
    'P10_SOURCE_SHA="$(git rev-parse HEAD)"',
  )
}, 'P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"')

expectFailure('qualification stops hashing repository migrations', snapshot => {
  snapshot.runner = snapshot.runner.replace('sha256sum "$file"', 'printf %s "$file"')
}, 'sha256sum "$file"')

expectFailure('source database is not disconnected', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'docker network disconnect "$DB_NETWORK" "$DB_CONTAINER"',
    'echo source-db-still-attached',
  )
}, 'docker network disconnect "$DB_NETWORK" "$DB_CONTAINER"')

expectFailure('restored authority loses production db alias', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'docker network connect --alias db --ip 172.29.0.10 "$DB_NETWORK" "$RESTORE_CONTAINER"',
    'docker network connect "$DB_NETWORK" "$RESTORE_CONTAINER"',
  )
}, 'docker network connect --alias db --ip 172.29.0.10 "$DB_NETWORK" "$RESTORE_CONTAINER"')

expectFailure('restored runtime starts source db dependency', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    '"${compose[@]}" up -d --no-deps --force-recreate app nginx',
    '"${compose[@]}" up -d --force-recreate db app nginx',
  )
}, 'restored runtime startup')

expectFailure('restored database URL uses source password', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${RESTORE_PASSWORD}@db:5432/talos"',
    'export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"',
  )
}, 'RESTORE_PASSWORD')

expectFailure('restored identity continuity proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$RESTORED_IDENTITY_ID" = "$SOURCE_IDENTITY_ID"',
    'echo identity-not-compared',
  )
}, 'test "$RESTORED_IDENTITY_ID" = "$SOURCE_IDENTITY_ID"')

expectFailure('restored machine credential authority uses a retired table', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'FROM api_clients WHERE tenant_id=',
    'FROM removed_machine_clients WHERE tenant_id=',
  )
}, 'FROM api_clients WHERE tenant_id=')

expectFailure('restored machine write disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'runtime_write_status="$(curl',
    'write_disabled="$(curl',
  )
}, 'runtime_write_status="$(curl')

expectFailure('source correlation absence proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$SOURCE_RUNTIME_CORRELATION_COUNT" = "0"',
    'echo source-correlation-not-checked',
  )
}, 'test "$SOURCE_RUNTIME_CORRELATION_COUNT" = "0"')

expectFailure('Integration recovery state is no longer governed', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test "$RESTORED_OPERATION_AFTER_RUNTIME" = "ready|0"',
    'echo integration-state-not-checked',
  )
}, 'test "$RESTORED_OPERATION_AFTER_RUNTIME" = "ready|0"')

expectFailure('SQLite exclusion disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'test -z "$RESTORED_SQLITE_FILES"',
    'echo sqlite-files-not-checked',
  )
}, 'test -z "$RESTORED_SQLITE_FILES"')

expectFailure('final cleanup secret scan disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace('if ! scan_retained_evidence; then', 'if false; then')
}, 'if ! scan_retained_evidence; then')

expectFailure('cleanup scans before final logs', snapshot => {
  const scan='if ! scan_retained_evidence; then'
  snapshot.runner = snapshot.runner.replace(scan, '# scan moved')
  snapshot.runner = snapshot.runner.replace('cleanup() {', 'cleanup() {\n  ' + scan)
}, 'collect final logs -> scan retained evidence -> remove runtime secrets')

expectFailure('SP04 calls historical SP03 runner', snapshot => {
  snapshot.runner += '\nbash scripts/r4-p10-sp03-isolated-restore.sh\n'
}, 'r4-p10-sp03-isolated-restore.sh')

expectFailure('SP04 registration disappears', snapshot => {
  snapshot.registry = snapshot.registry.replace(
    '"branch_prefix": "agent/r4-p10-sp04-"',
    '"branch_prefix": "agent/r4-p10-sp99-removed-"',
  )
}, '"branch_prefix": "agent/r4-p10-sp04-"')

expectFailure('SP04 returns to permanent Exact-Head job', snapshot => {
  snapshot.workflow += '\n  p10_sp04_restored_runtime:\n    name: P10-SP04 restored exact-application runtime\n'
}, 'must not hard-code P10-SP04 package job')

console.log('R4-P10-SP04 restored runtime mutation tests passed: 19 cases.')
