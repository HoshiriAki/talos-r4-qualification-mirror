#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP9Sp03Snapshot,
  validateP9Sp03Snapshot,
} from './check-r4-p9-sp03-deployed-auth.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP9Sp03Snapshot())
  mutate(snapshot)
  const errors = validateP9Sp03Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP9Sp03Snapshot(collectP9Sp03Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'qualification bootstrap opt-in is removed',
  snapshot => {
    snapshot.overlay = snapshot.overlay.replace(
      'AUTH_BOOTSTRAP_ON_START: "true"',
      'AUTH_BOOTSTRAP_ON_START: "false"',
    )
  },
  'AUTH_BOOTSTRAP_ON_START',
)

expectFailure(
  'bootstrap leaks into the production profile',
  snapshot => {
    snapshot.productionCompose += '\n      AUTH_BOOTSTRAP_ON_START: "true"\n'
  },
  'Production Compose must keep authentication bootstrap disabled',
)

expectFailure(
  'platform login no longer proves secure cookies',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "grep -qi '^set-cookie:.*Secure' \"$PLATFORM_LOGIN_HEADERS\"",
      'echo skip-secure-cookie-proof',
    )
  },
  'Secure',
)

expectFailure(
  'tenant creation CSRF negative path stops failing closed',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$csrf_tenant_status" = "403"',
      'test "$csrf_tenant_status" = "200"',
    )
  },
  'Platform CSRF negative-path invariant',
)

expectFailure(
  'platform cookie is no longer denied on the tenant host',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$platform_cookie_on_tenant_status" = "401"',
      'test "$platform_cookie_on_tenant_status" = "200"',
    )
  },
  'Platform cookie cross-host invariant',
)

expectFailure(
  'browser authority is coupled to the runner high port',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'PLATFORM_ORIGIN="https://${PLATFORM_HOST}"',
      'PLATFORM_ORIGIN="https://127.0.0.1:${TALOS_HTTPS_PORT}"',
    )
  },
  'browser authority must model default public HTTPS',
)

expectFailure(
  'tenant host returns to curl resolve instead of default-HTTPS authority routing',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '--connect-to "$TENANT_CONNECT"',
      '--resolve "${TENANT_HOST}:${TALOS_HTTPS_PORT}:127.0.0.1"',
    )
  },
  'browser authority must model default public HTTPS',
)

expectFailure(
  'tenant traffic is allowed to leave the local qualification stack',
  snapshot => {
    snapshot.runner = snapshot.runner.replaceAll("--noproxy '*'", '')
  },
  "--noproxy '*'",
)

expectFailure(
  'trusted-proxy audit query returns to the nonexistent root ip field',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "detail_json #>> '{detail,ip}'",
      "detail_json->>'ip'",
    )
  },
  'nested audit detail IP',
)

expectFailure(
  'trusted-proxy spoof audit check is removed',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$AUDIT_IP" != "$SPOOFED_IP"',
      'echo skip-forwarded-header-audit',
    )
  },
  'AUDIT_IP',
)

expectFailure(
  'machine provisioning CSRF negative path stops failing closed',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$machine_csrf_status" = "403"',
      'test "$machine_csrf_status" = "200"',
    )
  },
  'Machine provisioning CSRF negative-path invariant',
)

expectFailure(
  'machine issue response is persisted into evidence',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'MACHINE_RESPONSE="$RUNTIME_DIR/machine-issued.json"',
      'MACHINE_RESPONSE="$EVIDENCE_DIR/machine-issued.json"',
    )
  },
  'authentication secret material',
)

expectFailure(
  'valid machine lane is contaminated with a browser cookie',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '--dump-header "$MACHINE_EXEC_HEADERS" \\',
      '--dump-header "$MACHINE_EXEC_HEADERS" \\\n  --cookie "$TENANT_COOKIE" \\',
    )
  },
  'bearer-only',
)

expectFailure(
  'machine credential revocation proof is removed',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '/credentials/${MACHINE_CREDENTIAL_ID}/revoke',
      '/credentials/${MACHINE_CREDENTIAL_ID}/removed',
    )
  },
  '/credentials/',
)

expectFailure(
  'revoked machine bearer is no longer required to fail closed',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'test "$revoked_machine_status" = "401"',
      'test "$revoked_machine_status" = "200"',
    )
  },
  'revoked_machine_status',
)

expectFailure(
  'SP03 deployment-auth job is removed',
  snapshot => {
    snapshot.milestoneWorkflow = snapshot.milestoneWorkflow.replace(
      'p9_deployed_auth:',
      'removed_deployed_auth_job:',
    )
  },
  'exact top-level YAML line',
)

expectFailure(
  'SP03 structural gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'node scripts/check-r4-p9-sp03-deployed-auth.mjs',
      'node scripts/REMOVED-r4-p9-sp03-deployed-auth.mjs',
    )
  },
  'must run SP03 mutation and structural gates once',
)

console.log('R4-P9-SP03 deployed-auth mutation tests passed.')
