#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectBookingAuthoritySnapshot,
  validateBookingAuthoritySnapshot,
} from './check-r4-p8-booking-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectBookingAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateBookingAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateBookingAuthoritySnapshot(collectBookingAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Booking factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'BookingCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureBooking)',
      'Booking factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Booking factory restores stale FeatureBooking type mapping',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      '(BookingCompatibilityModule, Booking, "booking")',
      '(BookingCompatibilityModule, Booking, "booking")\n    (FeatureBooking, Booking, "booking")',
      'Booking factory restores stale FeatureBooking type mapping',
    )
  },
  'stale FeatureBooking type mapping',
)

expectFailure(
  'Booking descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("booking", Business, ModuleActivation::Always, NONE, Booking)',
      'descriptor!("booking", Business, ModuleActivation::Always, SQLITE, Booking)',
      'Booking descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Booking compatibility reopens legacy write',
  snapshot => {
    assert.ok(
      snapshot.application.includes('BIZ_LEGACY_BOOKING_WRITE_RETIRED'),
      'Booking compatibility reopens legacy write: mutation anchor missing',
    )
    snapshot.application = snapshot.application.replaceAll(
      'BIZ_LEGACY_BOOKING_WRITE_RETIRED',
      'BIZ_LEGACY_BOOKING_WRITE_ACTIVE',
    )
  },
  'BIZ_LEGACY_BOOKING_WRITE_RETIRED',
)

expectFailure(
  'Booking PostgreSQL price loses tenant join',
  snapshot => {
    assert.ok(
      snapshot.postgres.includes('bp.tenant_id=d.tenant_id'),
      'Booking PostgreSQL price loses tenant join: mutation anchor missing',
    )
    snapshot.postgres = snapshot.postgres.replaceAll(
      'bp.tenant_id=d.tenant_id',
      'bp.tenant_id=tenant_scope_removed',
    )
  },
  'PostgreSQL Booking read invariant',
)

expectFailure(
  'Booking PostgreSQL price loses numeric normalization',
  snapshot => {
    assert.ok(
      snapshot.postgres.includes('bp.weekdayprice::double precision'),
      'Booking PostgreSQL price loses numeric normalization: mutation anchor missing',
    )
    snapshot.postgres = snapshot.postgres.replaceAll(
      'bp.weekdayprice::double precision',
      'bp.weekdayprice',
    )
  },
  'PostgreSQL Booking read invariant',
)

expectFailure(
  'Booking live proof loses cross-tenant rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'price("BOOK-B-1")?.is_none()',
      'price("BOOK-B-1")?.is_some()',
      'Booking live proof loses cross-tenant rejection',
    )
  },
  'PG18 Booking evidence missing',
)

console.log('R4-P8 Booking authority mutation tests passed.')
