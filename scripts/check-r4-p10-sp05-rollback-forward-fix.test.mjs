#!/usr/bin/env node

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  collectP10Sp05Snapshot,
  validateP10Sp05Snapshot,
} from './check-r4-p10-sp05-rollback-forward-fix.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP10Sp05Snapshot())
  mutate(snapshot)
  const errors = validateP10Sp05Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP10Sp05Snapshot(collectP10Sp05Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

const compatible = spawnSync(process.execPath, [
  'scripts/r4-p10-schema-compatibility.mjs',
  '--candidate-ref', 'fixture-compatible',
  '--candidate-max', '083_r4_reservation_rule_sequence_invariant',
  '--restored-latest', '083_r4_reservation_rule_sequence_invariant',
], { encoding: 'utf8' })
assert.equal(compatible.status, 0)
assert.match(compatible.stdout, /ROLLBACK_SCHEMA_COMPATIBLE/)

const incompatible = spawnSync(process.execPath, [
  'scripts/r4-p10-schema-compatibility.mjs',
  '--candidate-ref', 'fixture-incompatible',
  '--candidate-max', '082_r4_reservation_rule_tenant_invariant',
  '--restored-latest', '083_r4_reservation_rule_sequence_invariant',
], { encoding: 'utf8' })
assert.equal(incompatible.status, 42)
assert.match(incompatible.stderr, /ROLLBACK_SCHEMA_INCOMPATIBLE/)
assert.match(incompatible.stderr, /candidate_process_started=false/)
assert.match(incompatible.stderr, /schema_downgrade_attempted=false/)

expectFailure('rollback candidate source pin changes', snapshot => {
  snapshot.contract.rollbackCandidate.sourceCommit = '0'.repeat(40)
}, 'rollback candidate authority')

expectFailure('rollback build fixture drifts', snapshot => {
  snapshot.p9DbMod += '\n// drift\n'
}, 'rollback build projection blob mismatch')

expectFailure('private Git history dependency returns', snapshot => {
  snapshot.runner += '\ngit worktree add /tmp/rollback "$P9_ROLLBACK_SOURCE_SHA"\n'
}, 'private-history boundary')

expectFailure('qualified source tree identity disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    ': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"',
    'echo source-tree-not-required',
  )
}, 'TALOS_QUALIFIED_SOURCE_TREE_SHA')

expectFailure('migration manifest stops hashing files', snapshot => {
  snapshot.runner = snapshot.runner.replace('sha256sum "$file"', 'printf %s "$file"')
}, 'sha256sum "$file"')

expectFailure('P9 max migration is no longer frozen', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'P9_ROLLBACK_MAX_MIGRATION=083_r4_reservation_rule_sequence_invariant',
    'P9_ROLLBACK_MAX_MIGRATION="$source_migration_registry_head"',
  )
}, 'P9_ROLLBACK_MAX_MIGRATION=083_r4_reservation_rule_sequence_invariant')

expectFailure('rollback image no longer uses projected context', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'docker build -t "$P9_IMAGE" "$P9_BUILD_CONTEXT"',
    'docker build -t "$P9_IMAGE" .',
  )
}, 'docker build -t "$P9_IMAGE" "$P9_BUILD_CONTEXT"')

expectFailure('drain proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace('test "$DRAIN_DISPATCHING" = 0', 'echo no-drain-check')
}, 'test "$DRAIN_DISPATCHING" = 0')

expectFailure('UnknownOutcome classification proof disappears', snapshot => {
  snapshot.runner = snapshot.runner.replaceAll(
    'unknown_outcome|1|worker_restarted_after_dispatch',
    'ready|2|blind-retry',
  )
}, 'unknown_outcome|1|worker_restarted_after_dispatch')

expectFailure('incompatible rollback no longer fails closed', snapshot => {
  snapshot.runner = snapshot.runner.replace('test "$BLOCK_STATUS" = 42', 'test "$BLOCK_STATUS" = 0')
}, 'test "$BLOCK_STATUS" = 42')

expectFailure('forward-fix exact app restart disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'compose up -d --no-deps --force-recreate app nginx',
    'echo no-forward-fix',
  )
}, 'compose up -d --no-deps --force-recreate app nginx')

expectFailure('production reconciliation probe disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace(
    'p10_unknown_outcome_requires_reconciliation_before_retry',
    'removed_reconciliation_probe',
  )
}, 'p10_unknown_outcome_requires_reconciliation_before_retry')

expectFailure('destructive schema rollback is introduced', snapshot => {
  snapshot.runner += '\npsql -c "ALTER TABLE devices DROP COLUMN notes"\n'
}, 'private-history boundary')

expectFailure('final evidence scan disappears', snapshot => {
  snapshot.runner = snapshot.runner.replace('if ! scan_evidence; then', 'if false; then')
}, 'if ! scan_evidence; then')

expectFailure('SP05 registration disappears', snapshot => {
  snapshot.registry = snapshot.registry.replace(
    '"branch_prefix": "agent/r4-p10-sp05-"',
    '"branch_prefix": "agent/r4-p10-sp99-removed-"',
  )
}, '"branch_prefix": "agent/r4-p10-sp05-"')

expectFailure('SP05 returns to permanent Exact-Head job', snapshot => {
  snapshot.workflow += '\n  p10_sp05_rollback_forward_fix:\n    run: bash scripts/r4-p10-sp05-rollback-forward-fix.sh\n'
}, 'must not hard-code P10-SP05 package job')

console.log('R4-P10-SP05 rollback/forward-fix mutation tests passed: 16 cases.')
