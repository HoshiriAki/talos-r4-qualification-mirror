#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  metricsRoute: 'backend/src/routes/metrics.rs',
  router: 'backend/src/routes/mod.rs',
  compose: 'deploy/compose.production.yml',
  prometheus: 'prometheus/prometheus.yml',
  pluginQualification: 'backend/src/application/plugin_runtime_services_pg_qualification_tests.rs',
  main: 'backend/src/main.rs',
  sp01Compose: 'scripts/check-r4-p9-sp01-compose-config.mjs',
  sp02Runner: 'scripts/r4-p9-sp02-clean-stack.sh',
  sp03Runner: 'scripts/r4-p9-sp03-deployed-auth.sh',
  sp04Runner: 'scripts/r4-p9-sp04-business-evidence.sh',
  sp05Compose: 'deploy/compose.p9-sp05.yml',
  runner: 'scripts/r4-p9-sp05-observability-plugin-network.sh',
  workflow: '.github/workflows/exact-head-qualification.yml',
  milestoneWorkflow: '.github/workflows/milestone-qualification.yml',
  evidence: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p9-sp05-observability-plugin-network.md',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP9Sp05Snapshot() {
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

export function validateP9Sp05Snapshot(snapshot) {
  const errors = []

  requireTokens(errors, snapshot.metricsRoute, 'Metrics scrape authority', [
    'METRICS_SCRAPE_TOKEN_FILE',
    'METRICS_SCRAPE_PEER',
    'ConnectInfo<SocketAddr>',
    'ConstantTimeEq',
    'token.len() != 64',
    'peer != expected_peer',
    'candidate.as_bytes().ct_eq(expected_token)',
    'PlatformCapability::PlatformOperationsManage',
  ])

  requireTokens(errors, snapshot.router, 'Metrics scrape composition', [
    'MetricsScrapeAuthority::from_env()',
    '.layer(Extension(metrics_scrape_authority))',
  ])

  requireTokens(errors, snapshot.compose, 'Production metrics composition', [
    'METRICS_SCRAPE_TOKEN_FILE: /run/secrets/talos_metrics_scrape_token',
    'METRICS_SCRAPE_PEER: "172.29.0.40"',
    'talos_metrics_scrape_token',
    'file: ${METRICS_SCRAPE_TOKEN_FILE:?METRICS_SCRAPE_TOKEN_FILE is required}',
    'ipv4_address: 172.29.0.40',
  ])
  if (snapshot.compose.includes('METRICS_SCRAPE_TOKEN:')) {
    errors.push('Production Compose must not inline the metrics scrape credential')
  }

  requireTokens(errors, snapshot.prometheus, 'Prometheus scrape auth', [
    "job_name: 'talos-backend'",
    "targets: ['app:8080']",
    "metrics_path: '/metrics'",
    'authorization:',
    'type: Bearer',
    'credentials_file: /run/secrets/talos_metrics_scrape_token',
  ])
  if (/^\s*credentials:\s*\S+/m.test(snapshot.prometheus)) {
    errors.push('Prometheus must read scrape credentials from file, not inline YAML')
  }

  requireTokens(errors, snapshot.pluginQualification, 'Qualification-only plugin runtime', [
    'p9.qualification.publisher',
    'p9.sp05.qualification.key',
    'PluginRuntimeServices::postgres',
    'DestinationPolicy::public_only()',
    'destination_host: "127.0.0.1"',
    'TransportErrorClass::EgressDenied',
    '!failure.may_have_dispatched',
    'P9_SP05_PLUGIN_FIXTURE',
  ])
  if (snapshot.pluginQualification.includes('talos.release.root')) {
    errors.push('SP05 qualification plugin must not reuse a production-looking release trust root')
  }

  requireTokens(errors, snapshot.main, 'Default executable plugin boundary', [
    'starts with plugin runtime disabled',
    'ApplicationServices::production(',
  ])
  for (const forbidden of ['p9.qualification.publisher', 'p9.sp05.qualification.key']) {
    if (snapshot.main.includes(forbidden)) {
      errors.push('Default executable must not contain SP05 qualification trust: ' + forbidden)
    }
  }

  requireTokens(errors, snapshot.sp01Compose, 'SP01 compatibility harness', [
    "METRICS_SCRAPE_TOKEN_FILE: '/dev/null'",
  ])
  for (const [label, source] of [
    ['SP02', snapshot.sp02Runner],
    ['SP03', snapshot.sp03Runner],
    ['SP04', snapshot.sp04Runner],
  ]) {
    requireTokens(errors, source, label + ' compatibility harness', [
      'export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"',
      'test "$(stat -c \'%a\' "$RUNTIME_DIR")" = "700"',
      'openssl rand -hex 32 > "$METRICS_SCRAPE_TOKEN_FILE"',
      'chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"',
    ])
  }

  requireTokens(errors, snapshot.sp05Compose, 'SP05 qualification bootstrap override', [
    'AUTH_BOOTSTRAP_ON_START: "true"',
    'AUTH_BOOTSTRAP_ADMIN_USERNAME: ${P9_SP05_BOOTSTRAP_USERNAME:?P9_SP05_BOOTSTRAP_USERNAME is required}',
    'AUTH_BOOTSTRAP_ADMIN_PASSWORD: ${P9_SP05_BOOTSTRAP_PASSWORD:?P9_SP05_BOOTSTRAP_PASSWORD is required}',
  ])

  requireTokens(errors, snapshot.runner, 'SP05 live observability runner', [
    'export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"',
    'test "$(stat -c \'%a\' "$RUNTIME_DIR")" = "700"',
    'chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"',
    '-f deploy/compose.p9-sp05.yml',
    'P9_SP05_BOOTSTRAP_USERNAME',
    'P9_SP05_BOOTSTRAP_PASSWORD',
    '/auth/platform/login',
    '/api/tenant-governance/tenants',
    'test "$governance_status" = "200"',
    '"${compose[@]}" up -d --no-build db app nginx prometheus',
    'query=up{job="talos-backend"}',
    'query=talos_uptime_seconds',
    'query=count(talos_http_requests_total)',
    'query=sum(talos_registry_commands_total{phase="attempt",module="tenant_governance"})',
    'tenant_governance_attempt_observed',
    'test "$public_metrics_status" = "401"',
    'metrics_scrape_peer=172.29.0.40',
    'production_plugin_runtime=disabled',
    'P9_SP05_OBSERVABILITY',
  ])
  const registryMetricQuery =
    'query=sum(talos_registry_commands_total{phase="attempt",module="tenant_governance"})'
  const registryMetricQueryCount = snapshot.runner.split(registryMetricQuery).length - 1
  if (registryMetricQueryCount !== 2) {
    errors.push(
      'SP05 runner must execute ' +
        registryMetricQuery +
        ' exactly twice (poll + final confirmation); found ' +
        registryMetricQueryCount,
    )
  }

  for (const forbidden of [
    '$EVIDENCE_DIR/metrics-scrape-token',
    'SCRAPE_TOKEN=' + '"$(cat "$METRICS_SCRAPE_TOKEN_FILE")" > "$EVIDENCE_DIR',
  ]) {
    if (snapshot.runner.includes(forbidden)) {
      errors.push('SP05 evidence must not persist the scrape credential: ' + forbidden)
    }
  }

  const qualificationCall =
    'cargo test --features postgres --bin talos-backend p9_sp05_qualification_runtime_admits_fixture_and_rejects_loopback_egress --locked -- --ignored --nocapture'
  if (!snapshot.workflow.includes(qualificationCall)) {
    errors.push('Exact-Head must execute the SP05 qualification-only plugin runtime test')
  }

  requireTokens(errors, snapshot.milestoneWorkflow, 'Milestone SP05 job', [
    'p9_observability_plugin_network:',
    'name: P9-SP05 metrics, plugin fixture and network negative path',
    'needs: pin',
    'bash scripts/r4-p9-sp05-observability-plugin-network.sh',
    'p9-sp05-observability-plugin-network-${{ github.run_id }}-${{ github.run_attempt }}',
    '.talos-evidence/p9-sp05',
  ])

  const testCall = 'node scripts/check-r4-p9-sp05-observability-plugin-network.test.mjs'
  const gateCall = 'node scripts/check-r4-p9-sp05-observability-plugin-network.mjs'
  const testCount = snapshot.workflow.split(testCall).length - 1
  const gateCount = snapshot.workflow.split(gateCall).length - 1
  if (testCount !== 1 || gateCount !== 1) {
    errors.push(
      'Exact-Head must run SP05 mutation and structural gates once; found test=' +
      testCount + ', gate=' + gateCount,
    )
  }

  requireTokens(errors, snapshot.evidence, 'SP05 evidence contract', [
    'P9_OBSERVABILITY_PLUGIN_NETWORK_PASS',
    'TCP peer is exactly `172.29.0.40`',
    'token comparison uses `subtle::ConstantTimeEq`',
    'default executable remains unchanged',
    'p9.qualification.publisher',
    'EgressDenied',
    'may_have_dispatched=false',
    'same token routed through public nginx is rejected',
    'read-only deployed Registry command',
    'tenant_governance',
    'zero-valued Registry series',
    'CLEAN_ENV_DEPLOYMENT_PASS',
  ])

  return errors
}

function main() {
  const errors = validateP9Sp05Snapshot(collectP9Sp05Snapshot())
  if (errors.length > 0) {
    console.error('R4-P9-SP05 observability/plugin/network gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P9-SP05 observability/plugin/network gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
