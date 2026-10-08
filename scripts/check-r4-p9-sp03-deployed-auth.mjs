#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  overlay: 'deploy/compose.p9-sp03.yml',
  productionCompose: 'deploy/compose.production.yml',
  runner: 'scripts/r4-p9-sp03-deployed-auth.sh',
  workflow: '.github/workflows/exact-head-qualification.yml',
  milestoneWorkflow: '.github/workflows/milestone-qualification.yml',
  evidence: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p9-sp03-deployed-auth.md',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP9Sp03Snapshot() {
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

function between(source, start, end) {
  const startAt = source.indexOf(start)
  if (startAt < 0) return ''
  const endAt = source.indexOf(end, startAt + start.length)
  if (endAt < 0) return source.slice(startAt)
  return source.slice(startAt, endAt)
}

export function validateP9Sp03Snapshot(snapshot) {
  const errors = []

  requireTokens(errors, snapshot.overlay, 'SP03 bootstrap overlay invariant', [
    'AUTH_BOOTSTRAP_ON_START: "true"',
    'AUTH_BOOTSTRAP_ADMIN_USERNAME: ${P9_SP03_BOOTSTRAP_USERNAME:?P9_SP03_BOOTSTRAP_USERNAME is required}',
    'AUTH_BOOTSTRAP_ADMIN_PASSWORD: ${P9_SP03_BOOTSTRAP_PASSWORD:?P9_SP03_BOOTSTRAP_PASSWORD is required}',
  ])
  if (snapshot.productionCompose.includes('AUTH_BOOTSTRAP_ON_START')) {
    errors.push('Production Compose must keep authentication bootstrap disabled by omission')
  }
  const bootstrapPasswordLines = snapshot.overlay
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(line => line.startsWith('AUTH_BOOTSTRAP_ADMIN_PASSWORD:'))
  const bootstrapPasswordPattern =
    /^AUTH_BOOTSTRAP_ADMIN_PASSWORD:\s+\$\{P9_SP03_BOOTSTRAP_PASSWORD:\?P9_SP03_BOOTSTRAP_PASSWORD is required\}$/
  if (
    bootstrapPasswordLines.length !== 1 ||
    !bootstrapPasswordPattern.test(bootstrapPasswordLines[0])
  ) {
    errors.push('SP03 bootstrap password must come only from the required runtime variable')
  }

  requireTokens(errors, snapshot.runner, 'SP03 deployed-auth runner invariant', [
    'P9_SP03_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"',
    'PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"',
    'TENANT_COOKIE="$RUNTIME_DIR/tenant.cookies"',
    'MACHINE_RESPONSE="$RUNTIME_DIR/machine-issued.json"',
    'MACHINE_SECRET="${MACHINE_VALUES[2]}"',
    '/auth/platform/login',
    '/auth/login',
    '/auth/me',
    'AUTH_INVALID_CREDENTIALS',
    'Secure',
    'HttpOnly',
    'SameSite=Lax',
    'tenant_create_csrf',
    'AUTH_CSRF_REJECTED',
    'platform_cookie_on_tenant_status',
    'CORS_ALLOWED_ORIGIN="https://p9-sp03.invalid"',
    'PLATFORM_HOST="p9-sp03.invalid"',
    'PLATFORM_ORIGIN="https://${PLATFORM_HOST}"',
    'TENANT_ORIGIN="https://${TENANT_HOST}"',
    'PLATFORM_CONNECT="${PLATFORM_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"',
    'TENANT_CONNECT="${TENANT_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"',
    '--connect-to "$PLATFORM_CONNECT"',
    '--connect-to "$TENANT_CONNECT"',
    "--noproxy '*'",
    'X-Forwarded-For: $SPOOFED_IP',
    "detail_json #>> '{detail,ip}'",
    "action='auth_login'",
    'test "$AUDIT_IP" != "$SPOOFED_IP"',
    '/api/machine-clients',
    '"module":"device","command":"list_devices"',
    'Authorization: Bearer $MACHINE_SECRET',
    'Authorization: Bearer invalid-machine-secret',
    'Cookie: talos_session=browser-lane-forbidden',
    '/credentials/${MACHINE_CREDENTIAL_ID}/revoke',
    'test "$revoked_machine_status" = "401"',
    'P9_SP03_DEPLOYED_AUTH',
  ])

  if (snapshot.runner.includes("detail_json->>'ip'")) {
    errors.push('SP03 trusted-proxy evidence must read the nested audit detail IP, not a nonexistent root field')
  }

  for (const forbidden of [
    'PLATFORM_COOKIE="$EVIDENCE_DIR/',
    'TENANT_COOKIE="$EVIDENCE_DIR/',
    'MACHINE_RESPONSE="$EVIDENCE_DIR/',
    'TLS_KEY_FILE="$EVIDENCE_DIR/',
    'compose-rendered.yml',
    'echo "$P9_SP03_BOOTSTRAP_PASSWORD"',
    'echo "$MACHINE_SECRET"',
  ]) {
    if (snapshot.runner.includes(forbidden)) {
      errors.push('SP03 evidence must not persist authentication secret material: ' + forbidden)
    }
  }

  for (const forbidden of [
    '--resolve "${TENANT_HOST}:${TALOS_HTTPS_PORT}:127.0.0.1"',
    'PLATFORM_ORIGIN="https://127.0.0.1:${TALOS_HTTPS_PORT}"',
    'TENANT_ORIGIN="https://${TENANT_HOST}:${TALOS_HTTPS_PORT}"',
  ]) {
    if (snapshot.runner.includes(forbidden)) {
      errors.push('SP03 browser authority must model default public HTTPS independently of runner port mapping: ' + forbidden)
    }
  }

  const platformLoginBlock = between(
    snapshot.runner,
    'platform_login_status="$(curl',
    'IDENTITY_ID="$(python3',
  )
  requireTokens(errors, platformLoginBlock, 'Platform session-cookie invariant', [
    '--dump-header "$PLATFORM_LOGIN_HEADERS"',
    '--cookie-jar "$PLATFORM_COOKIE"',
    'grep -qi \'^set-cookie:.*Secure\' "$PLATFORM_LOGIN_HEADERS"',
    'grep -qi \'^set-cookie:.*HttpOnly\' "$PLATFORM_LOGIN_HEADERS"',
    'grep -qi \'^set-cookie:.*SameSite=Lax\' "$PLATFORM_LOGIN_HEADERS"',
  ])

  const platformCsrfBlock = between(
    snapshot.runner,
    'csrf_tenant_status="$(curl',
    'tenant_create_status="$(curl',
  )
  requireTokens(errors, platformCsrfBlock, 'Platform CSRF negative-path invariant', [
    '--cookie "$PLATFORM_COOKIE"',
    'test "$csrf_tenant_status" = "403"',
    'AUTH_CSRF_REJECTED',
  ])
  if (platformCsrfBlock.includes('-H "Origin: $PLATFORM_ORIGIN"')) {
    errors.push('Platform CSRF negative path must omit Origin so the request is rejected')
  }

  const crossHostCookieBlock = between(
    snapshot.runner,
    'platform_cookie_on_tenant_status="$(curl',
    'TENANT_COOKIE="$RUNTIME_DIR/tenant.cookies"',
  )
  requireTokens(errors, crossHostCookieBlock, 'Platform cookie cross-host invariant', [
    '--connect-to "$TENANT_CONNECT"',
    '--cookie "$PLATFORM_COOKIE"',
    'test "$platform_cookie_on_tenant_status" = "401"',
  ])

  const machineCsrfBlock = between(
    snapshot.runner,
    'machine_csrf_status="$(curl',
    'MACHINE_RESPONSE="$RUNTIME_DIR/machine-issued.json"',
  )
  requireTokens(errors, machineCsrfBlock, 'Machine provisioning CSRF negative-path invariant', [
    '--cookie "$TENANT_COOKIE"',
    'test "$machine_csrf_status" = "403"',
    'AUTH_CSRF_REJECTED',
  ])
  if (machineCsrfBlock.includes('-H "Origin: $TENANT_ORIGIN"')) {
    errors.push('Machine provisioning CSRF negative path must omit Origin so the request is rejected')
  }

  const validMachineBlock = between(
    snapshot.runner,
    'valid_machine_status="$(curl',
    'invalid_machine_status="$(curl',
  )
  if (validMachineBlock.includes('--cookie "$TENANT_COOKIE"') || validMachineBlock.includes('Cookie:')) {
    errors.push('Valid machine execution must remain bearer-only and must not send browser cookies')
  }

  const tenantLoginBlock = between(
    snapshot.runner,
    'tenant_login_status="$(curl',
    'tenant_me_status="$(curl',
  )
  requireTokens(errors, tenantLoginBlock, 'Tenant session-cookie invariant', [
    '--dump-header "$TENANT_LOGIN_HEADERS"',
    '--cookie-jar "$TENANT_COOKIE"',
    'grep -qi \'^set-cookie:.*Secure\' "$TENANT_LOGIN_HEADERS"',
    'grep -qi \'^set-cookie:.*HttpOnly\' "$TENANT_LOGIN_HEADERS"',
    'grep -qi \'^set-cookie:.*SameSite=Lax\' "$TENANT_LOGIN_HEADERS"',
    'X-Forwarded-For: $SPOOFED_IP',
  ])

  requireTokens(errors, snapshot.milestoneWorkflow, 'Milestone SP03 job invariant', [
    'p9_deployed_auth:',
    'name: P9-SP03 deployed human and machine authentication',
    'needs: pin',
    'runs-on: ubuntu-latest',
    'bash scripts/r4-p9-sp03-deployed-auth.sh',
    'p9-sp03-deployed-auth-${{ github.run_id }}-${{ github.run_attempt }}',
    '.talos-evidence/p9-sp03',
  ])

  const workflowLines = snapshot.milestoneWorkflow.split(/\r?\n/).map(line => line.trim())
  if (!workflowLines.includes('p9_deployed_auth:')) {
    errors.push('Milestone SP03 job key must exist as one exact top-level YAML line')
  }

  const testCall = 'node scripts/check-r4-p9-sp03-deployed-auth.test.mjs'
  const gateCall = 'node scripts/check-r4-p9-sp03-deployed-auth.mjs'
  const testCount = snapshot.workflow.split(testCall).length - 1
  const gateCount = snapshot.workflow.split(gateCall).length - 1
  if (testCount !== 1 || gateCount !== 1) {
    errors.push(
      'Exact-Head must run SP03 mutation and structural gates once; found test=' +
      testCount + ', gate=' + gateCount,
    )
  }

  requireTokens(errors, snapshot.evidence, 'SP03 evidence contract', [
    'P9_DEPLOYED_AUTH_PASS',
    'Qualification-only authority',
    'platform owner',
    'tenant-host session',
    'bearer-only',
    'spoofed client `X-Forwarded-For` value cannot become the audited client IP',
    'production nginx overwrites forwarding authority',
    'must not',
    'CLEAN_ENV_DEPLOYMENT_PASS',
  ])

  return errors
}

function main() {
  const errors = validateP9Sp03Snapshot(collectP9Sp03Snapshot())
  if (errors.length > 0) {
    console.error('R4-P9-SP03 deployed-auth gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P9-SP03 deployed-auth gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
