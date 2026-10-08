#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/booking_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/booking_read.rs',
  dispatch: 'backend/src/repositories/booking_read_dispatch.rs',
  postgres: 'backend/src/repositories/booking_read_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/booking_read_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectBookingAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateBookingAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'BookingCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.booking_reads()',
    '.availability(',
    '.price(',
    '.search(',
    'FeatureBooking::new().metadata()',
    'FeatureBooking::new().commands()',
    'FeatureBooking::new().schema()',
    'BIZ_LEGACY_BOOKING_WRITE_RETIRED',
    'reservation_v2.create_legacy_hold',
    'reservation_v2.confirm_reservation',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Booking compatibility invariant missing: ' + token)
    }
  }

  for (const token of [
    'mod booking_read;',
    'mod booking_read_dispatch;',
    'mod booking_read_postgres;',
    'pub use booking_read_dispatch::ScopedBookingReadRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Booking repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn booking_reads(&self) -> ScopedBookingReadRepository<'_>")) {
    errors.push('ScopedRepositories must expose Booking read authority')
  }
  if (!snapshot.dispatch.includes('PostgresBookingReadRepository')
      || !snapshot.dispatch.includes('SqliteBookingReadRepository')) {
    errors.push('Booking backend dispatch is incomplete')
  }

  for (const token of [
    'WHERE tenant_id=?1',
    'bp.tenant_id=d.tenant_id',
    'sqlite_booking_reads_preserve_scope_availability_and_pricing',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Booking read invariant missing: ' + token)
    }
  }

  for (const token of [
    'd.tenant_id=$1',
    'bp.tenant_id=d.tenant_id',
    'bp.weekdayprice::double precision',
    'bp.weekendprice::double precision',
    'QueryBuilder::<Postgres>',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL Booking read invariant missing: ' + token)
    }
  }

  if (!factory.includes('BookingCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Booking provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureBooking)')) {
    errors.push('Registry Booking restored direct SQLite construction')
  }
  if (compact(snapshot.factory).includes('(FeatureBooking,Booking,"booking")')) {
    errors.push('Registry Booking retained stale FeatureBooking type mapping')
  }
  if (!descriptors.includes('descriptor!("booking",Business,ModuleActivation::Always,NONE,Booking)')) {
    errors.push('Booking Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod booking_read_qualification_tests;')) {
    errors.push('PostgreSQL Booking qualification module is not registered')
  }
  for (const token of [
    'live_pg18_booking_reads_preserve_scope_availability_pricing_and_recomposition',
    'a.devices[0].serial_no, "BOOK-A-1"',
    '!a.dates[0].available',
    'search[0].daily_rate, 15.0',
    'price("BOOK-B-1")?.is_none()',
    'price_after.weekday_price, 10.0',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Booking evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-booking-authority.test.mjs',
    'node scripts/check-r4-p8-booking-authority.mjs',
    'live_pg18_booking_reads_preserve_scope_availability_pricing_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Booking qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateBookingAuthoritySnapshot(collectBookingAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Booking authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Booking authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
