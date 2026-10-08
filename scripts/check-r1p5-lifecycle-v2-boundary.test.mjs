#!/usr/bin/env node

import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { spawnSync } from 'node:child_process'

const root = process.cwd()
const checker = path.join(root, 'scripts/check-r1p5-lifecycle-v2-boundary.mjs')
const files = [
  'backend/src/db/migrations/056_order_lifecycle_v2.sql',
  'backend/src/db/migrations/postgres/056_order_lifecycle_v2.sql',
  'backend/src/db/migrations.rs',
  'backend/src/db/migrations_pg.rs',
  'backend/src/repositories/lifecycle.rs',
  'backend/src/repositories/contracts/provider.rs',
  'backend/src/application/order_lifecycle_v2.rs',
  'backend/src/application/order_lifecycle_compatibility.rs',
  'backend/src/application/order_query_v2.rs',
  'backend/src/routes/order_lifecycle_v2.rs',
  'backend/src/routes/orders.rs',
  'backend/official/order/src/state_machine.rs',
  'frontend/src/api/orders.ts',
  'frontend/src/components/order/OrderActions.vue',
  'backend/src/application/mod.rs',
  'backend/src/application/lifecycle_tests.rs',
  'backend/src/registry/descriptors.rs',
  'backend/src/registry/factory.rs',
  'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs',
  'package.json',
]

function run(fixtureRoot = root) {
  return spawnSync(process.execPath, [checker], {
    cwd: root,
    env: { ...process.env, TALOS_R1P5_ROOT: fixtureRoot },
    encoding: 'utf8',
  })
}

function fixture() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'talos-r1p5-'))
  for (const rel of files) {
    const from = path.join(root, rel)
    const to = path.join(temp, rel)
    fs.mkdirSync(path.dirname(to), { recursive: true })
    fs.copyFileSync(from, to)
  }
  return temp
}

function expectFailure(name, mutate, expectedText) {
  const temp = fixture()
  try {
    mutate(temp)
    const result = run(temp)
    if (result.status === 0 || !`${result.stdout}${result.stderr}`.includes(expectedText)) {
      throw new Error(`${name} did not fail closed as expected\n${result.stdout}\n${result.stderr}`)
    }
  } finally {
    fs.rmSync(temp, { recursive: true, force: true })
  }
}

const baseline = run()
if (baseline.status !== 0) {
  throw new Error(`baseline TALOS-OPS-031 failed\n${baseline.stdout}\n${baseline.stderr}`)
}

expectFailure(
  'missing optimistic version evidence',
  (temp) => {
    const file = path.join(temp, 'backend/src/repositories/lifecycle.rs')
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('expected_version', 'removed_version_token'))
  },
  'expected_version',
)

expectFailure(
  'legacy transition wrapper regression',
  (temp) => {
    const file = path.join(temp, 'backend/src/registry/factory.rs')
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('(OrderLifecycleCompatibilityModule, Order, "order")', '(FeatureOrder, Order, "order")'))
  },
  'OrderLifecycleCompatibilityModule',
)

expectFailure(
  'missing PostgreSQL parity',
  (temp) => {
    const file = path.join(temp, 'backend/src/db/migrations_pg.rs')
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('056_order_lifecycle_v2', '056_removed_lifecycle'))
  },
  '056_order_lifecycle_v2',
)

const crlf = fixture()
try {
  for (const rel of files) {
    const file = path.join(crlf, rel)
    const text = fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n')
    fs.writeFileSync(file, text.replaceAll('\n', '\r\n'))
  }
  const result = run(crlf)
  if (result.status !== 0) {
    throw new Error(`CRLF parity failed\n${result.stdout}\n${result.stderr}`)
  }
} finally {
  fs.rmSync(crlf, { recursive: true, force: true })
}

process.stdout.write('TALOS-OPS-031 negative fixtures and CRLF parity passed.\n')
