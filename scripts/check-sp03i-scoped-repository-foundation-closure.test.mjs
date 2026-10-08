#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkSp03iScopedRepositoryFoundationClosure } from './check-sp03i-scoped-repository-foundation-closure.mjs'

const paths = [
  'backend/src/repositories/contracts/binding.rs',
  'backend/src/repositories/contracts/provider.rs',
  'backend/src/repositories/sqlite/session.rs',
  'backend/src/application/services.rs',
  'backend/src/repositories/order_read.rs',
  'backend/src/application/order_queries.rs',
  'backend/src/application/order_read_compatibility.rs',
  'backend/src/routes/orders.rs',
  'scripts/check-sp08-scoped-repository-boundary.mjs',
  'scripts/check-sp03h-order-offset-caller-cutover-boundary.mjs',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03h-order-offset-caller-cutover.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
  'package.json',
  'policy/qualification/legacy-evidence/docs/README.md',
]

function normalizeNewlines(source) {
  return source.replace(/\r\n/g, '\n')
}

const baseline = Object.fromEntries(
  paths.map((path) => [path, normalizeNewlines(readFileSync(join(process.cwd(), path), 'utf8'))]),
)
const crlfBaseline = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replace(/\n/g, '\r\n')]),
)

assert.deepEqual(checkSp03iScopedRepositoryFoundationClosure(baseline), [])
assert.deepEqual(checkSp03iScopedRepositoryFoundationClosure(crlfBaseline), [])

const cases = [
  {
    name: 'fails closed when foundation evidence is absent',
    mutate: (files) => {
      const next = { ...files }
      delete next['backend/src/repositories/contracts/binding.rs']
      return next
    },
  },
  {
    name: 'requires ExecutionContext-derived repository binding',
    path: 'backend/src/repositories/contracts/binding.rs',
    mutateSource: (source) => source.replace('pub fn from_execution', 'pub fn from_raw_tenant'),
  },
  {
    name: 'requires RepositoryProvider bind authority',
    path: 'backend/src/repositories/contracts/provider.rs',
    mutateSource: (source) => source.replace('fn bind(', 'fn bind_removed('),
  },
  {
    name: 'requires SQLite query-only restoration',
    path: 'backend/src/repositories/sqlite/session.rs',
    mutateSource: (source) => source.replace('impl Drop for QueryOnlyGuard', 'impl QueryOnlyGuard'),
  },
  {
    name: 'requires TALOS-OPS-018 predecessor authority',
    path: 'scripts/check-sp08-scoped-repository-boundary.mjs',
    mutateSource: (source) => source.replace("rule: 'TALOS-OPS-018'", "rule: 'REMOVED'"),
  },
  {
    name: 'keeps the ApplicationServices provider private',
    path: 'backend/src/application/services.rs',
    mutateSource: (source) => source.replace('    repository_provider: Arc<dyn RepositoryProvider>,', '    pub repository_provider: Arc<dyn RepositoryProvider>,'),
  },
  {
    name: 'requires typed exact offset support',
    path: 'backend/src/repositories/order_read.rs',
    mutateSource: (source) => source.replace('    pub offset: Option<u64>,\n', ''),
  },
  {
    name: 'requires per-context Order query binding',
    path: 'backend/src/application/order_queries.rs',
    mutateSource: (source) => source.replace('self.repository_provider.bind(ctx)?.orders().list(request)', 'self.cached_orders.list(request)'),
  },
  {
    name: 'requires all compatibility read commands',
    path: 'backend/src/application/order_read_compatibility.rs',
    mutateSource: (source) => source.replaceAll('"list_orders_offset"', '"offset_command_removed"'),
  },
  {
    name: 'requires the migrated public offset caller',
    path: 'backend/src/routes/orders.rs',
    mutateSource: (source) => source.replace('"list_orders_offset"', '"list_orders"'),
  },
  {
    name: 'requires TALOS-OPS-025 predecessor authority',
    path: 'scripts/check-sp03h-order-offset-caller-cutover-boundary.mjs',
    mutateSource: (source) => source.replace("const RULE = 'TALOS-OPS-025'", "const RULE = 'REMOVED'"),
  },
  {
    name: 'requires finalized SP-03H verification evidence',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03h-order-offset-caller-cutover.md',
    mutateSource: (source) => source.replaceAll('EXACT_HEAD_CI_VERIFIED', 'SP03H_CI_EVIDENCE_REMOVED'),
  },
  {
    name: 'requires every deferred owner class',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
    mutateSource: (source) => source.replaceAll('R3-P5 PostgreSQL Parity', 'UNASSIGNED_POSTGRES_OWNER'),
  },
  {
    name: 'requires final exact-head verification evidence',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
    mutateSource: (source) => source.replaceAll('EXACT_HEAD_CI_VERIFIED', 'FINAL_CI_EVIDENCE_REMOVED'),
  },
  {
    name: 'requires final merge evidence',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
    mutateSource: (source) => source.replaceAll('MERGED_TO_PROTOTYPE', 'FINAL_MERGE_EVIDENCE_REMOVED'),
  },
  {
    name: 'rejects stale outage state',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
    mutateSource: (source) => `${source}\nCI_DEFERRED_EXTERNAL_ACTIONS_OUTAGE\n`,
  },
  {
    name: 'rejects false master-plan completion',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03i-scoped-repository-foundation-closure.md',
    mutateSource: (source) => `${source}\nstatus: MASTER_PLAN_SP_03_COMPLETE\n`,
  },
  {
    name: 'requires package registration',
    path: 'package.json',
    mutateSource: (source) => source.replaceAll('quality:talos-ops:repository-foundation-closure', 'quality:talos-ops:closure-unregistered'),
  },
  {
    name: 'requires documentation index registration',
    path: 'policy/qualification/legacy-evidence/docs/README.md',
    mutateSource: (source) => source.replaceAll('SP-03I Scoped Repository Foundation closure', 'SP-03I OMITTED'),
  },
  {
    name: 'requires final documentation status',
    path: 'policy/qualification/legacy-evidence/docs/README.md',
    mutateSource: (source) => source.replaceAll('已验证并合并', 'FINAL_STATUS_REMOVED'),
  },
]

for (const testCase of cases) {
  const files = testCase.mutate
    ? testCase.mutate(baseline)
    : {
        ...baseline,
        [testCase.path]: testCase.mutateSource(baseline[testCase.path]),
      }
  assert.ok(
    checkSp03iScopedRepositoryFoundationClosure(files).length > 0,
    `${testCase.name}: expected failure`,
  )
}

console.log(`SP-03I closure fixtures passed: ${cases.length} negative + CRLF parity`)
