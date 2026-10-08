#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { checkR1P4ReservationAllocationBoundary } from './check-r1p4-reservation-allocation-boundary.mjs'

const paths = [
  'backend/src/domain/reservation.rs',
  'backend/src/application/reservation_v2.rs',
  'backend/src/application/reservation_tests.rs',
  'backend/src/repositories/reservation.rs',
  'backend/src/repositories/contracts/provider.rs',
  'backend/src/repositories/sqlite/session.rs',
  'backend/src/db/migrations/055_reservation_allocation_v2.sql',
  'backend/src/db/migrations/postgres/055_reservation_allocation_v2.sql',
  'backend/src/db/migrations.rs',
  'backend/src/db/migrations_pg.rs',
  'backend/src/routes/reservations_v2.rs',
  'backend/src/routes/booking.rs',
  'backend/system/admin/src/booking.rs',
  'backend/src/registry/descriptors.rs',
  'backend/src/registry/factory.rs',
  'package.json',
  'policy/qualification/legacy-evidence/docs/README.md',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p4-reservation-allocation.md',
]

function normalize(source) {
  return source.replace(/\r\n/g, '\n')
}

const baseline = Object.fromEntries(paths.map((path) => [path, normalize(readFileSync(join(process.cwd(), path), 'utf8'))]))
const crlf = Object.fromEntries(Object.entries(baseline).map(([path, source]) => [path, source.replace(/\n/g, '\r\n')]))

assert.deepEqual(checkR1P4ReservationAllocationBoundary(baseline), [])
assert.deepEqual(checkR1P4ReservationAllocationBoundary(crlf), [])

const cases = [
  {
    name: 'rejects integer canonical Reservation identity',
    path: 'backend/src/domain/reservation.rs',
    mutate: (source) => source.replace('uuid_id!(ReservationId);', 'pub struct ReservationId(i64);'),
  },
  {
    name: 'rejects canonical raw pool ownership',
    path: 'backend/src/application/reservation_v2.rs',
    mutate: (source) => `${source}\n// Pool<SqliteConnectionManager>\n`,
  },
  {
    name: 'requires BEGIN IMMEDIATE scoped writes',
    path: 'backend/src/repositories/sqlite/session.rs',
    mutate: (source) => source.replace('TransactionBehavior::Immediate', 'TransactionBehavior::Deferred'),
  },
  {
    name: 'rejects inclusive overlap regression',
    path: 'backend/src/db/migrations/055_reservation_allocation_v2.sql',
    mutate: (source) => source.replace('NEW.start_date < a.end_date', 'NEW.start_date <= a.end_date'),
  },
  {
    name: 'requires PostgreSQL migration parity',
    path: 'backend/src/db/migrations_pg.rs',
    mutate: (source) => source.replaceAll('055_reservation_allocation_v2', '055_reservation_missing'),
  },
  {
    name: 'rejects route raw pool access',
    path: 'backend/src/routes/reservations_v2.rs',
    mutate: (source) => `${source}\n// state.pool\n`,
  },
  {
    name: 'rejects legacy Booking reserve authority',
    path: 'backend/system/admin/src/booking.rs',
    mutate: (source) => source.replace('"device_search",', '"reserve",', 1),
  },
  {
    name: 'requires Booking compatibility delegation',
    path: 'backend/src/routes/booking.rs',
    mutate: (source) => source.replaceAll('"reservation_v2"', '"booking"'),
  },
  {
    name: 'requires Registry descriptor wiring',
    path: 'backend/src/registry/descriptors.rs',
    mutate: (source) => source.replace('"reservation_v2"', '"reservation_v2_missing"'),
  },
  {
    name: 'requires package aggregate registration',
    path: 'package.json',
    mutate: (source) => source.replaceAll('quality:talos-ops:reservation-allocation', 'quality:talos-ops:reservation-allocation-missing'),
  },
]

for (const testCase of cases) {
  const files = { ...baseline, [testCase.path]: testCase.mutate(baseline[testCase.path]) }
  assert.ok(checkR1P4ReservationAllocationBoundary(files).length > 0, `${testCase.name}: expected structural failure`)
}

console.log(`R1-P4 Reservation/Allocation fixtures passed: ${cases.length} negative + CRLF parity`)
