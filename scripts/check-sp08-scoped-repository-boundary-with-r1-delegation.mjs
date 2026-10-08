#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import process from 'node:process'

const node = process.execPath
const root = process.cwd()
const lifecycleAuthorityPath = 'backend/src/repositories/lifecycle.rs'

function run(script) {
  return spawnSync(node, [script], {
    cwd: root,
    encoding: 'utf8',
    env: process.env,
  })
}

function output(result) {
  return `${result.stdout ?? ''}${result.stderr ?? ''}`
}

for (const authority of [
  'scripts/check-r1p1-customer-typed-ids-boundary.mjs',
  'scripts/check-r1p2-quote-pricing-orderline-boundary.mjs',
  'scripts/check-r1p4-reservation-allocation-boundary.mjs',
  'scripts/check-r1p5-lifecycle-v2-boundary.mjs',
  'scripts/check-r1p6-durable-rental-workflow-boundary.mjs',
]) {
  const result = run(authority)
  if (result.status !== 0) {
    process.stderr.write(output(result))
    process.exit(result.status ?? 1)
  }
}

if (!existsSync(lifecycleAuthorityPath)) {
  process.stderr.write(`TALOS-OPS-031 lifecycle delegation evidence missing: ${lifecycleAuthorityPath}\n`)
  process.exit(1)
}

const sp08 = run('scripts/check-sp08-scoped-repository-boundary.mjs')
if (sp08.status === 0) {
  process.stdout.write(output(sp08))
  process.exit(0)
}

const text = output(sp08).replaceAll('\r\n', '\n').trim()
const lines = text.split('\n').map((line) => line.trim()).filter((line) => line && !line.startsWith('warning:'))
const expectedHeader = 'SP-08 scoped repository boundary check failed:'
// These are the domain repositories that the historical SP-08 checker itself classifies as
// deferred. Lifecycle V2 is validated independently and fail-closed above by TALOS-OPS-031;
// it is intentionally not fabricated as an SP-08 finding that the base checker never emits.
const delegatedFindings = new Set([
  '- TALOS-OPS-018 backend/src/repositories/customer.rs: production domain repositories are deferred beyond SP-08',
  '- TALOS-OPS-018 backend/src/repositories/quote.rs: production domain repositories are deferred beyond SP-08',
  '- TALOS-OPS-018 backend/src/repositories/reservation.rs: production domain repositories are deferred beyond SP-08',
  '- TALOS-OPS-018 backend/src/repositories/workflow.rs: production domain repositories are deferred beyond SP-08',
])
const findings = lines.filter((line) => line.startsWith('- TALOS-OPS-018 '))
const onlyDelegatedR1Findings =
  lines.includes(expectedHeader)
  && findings.length === delegatedFindings.size
  && findings.every((line) => delegatedFindings.has(line))
  && lines.every((line) => line === expectedHeader || delegatedFindings.has(line))

if (!onlyDelegatedR1Findings) {
  process.stderr.write(`${text}\n`)
  process.exit(sp08.status ?? 1)
}

process.stdout.write(
  'SP-08 scoped repository boundary passed with explicit Customer delegation to TALOS-OPS-027, Quote delegation to TALOS-OPS-028, Reservation/Allocation delegation to TALOS-OPS-030, Order Lifecycle authority at TALOS-OPS-031, and Durable Rental Workflow authority at TALOS-OPS-032.\n',
)
