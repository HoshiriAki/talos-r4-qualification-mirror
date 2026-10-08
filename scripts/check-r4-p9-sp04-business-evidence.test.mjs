#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP9Sp04Snapshot,
  validateP9Sp04Snapshot,
} from './check-r4-p9-sp04-business-evidence.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP9Sp04Snapshot())
  mutate(snapshot)
  const errors = validateP9Sp04Snapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateP9Sp04Snapshot(collectP9Sp04Snapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'business write scope drifts away from device create',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      '"module":"device","command":"create_device"',
      '"module":"device","command":"list_devices"',
    )
  },
  'create_device',
)

expectFailure(
  'cross-tenant read is allowed to expose tenant A device',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      'assert data is None',
      'assert data is not None',
    )
  },
  'assert data is None',
)

expectFailure(
  'canonical device proof stops checking tenant B absence',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${DEVICE_SERIAL}'",
      "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${DEVICE_SERIAL}'",
    )
  },
  'TENANT_B_ID',
)

expectFailure(
  'canonical machine audit loses correlation binding',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      " AND correlation_id='${CORRELATION_ID}'",
      '',
    )
  },
  'same correlation id',
)

expectFailure(
  'canonical command audit no longer requires success',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "detail_json->>'result'='succeeded'",
      "detail_json->>'result'='attempted'",
    )
  },
  "detail_json->>'result'='succeeded'",
)

expectFailure(
  'canonical command audit leaves normal production execution mode',
  snapshot => {
    snapshot.runner = snapshot.runner.replace(
      "detail_json->>'execution_mode'='Normal'",
      "detail_json->>'execution_mode'='Simulation'",
    )
  },
  "detail_json->>'execution_mode'='Normal'",
)

expectFailure(
  'business runner enters preview plane',
  snapshot => {
    snapshot.runner += '\ncurl https://example.invalid/api/tenant-preview/sessions/x/devices\n'
  },
  'non-production data planes',
)

expectFailure(
  'SP04 live job is removed',
  snapshot => {
    snapshot.milestoneWorkflow = snapshot.milestoneWorkflow.replace(
      'p9_business_evidence:',
      'removed_business_evidence_job:',
    )
  },
  'p9_business_evidence:',
)

expectFailure(
  'SP04 structural gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.workflow = snapshot.workflow.replace(
      'node scripts/check-r4-p9-sp04-business-evidence.mjs',
      'node scripts/REMOVED-r4-p9-sp04-business-evidence.mjs',
    )
  },
  'must run SP04 mutation and structural gates once',
)

console.log('R4-P9-SP04 business evidence mutation tests passed.')
