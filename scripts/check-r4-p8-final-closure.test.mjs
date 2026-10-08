#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8FinalClosureSnapshot,
  validateP8FinalClosureSnapshot,
} from './check-r4-p8-final-closure.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8FinalClosureSnapshot())
  mutate(snapshot)
  const errors = validateP8FinalClosureSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8FinalClosureSnapshot(collectP8FinalClosureSnapshot())
assert.deepEqual(baseline, [], `baseline must pass: ${baseline.join('; ')}`)

expectFailure(
  'performance qualification module is unregistered',
  snapshot => {
    snapshot.pgMod = snapshot.pgMod.replace(
      'mod p8f_performance_qualification_tests;',
      '// p8f performance module removed',
    )
  },
  'module is not registered',
)

expectFailure(
  'read regression guard is silently loosened',
  snapshot => {
    snapshot.performance = snapshot.performance.replace(
      'const ORDER_LIST_P95_LIMIT: Duration = Duration::from_millis(500);',
      'const ORDER_LIST_P95_LIMIT: Duration = Duration::from_millis(5_000);',
    )
  },
  'ORDER_LIST_P95_LIMIT',
)

expectFailure(
  'write regression guard is silently loosened',
  snapshot => {
    snapshot.performance = snapshot.performance.replace(
      'const IMPORTED_DRAFT_P95_LIMIT: Duration = Duration::from_millis(1_000);',
      'const IMPORTED_DRAFT_P95_LIMIT: Duration = Duration::from_millis(10_000);',
    )
  },
  'IMPORTED_DRAFT_P95_LIMIT',
)

expectFailure(
  'Exact-Head stops running the P8-F baseline',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'live_pg18_p8f_performance_baseline_bounds_order_read_and_transactional_write',
      'REMOVED_P8F_PERFORMANCE_BASELINE',
    )
  },
  'performance evidence wiring missing',
)

expectFailure(
  'P8-F performance artifact is removed',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'p8f-pg18-performance-${{ github.run_id }}-${{ github.run_attempt }}',
      'removed-p8f-performance-artifact',
    )
  },
  'performance evidence wiring missing',
)

expectFailure(
  'SQLite restore tooling is allowed to impersonate PG18 recovery',
  snapshot => {
    snapshot.handoff = snapshot.handoff.replace(
      'must **not** be used as PostgreSQL 18 production backup/restore evidence',
      'may be used as PostgreSQL 18 production backup/restore evidence',
    )
  },
  'recovery handoff invariant missing',
)

expectFailure(
  'DR plan loses the isolated PostgreSQL restore requirement',
  snapshot => {
    snapshot.drPlan = snapshot.drPlan.replace(
      'restore to fresh isolated PostgreSQL 18',
      'restore directly into production PostgreSQL 18',
    )
  },
  'DR/P10 handoff invariant missing',
)

console.log('R4-P8 final closure mutation tests passed.')
