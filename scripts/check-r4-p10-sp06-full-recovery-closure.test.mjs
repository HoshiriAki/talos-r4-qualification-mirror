#!/usr/bin/env node

import assert from 'node:assert/strict'
import { collectP10Sp06Snapshot, validateP10Sp06Snapshot } from './check-r4-p10-sp06-full-recovery-closure.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP10Sp06Snapshot())
  mutate(snapshot)
  const errors = validateP10Sp06Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(errors.some(error => error.includes(needle)), name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors))
}

const baseline = validateP10Sp06Snapshot(collectP10Sp06Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('source tree identity disappears', s => {
  s.runner = s.runner.replace(': "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"', 'echo tree-not-required')
}, 'TALOS_QUALIFIED_SOURCE_TREE_SHA')

expectFailure('private worktree dependency returns', s => {
  s.runner += '\ngit worktree add /tmp/p9 "$P9_ROLLBACK_SOURCE_SHA"\n'
}, 'clean-history')

expectFailure('protected backup disappears', s => {
  s.runner = s.runner.replace('bash scripts/r4-p10-pg-backup.sh', 'echo backup-skipped')
}, 'r4-p10-pg-backup.sh')

expectFailure('restore source-tree binding disappears', s => {
  s.runner = s.runner.replace('P10_EXPECTED_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"', 'P10_EXPECTED_SOURCE_TREE=transport')
}, 'P10_EXPECTED_SOURCE_TREE')

expectFailure('recovery timer moves before backup', s => {
  s.runner = s.runner.replace('RECOVERY_START_NS="$(date +%s%N)"', 'echo recovery-timer-removed')
  s.runner = s.runner.replace('bash scripts/r4-p10-pg-backup.sh', 'RECOVERY_START_NS="$(date +%s%N)"\nbash scripts/r4-p10-pg-backup.sh')
}, 'SP06 order must be backup')

expectFailure('controlled loss measurement disappears', s => {
  s.runner = s.runner.replace('controlled_missing_markers=1', 'controlled_missing_markers=unknown')
}, 'controlled_missing_markers=1')

expectFailure('historical SP05 runner is invoked', s => {
  s.runner += '\nbash scripts/r4-p10-sp05-rollback-forward-fix.sh\n'
}, 'single-chain boundary')

expectFailure('reconciliation actor disappears', s => {
  s.runner = s.runner.replace('TALOS_P10_RECOVERY_ACTOR_REF="p10-sp06-recovery-operator"', 'TALOS_P10_RECOVERY_ACTOR_REF=""')
}, 'p10-sp06-recovery-operator')

expectFailure('restored machine client check returns to retired table', s => {
  s.runner = s.runner.replace('FROM api_clients WHERE tenant_id=', 'FROM machine_clients WHERE tenant_id=')
}, 'FROM api_clients WHERE tenant_id=')

expectFailure('milestone loses p10 option', s => {
  s.milestone = s.milestone.replace('          - p10', '')
}, 'options:')

expectFailure('milestone drops P9 same-SHA dependency', s => {
  s.milestone = s.milestone.replace('      - p9_evidence_gate', '      - pin')
}, 'p9_evidence_gate')

expectFailure('final gate changes', s => {
  s.contract.exitGate = 'P10_PARTIAL_PASS'
}, 'final exit gate')

expectFailure('SP06 registration disappears', s => {
  s.registry = s.registry.replace('"branch_prefix": "agent/r4-p10-sp06-"', '"branch_prefix": "agent/r4-p10-sp99-"')
}, '"branch_prefix": "agent/r4-p10-sp06-"')

console.log('R4-P10-SP06 full recovery closure mutation tests passed: 13 cases.')
