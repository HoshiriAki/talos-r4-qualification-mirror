#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP9Sp05Snapshot,
  validateP9Sp05Snapshot,
} from './check-r4-p9-sp05-observability-plugin-network.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP9Sp05Snapshot())
  mutate(snapshot)
  const errors = validateP9Sp05Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP9Sp05Snapshot(collectP9Sp05Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'metrics token comparison becomes ordinary equality',
  snapshot => {
    snapshot.metricsRoute = snapshot.metricsRoute.replace(
      'candidate.as_bytes().ct_eq(expected_token)',
      'candidate.as_bytes() == expected_token',
    )
  },
  'candidate.as_bytes().ct_eq(expected_token)',
)

expectFailure(
  'evidence contract stops naming the exact Prometheus peer',
  snapshot => {
    snapshot.evidence = snapshot.evidence.replace(
      'TCP peer is exactly `172.29.0.40`',
      'TCP peer is on the backend network',
    )
  },
  'TCP peer is exactly',
)

expectFailure(
  'evidence contract stops requiring constant-time token comparison',
  snapshot => {
    snapshot.evidence = snapshot.evidence.replace(
      'token comparison uses `subtle::ConstantTimeEq`',
      'token comparison is secure',
    )
  },
  'ConstantTimeEq',
)

expectFailure(
  'metrics scrape token is no longer bound to the exact peer',
  snapshot => {
    snapshot.metricsRoute = snapshot.metricsRoute.replace(
      'if peer != expected_peer {',
      'if false {',
    )
  },
  'peer != expected_peer',
)

expectFailure(
  'Prometheus is rebound onto the public nginx peer address',
  snapshot => {
    snapshot.compose = snapshot.compose.replace(
      'METRICS_SCRAPE_PEER: "172.29.0.40"',
      'METRICS_SCRAPE_PEER: "172.29.0.30"',
    )
  },
  'METRICS_SCRAPE_PEER',
)

expectFailure(
  'Prometheus scrape credential is inlined in YAML',
  snapshot => {
    snapshot.prometheus = snapshot.prometheus.replace(
      'credentials_file: /run/secrets/talos_metrics_scrape_token',
      'credentials: qualification-secret',
    )
  },
  'credentials_file',
)

expectFailure(
  'qualification plugin reuses a production-looking release trust root',
  snapshot => {
    snapshot.pluginQualification = snapshot.pluginQualification.replaceAll(
      'p9.sp05.qualification.key',
      'talos.release.root',
    )
  },
  'must not reuse a production-looking release trust root',
)

expectFailure(
  'plugin network negative path stops targeting loopback',
  snapshot => {
    snapshot.pluginQualification = snapshot.pluginQualification.replace(
      'destination_host: "127.0.0.1"',
      'destination_host: "8.8.8.8"',
    )
  },
  'destination_host: "127.0.0.1"',
)

expectFailure(
  'plugin egress denial may have dispatched',
  snapshot => {
    snapshot.pluginQualification = snapshot.pluginQualification.replace(
      '!failure.may_have_dispatched',
      'failure.may_have_dispatched',
    )
  },
  '!failure.may_have_dispatched',
)

expectFailure(
  'public nginx accepts the scrape token',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$public_metrics_status" = "401"',
      'test "$public_metrics_status" = "200"',
    )
  },
  'test "$public_metrics_status" = "401"',
)

expectFailure(
  'SP05 secret source becomes unreadable to non-root service users',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"',
      'chmod 600 "$METRICS_SCRAPE_TOKEN_FILE"',
    )
  },
  'chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"',
)

expectFailure(
  'compatibility runner loses private runtime-directory guard',
  snapshot => {
    snapshot.sp02Runner = snapshot.sp02Runner.replace(
      'test "$(stat -c \'%a\' "$RUNTIME_DIR")" = "700"',
      'true',
    )
  },
  'stat -c',
)

expectFailure(
  'SP05 qualification bootstrap override is disabled',
  snapshot => {
    snapshot.sp05Compose = snapshot.sp05Compose.replace(
      'AUTH_BOOTSTRAP_ON_START: "true"',
      'AUTH_BOOTSTRAP_ON_START: "false"',
    )
  },
  'AUTH_BOOTSTRAP_ON_START: "true"',
)

expectFailure(
  'SP05 no longer drives a deployed Registry command',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '/api/tenant-governance/tenants',
      '/ready',
    )
  },
  '/api/tenant-governance/tenants',
)

expectFailure(
  'SP05 Registry metric proof is weakened back to generic series counting',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'query=sum(talos_registry_commands_total{phase="attempt",module="tenant_governance"})',
      'query=count(talos_registry_commands_total)',
    )
  },
  'query=sum(talos_registry_commands_total{phase="attempt",module="tenant_governance"})',
)

expectFailure(
  'qualification-only plugin runtime test is removed from milestone qualification',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'cargo test --features postgres --bin talos-backend p9_sp05_qualification_runtime_admits_fixture_and_rejects_loopback_egress --locked -- --ignored --nocapture',
      'echo removed-p9-sp05-plugin-runtime-test',
    )
  },
  'qualification-only plugin runtime test',
)

expectFailure(
  'SP05 live job is removed',
  snapshot => {
    snapshot.milestoneWorkflow = snapshot.milestoneWorkflow.replace(
      'p9_observability_plugin_network:',
      'removed_sp05_job:',
    )
  },
  'p9_observability_plugin_network:',
)

expectFailure(
  'SP05 structural gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'node scripts/check-r4-p9-sp05-observability-plugin-network.mjs',
      'node scripts/REMOVED-r4-p9-sp05-observability-plugin-network.mjs',
    )
  },
  'must run SP05 mutation and structural gates once',
)

console.log('R4-P9-SP05 observability/plugin/network mutation tests passed.')
