#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkR1P1CustomerTypedIdsBoundary } from './check-r1p1-customer-typed-ids-boundary.mjs'

const paths = [
  'backend/src/domain/customer.rs',
  'backend/src/db/migrations/053_customer_domain.sql',
  'backend/src/db/migrations.rs',
  'backend/src/db/migrations/postgres/053_customer_domain.sql',
  'backend/src/db/migrations_pg.rs',
  'backend/src/repositories/customer.rs',
  'backend/src/repositories/contracts/provider.rs',
  'backend/src/application/customer.rs',
  'backend/src/application/customer_tests.rs',
  'backend/src/routes/customers.rs',
  'backend/src/routes/mod.rs',
  'backend/src/registry/descriptors.rs',
  'backend/src/registry/factory.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/README.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p1-customer-typed-ids.md',
]

function normalize(source) {
  return source.replaceAll('\r\n', '\n')
}

const baseline = Object.fromEntries(
  paths.map((path) => [path, normalize(readFileSync(join(process.cwd(), path), 'utf8'))]),
)
const crlfBaseline = Object.fromEntries(
  Object.entries(baseline).map(([path, source]) => [path, source.replaceAll('\n', '\r\n')]),
)

assert.deepEqual(checkR1P1CustomerTypedIdsBoundary(baseline), [])
assert.deepEqual(checkR1P1CustomerTypedIdsBoundary(crlfBaseline), [])

const cases = [
  {
    name: 'requires CustomerId UUID parsing',
    path: 'backend/src/domain/customer.rs',
    mutate: (source) => source.replaceAll('Uuid::parse_str', 'String::from'),
  },
  {
    name: 'rejects automatic legacy Customer synthesis',
    path: 'backend/src/db/migrations/053_customer_domain.sql',
    mutate: (source) => `${source}\nINSERT INTO customers (id) SELECT customer_phone FROM credit_scores;\n`,
  },
  {
    name: 'requires SQLite migration registry ownership',
    path: 'backend/src/db/migrations.rs',
    mutate: (source) => source.replace('id: "053_customer_domain"', 'id: "053_customer_removed"'),
  },
  {
    name: 'requires PostgreSQL schema parity',
    path: 'backend/src/db/migrations/postgres/053_customer_domain.sql',
    mutate: (source) => source.replaceAll('customer_migration_exceptions', 'migration_exceptions_removed'),
  },
  {
    name: 'rejects raw pool ownership in Customer repository',
    path: 'backend/src/repositories/customer.rs',
    mutate: (source) => `${source}\n// forbidden fixture: Pool<SqliteConnectionManager>\n`,
  },
  {
    name: 'rejects raw contact values in normal projection',
    path: 'backend/src/repositories/customer.rs',
    mutate: (source) => source.replace('pub masked_value: String,', 'pub masked_value: String,\n    pub raw_value: String,'),
  },
  {
    name: 'requires hard-coded migration source ownership',
    path: 'backend/src/repositories/customer.rs',
    mutate: (source) => source.replace('"contracts" =>', '"contracts_removed" =>'),
  },
  {
    name: 'requires Simulation fail-closed metadata',
    path: 'backend/src/application/customer.rs',
    mutate: (source) => source.replaceAll('SimulationSupport::Blocked', 'SimulationSupport::Supported'),
  },
  {
    name: 'requires trusted tenant route context',
    path: 'backend/src/routes/customers.rs',
    mutate: (source) => source.replaceAll('TrustedTenantUser', 'TenantUser'),
  },
  {
    name: 'forbids route-layer SQL',
    path: 'backend/src/routes/customers.rs',
    mutate: (source) => `${source}\n// SELECT * FROM customers\n`,
  },
  {
    name: 'requires Customer descriptor registration independent of storage backend',
    path: 'backend/src/registry/descriptors.rs',
    mutate: (source) => {
      const anchor = `    descriptor!(
        "customer",
        Business,
        ModuleActivation::Always,
        NONE,
        Customer
    ),`
      assert.ok(source.includes(anchor), 'Customer descriptor mutation anchor missing')
      return source.replace(anchor, anchor.replace('"customer"', '"customer_removed"'))
    },
  },
  {
    name: 'requires Customer ModuleFactory identity',
    path: 'backend/src/registry/factory.rs',
    mutate: (source) => source.replace('(CustomerModule, Customer, "customer")', '(CustomerModule, Customer, "customer_removed")'),
  },
  {
    name: 'requires tenant isolation executable coverage',
    path: 'backend/src/application/customer_tests.rs',
    mutate: (source) => source.replace('customer_is_tenant_scoped_and_pii_is_masked', 'customer_scope_test_removed'),
  },
  {
    name: 'requires explicit anti-inference governance record',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p1-customer-typed-ids.md',
    mutate: (source) => source.replaceAll('LEGACY_CUSTOMER_INFERENCE_FORBIDDEN', 'LEGACY_INFERENCE_RULE_REMOVED'),
  },
  {
    name: 'rejects false R1 completion',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p1-customer-typed-ids.md',
    mutate: (source) => `${source}\nstatus: R1_COMPLETE\n`,
  },
  {
    name: 'requires package Gate registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:customer-typed-ids', 'quality:talos-ops:customer-gate-removed'),
  },
]

for (const testCase of cases) {
  const files = {
    ...baseline,
    [testCase.path]: testCase.mutate(baseline[testCase.path]),
  }
  assert.ok(
    checkR1P1CustomerTypedIdsBoundary(files).length > 0,
    `${testCase.name}: expected fail-closed finding`,
  )
}

console.log(`R1-P1 Customer / Typed IDs fixtures passed: ${cases.length} negative + CRLF parity`)
