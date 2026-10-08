#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  runner: 'scripts/r4-p9-sp04-business-evidence.sh',
  workflow: '.github/workflows/exact-head-qualification.yml',
  milestoneWorkflow: '.github/workflows/milestone-qualification.yml',
  evidence: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p9-sp04-business-evidence.md',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP9Sp04Snapshot() {
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

export function validateP9Sp04Snapshot(snapshot) {
  const errors = []

  const createDeviceScopeToken = '"module":"device","command":"create_device"'
  const createDeviceScopeCount = snapshot.runner.split(createDeviceScopeToken).length - 1
  if (createDeviceScopeCount !== 2) {
    errors.push(
      'SP04 must bind device/create_device exactly twice: machine provisioning scope and machine execute request; found ' +
      createDeviceScopeCount,
    )
  }

  const correlationAuditToken = "correlation_id='${CORRELATION_ID}'"
  const correlationAuditCount = snapshot.runner.split(correlationAuditToken).length - 1
  if (correlationAuditCount !== 3) {
    errors.push(
      'SP04 must bind the same correlation id to machine admission, succeeded command audit, and cross-tenant audit assertions; found ' +
      correlationAuditCount,
    )
  }

  requireTokens(errors, snapshot.runner, 'SP04 runner invariant', [
    '-f deploy/compose.production.yml',
    '-f deploy/compose.p9-sp03.yml',
    'P9_SP03_BOOTSTRAP_USERNAME',
    'P9_SP03_BOOTSTRAP_PASSWORD',
    'TENANT_A_ID',
    'TENANT_B_ID',
    '"module":"device","command":"create_device"',
    '"role":"admin"',
    'CORRELATION_ID=',
    'test "$read_a_status" = "200"',
    'test "$read_b_status" = "200"',
    'assert data is None',
    "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${DEVICE_SERIAL}'",
    "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${DEVICE_SERIAL}'",
    "action='machine.access'",
    "action='device.create_device'",
    "resource_type='command'",
    "correlation_id='${CORRELATION_ID}'",
    "detail_json->>'outcome'='admitted'",
    "detail_json->>'result'='succeeded'",
    "detail_json->>'execution_mode'='Normal'",
    "detail_json #>> '{data_scope,namespace}'='Production'",
    "detail_json #>> '{payload,module}'='device'",
    "detail_json #>> '{payload,command}'='create_device'",
    'test "$machine_admission_count" = "1"',
    'test "$command_audit_count" = "1"',
    'test "$cross_tenant_audit_count" = "0"',
    'preview_simulation_execution_mode=Normal_Production',
    'P9_SP04_BUSINESS_EVIDENCE',
  ])

  for (const token of [
    '/api/tenant-preview/',
    '/api/tenant-simulations/',
    'ExecutionMode::Simulation',
    'ReadOnlyPreview',
    '$EVIDENCE_DIR/tls',
    'machine-issued.json" > "$EVIDENCE_DIR',
    'tenant-a.cookies" > "$EVIDENCE_DIR',
    'tenant-b.cookies" > "$EVIDENCE_DIR',
  ]) {
    if (snapshot.runner.includes(token)) {
      errors.push('SP04 runner must not persist secrets or select non-production data planes: ' + token)
    }
  }

  requireTokens(errors, snapshot.milestoneWorkflow, 'Milestone SP04 job invariant', [
    'p9_business_evidence:',
    'name: P9-SP04 deployed business read/write and audit evidence',
    'needs: pin',
    'bash scripts/r4-p9-sp04-business-evidence.sh',
    'p9-sp04-business-evidence-${{ github.run_id }}-${{ github.run_attempt }}',
    '.talos-evidence/p9-sp04',
  ])

  const testCall = 'node scripts/check-r4-p9-sp04-business-evidence.test.mjs'
  const gateCall = 'node scripts/check-r4-p9-sp04-business-evidence.mjs'
  const testCount = snapshot.workflow.split(testCall).length - 1
  const gateCount = snapshot.workflow.split(gateCall).length - 1
  if (testCount !== 1 || gateCount !== 1) {
    errors.push(
      'Exact-Head must run SP04 mutation and structural gates once; found test=' +
      testCount + ', gate=' + gateCount,
    )
  }

  requireTokens(errors, snapshot.evidence, 'SP04 evidence contract', [
    'P9_DEPLOYED_BUSINESS_EVIDENCE_PASS',
    'device/create_device',
    'canonical PostgreSQL',
    'audit_events',
    'zero under tenant B',
    'does not create Preview or Simulation sessions',
    'CLEAN_ENV_DEPLOYMENT_PASS',
  ])

  return errors
}

function main() {
  const errors = validateP9Sp04Snapshot(collectP9Sp04Snapshot())
  if (errors.length > 0) {
    console.error('R4-P9-SP04 business evidence gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P9-SP04 business evidence gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
