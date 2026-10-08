#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  health: 'backend/src/routes/health.rs',
  governance: 'backend/src/middleware/api_governance.rs',
  nginx: 'nginx/production.conf',
  runner: 'scripts/r4-p9-sp02-clean-stack.sh',
  workflow: '.github/workflows/exact-head-qualification.yml',
  milestoneWorkflow: '.github/workflows/milestone-qualification.yml',
  evidence: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p9-sp02-clean-stack-readiness.md',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP9Sp02Snapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) {
      errors.push(label + ' missing: ' + token)
    }
  }
}

export function validateP9Sp02Snapshot(snapshot) {
  const errors = []

  requireTokens(errors, snapshot.health, 'Readiness endpoint invariant', [
    '.route("/ready", get(readiness_check))',
    'sqlx::query_scalar::<_, i32>("SELECT 1")',
    'StatusCode::SERVICE_UNAVAILABLE',
    '"database": "postgres"',
    'state.config.is_production',
  ])
  if (snapshot.health.includes('error.to_string()')) {
    errors.push('Readiness endpoint must not expose database error details')
  }

  const unavailableCount =
    snapshot.health.split('StatusCode::SERVICE_UNAVAILABLE').length - 1
  if (unavailableCount !== 2) {
    errors.push(
      'Readiness must keep two fail-closed 503 branches: PostgreSQL probe failure and missing production PostgreSQL pool',
    )
  }

  requireTokens(errors, snapshot.governance, 'Readiness governance invariant', [
    'path == "/health" || path == "/ready"',
    '"/health" | "/ready" => Self::NoBody',
    'IngressSurface::classify("/ready")',
    'ApiResourceProfile::classify("/ready")',
  ])

  requireTokens(errors, snapshot.nginx, 'Production readiness ingress invariant', [
    'location /ready {',
    'proxy_pass http://app:8080;',
    'proxy_set_header X-Forwarded-For $remote_addr;',
    'proxy_set_header X-Forwarded-Proto https;',
  ])

  requireTokens(errors, snapshot.runner, 'Clean-stack runner invariant', [
    'docker compose -p "$PROJECT" -f deploy/compose.production.yml',
    '"${compose[@]}" pull db prometheus grafana',
    '"${compose[@]}" build --pull app nginx',
    '"${compose[@]}" up -d --no-build db app nginx',
    'https://127.0.0.1:${TALOS_HTTPS_PORT}/health',
    'https://127.0.0.1:${TALOS_HTTPS_PORT}/ready',
    'SELECT COUNT(*) FROM schema_migrations;',
    "id='083_r4_reservation_rule_sequence_invariant'",
    'test "$migration_count" = "80"',
    'expected PostgreSQL 18',
    'find /app -type f',
    'docker run --rm --network none',
    '"${negative_env[@]}"',
    '-e DB_BACKEND=sqlite',
    '-e DB_BACKEND=postgres',
    'production requires the PostgreSQL 18 database profile',
    'invalid production PostgreSQL authority did not fail closed',
    'P9_SP02_CLEAN_STACK',
    'git rev-parse HEAD',
    'image-identities.txt',
    'RUNTIME_DIR="$(mktemp -d',
    'config --images > "$EVIDENCE_DIR/compose-images.txt"',
    'down -v --remove-orphans',
  ])

  for (const forbidden of [
    '$EVIDENCE_DIR/tls',
    'compose-rendered.yml',
  ]) {
    if (snapshot.runner.includes(forbidden)) {
      errors.push('Clean-stack evidence must not persist secret-bearing qualification material: ' + forbidden)
    }
  }

  if (snapshot.runner.includes('"${compose[@]}" run --rm --no-deps')) {
    errors.push(
      'Negative deployment checks must use the built image outside the live Compose service network',
    )
  }

  requireTokens(errors, snapshot.milestoneWorkflow, 'Milestone SP02 job invariant', [
    'p9_clean_stack:',
    'name: P9-SP02 clean Linux deployment',
    'needs: pin',
    'runs-on: ubuntu-latest',
    'bash scripts/r4-p9-sp02-clean-stack.sh',
    'p9-sp02-clean-stack-${{ github.run_id }}-${{ github.run_attempt }}',
    '.talos-evidence/p9-sp02',
  ])

  const workflowLines = snapshot.milestoneWorkflow
    .split(/\r?\n/)
    .map(line => line.trim())
  if (!workflowLines.includes('p9_clean_stack:')) {
    errors.push('Milestone SP02 job key must exist as one exact top-level YAML line')
  }

  const testCall = 'node scripts/check-r4-p9-sp02-clean-stack.test.mjs'
  const gateCall = 'node scripts/check-r4-p9-sp02-clean-stack.mjs'
  const testCount = snapshot.workflow.split(testCall).length - 1
  const gateCount = snapshot.workflow.split(gateCall).length - 1
  if (testCount !== 1 || gateCount !== 1) {
    errors.push(
      'Exact-Head must run SP02 mutation and structural gates once; found test=' +
      testCount + ', gate=' + gateCount,
    )
  }

  requireTokens(errors, snapshot.evidence, 'SP02 evidence contract', [
    'P9_CLEAN_STACK_READY_PASS',
    'process liveness only',
    'production admission/readiness signal',
    'complete 80-row migration registry',
    'CLEAN_ENV_DEPLOYMENT_PASS',
  ])

  return errors
}

function main() {
  const errors = validateP9Sp02Snapshot(collectP9Sp02Snapshot())
  if (errors.length > 0) {
    console.error('R4-P9-SP02 clean-stack gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P9-SP02 clean-stack gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
