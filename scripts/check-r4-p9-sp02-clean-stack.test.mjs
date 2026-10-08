#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP9Sp02Snapshot,
  validateP9Sp02Snapshot,
} from './check-r4-p9-sp02-clean-stack.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP9Sp02Snapshot())
  mutate(snapshot)
  const errors = validateP9Sp02Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP9Sp02Snapshot(collectP9Sp02Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'readiness loses its PostgreSQL probe',
  snapshot => {
    snapshot.health = snapshot.health.replace(
      'sqlx::query_scalar::<_, i32>("SELECT 1")',
      'removed_postgres_probe',
    )
  },
  'SELECT 1',
)

expectFailure(
  'readiness stops failing closed',
  snapshot => {
    snapshot.health = snapshot.health.replace(
      'StatusCode::SERVICE_UNAVAILABLE',
      'StatusCode::OK',
    )
  },
  'two fail-closed 503 branches',
)

expectFailure(
  'production missing-pool readiness stops failing closed',
  snapshot => {
    const first = snapshot.health.indexOf('StatusCode::SERVICE_UNAVAILABLE')
    const second = snapshot.health.indexOf('StatusCode::SERVICE_UNAVAILABLE', first + 1)
    snapshot.health =
      snapshot.health.slice(0, second) +
      snapshot.health.slice(second).replace('StatusCode::SERVICE_UNAVAILABLE', 'StatusCode::OK')
  },
  'two fail-closed 503 branches',
)

expectFailure(
  'readiness is no longer a public diagnostic surface',
  snapshot => {
    snapshot.governance = snapshot.governance.replace(
      'path == "/health" || path == "/ready"',
      'path == "/health"',
    )
  },
  'Readiness governance invariant',
)

expectFailure(
  'nginx stops routing readiness to the app',
  snapshot => {
    snapshot.nginx = snapshot.nginx.replace(
      'location /ready {',
      'location /removed-ready {',
    )
  },
  'location /ready',
)

expectFailure(
  'clean stack stops building candidate images',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '"${compose[@]}" build --pull app nginx',
      'echo skip-image-build',
    )
  },
  'build --pull app nginx',
)

expectFailure(
  'qualification TLS private key is persisted in the evidence artifact',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"',
      'export TLS_KEY_FILE="$EVIDENCE_DIR/tls/tls.key"',
    )
  },
  'secret-bearing qualification material',
)

expectFailure(
  'rendered Compose with interpolated credentials is persisted as evidence',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '"${compose[@]}" config --images > "$EVIDENCE_DIR/compose-images.txt"',
      '"${compose[@]}" config > "$EVIDENCE_DIR/compose-rendered.yml"',
    )
  },
  'secret-bearing qualification material',
)

expectFailure(
  'migration registry evidence becomes partial',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$migration_count" = "80"',
      'test "$migration_count" -gt "0"',
    )
  },
  'test "$migration_count" = "80"',
)

expectFailure(
  'negative path is moved back onto the live Compose service network',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'timeout 20s docker run --rm --network none',
      'timeout 20s "${compose[@]}" run --rm --no-deps',
    )
  },
  'outside the live Compose service network',
)

expectFailure(
  'invalid SQLite production profile is no longer tested',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '-e DB_BACKEND=sqlite',
      '-e DB_BACKEND=postgres',
    )
  },
  'DB_BACKEND=sqlite',
)

expectFailure(
  'SP02 deployment job is removed',
  snapshot => {
    snapshot.milestoneWorkflow = snapshot.milestoneWorkflow.replace(
      'p9_clean_stack:',
      'removed_clean_stack_job:',
    )
  },
  'exact top-level YAML line',
)

expectFailure(
  'SP02 structural gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'node scripts/check-r4-p9-sp02-clean-stack.mjs',
      'node scripts/REMOVED-r4-p9-sp02-clean-stack.mjs',
    )
  },
  'must run SP02 mutation and structural gates once',
)

console.log('R4-P9-SP02 clean-stack mutation tests passed.')
