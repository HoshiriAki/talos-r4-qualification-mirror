#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-030'
const PATHS = Object.freeze({
  domain: 'backend/src/domain/reservation.rs',
  application: 'backend/src/application/reservation_v2.rs',
  tests: 'backend/src/application/reservation_tests.rs',
  repository: 'backend/src/repositories/reservation.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  session: 'backend/src/repositories/sqlite/session.rs',
  sqliteMigration: 'backend/src/db/migrations/055_reservation_allocation_v2.sql',
  pgMigration: 'backend/src/db/migrations/postgres/055_reservation_allocation_v2.sql',
  migrations: 'backend/src/db/migrations.rs',
  pgMigrations: 'backend/src/db/migrations_pg.rs',
  routes: 'backend/src/routes/reservations_v2.rs',
  bookingRoutes: 'backend/src/routes/booking.rs',
  bookingModule: 'backend/system/admin/src/booking.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  package: 'package.json',
  docsIndex: 'policy/qualification/legacy-evidence/docs/README.md',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p4-reservation-allocation.md',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}
function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(finding(path, evidence))
}
function forbidText(failures, source, token, path, evidence) {
  if (source.includes(token)) failures.push(finding(path, evidence))
}
function requirePattern(failures, source, pattern, path, evidence) {
  if (!pattern.test(source)) failures.push(finding(path, evidence))
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function checkR1P4ReservationAllocationBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R1-P4 evidence is missing'))
  }
  if (failures.length > 0) return failures

  const domain = files[PATHS.domain]
  const application = files[PATHS.application]
  const tests = files[PATHS.tests]
  const repository = files[PATHS.repository]
  const provider = files[PATHS.provider]
  const session = files[PATHS.session]
  const sqliteMigration = files[PATHS.sqliteMigration]
  const pgMigration = files[PATHS.pgMigration]
  const migrations = files[PATHS.migrations]
  const pgMigrations = files[PATHS.pgMigrations]
  const routes = files[PATHS.routes]
  const bookingRoutes = files[PATHS.bookingRoutes]
  const bookingModule = files[PATHS.bookingModule]
  const descriptors = files[PATHS.descriptors]
  const factory = files[PATHS.factory]
  const packageJson = files[PATHS.package]
  const docsIndex = files[PATHS.docsIndex]
  const record = files[PATHS.record]

  for (const token of ['ReservationId', 'ReservationRequirementId', 'AllocationId', 'Uuid::new_v4()', 'ReservationStatus', 'AllocationStatus']) {
    requireText(failures, domain, token, PATHS.domain, `typed UUID Reservation authority missing ${token}`)
  }
  forbidText(failures, domain, 'pub struct ReservationId(i64', PATHS.domain, 'canonical Reservation ID must not be integer-backed')

  for (const token of [
    'pub struct ReservationV2Module',
    'RepositoryProvider',
    'self.repositories.bind(ctx)?',
    '"create_from_order"',
    '"availability"',
    '"confirm_reservation"',
    '"expire_due"',
    '"allocate_device"',
    '"release_allocation"',
    'SimulationSupport::Blocked',
  ]) requireText(failures, application, token, PATHS.application, `Reservation V2 application boundary missing ${token}`)
  forbidText(failures, application, 'Pool<SqliteConnectionManager>', PATHS.application, 'canonical Reservation module must not own a raw pool')

  for (const token of [
    'write_immediate',
    'load_order_demand',
    'ensure_capacity',
    '?3 < end_date',
    'start_date < ?4',
    'create_from_order',
    'allocate_device',
    'expire_due',
    'list_migration_exceptions',
  ]) requireText(failures, repository, token, PATHS.repository, `scoped Reservation repository missing ${token}`)
  requireText(failures, provider, 'pub fn reservations(&self)', PATHS.provider, 'ScopedRepositories must expose Reservation repository')
  requireText(failures, session, 'TransactionBehavior::Immediate', PATHS.session, 'capacity check + insert must use BEGIN IMMEDIATE semantics')
  requireText(failures, session, 'pub(in crate::repositories) fn write_immediate', PATHS.session, 'scoped immediate write primitive missing')

  for (const token of [
    'CREATE TABLE IF NOT EXISTS rental_reservations',
    'CREATE TABLE IF NOT EXISTS reservation_requirements',
    'CREATE TABLE IF NOT EXISTS allocations',
    'CREATE TABLE IF NOT EXISTS reservation_migration_exceptions',
    'CHECK(start_date < end_date)',
    'NEW.start_date < a.end_date',
    'a.start_date < NEW.end_date',
    "r.status = 'confirmed'",
    "source_table IN ('inventory_reservations', 'booking_availability', 'order_devices')",
    'FROM inventory_reservations',
    'FROM booking_availability',
    'FROM order_devices',
  ]) requireText(failures, sqliteMigration, token, PATHS.sqliteMigration, `SQLite migration 055 missing ${token}`)
  forbidText(failures, sqliteMigration, 'start_date <= a.end_date', PATHS.sqliteMigration, 'inclusive overlap semantics are forbidden')
  forbidText(failures, sqliteMigration, 'a.start_date <= NEW.end_date', PATHS.sqliteMigration, 'inclusive overlap semantics are forbidden')
  for (const token of ['rental_reservations', 'reservation_requirements', 'allocations', 'reservation_migration_exceptions', 'NEW.start_date < a.end_date', 'a.start_date < NEW.end_date']) {
    requireText(failures, pgMigration, token, PATHS.pgMigration, `PostgreSQL migration 055 missing ${token}`)
  }
  requireText(failures, migrations, '055_reservation_allocation_v2', PATHS.migrations, 'SQLite migration registry missing 055')
  requireText(failures, pgMigrations, '055_reservation_allocation_v2', PATHS.pgMigrations, 'PostgreSQL migration registry missing 055')

  for (const token of ['TrustedTenantUser', 'TrustedTenantAdmin', '"/api/v2/reservations', '"reservation_v2"', 'tenant_user.context()', 'tenant_admin.context()']) {
    requireText(failures, routes, token, PATHS.routes, `canonical Reservation route missing ${token}`)
  }
  forbidText(failures, routes, 'state.pool', PATHS.routes, 'canonical Reservation route must not own raw-pool access')
  forbidText(failures, routes, 'SELECT ', PATHS.routes, 'route SQL is forbidden')

  requireText(failures, bookingRoutes, '"reservation_v2"', PATHS.bookingRoutes, 'legacy Booking reserve/confirm/availability must delegate to canonical reservation_v2')
  forbidText(failures, bookingRoutes, '.execute(\n            "booking",\n            "reserve"', PATHS.bookingRoutes, 'legacy Booking route must not invoke raw-pool reserve authority')
  forbidText(failures, bookingRoutes, '.execute(\n            "booking",\n            "confirm"', PATHS.bookingRoutes, 'legacy Booking route must not invoke raw-pool confirm authority')
  for (const token of ['"availability"', '"estimate"', '"device_search"']) {
    requireText(failures, bookingModule, token, PATHS.bookingModule, `bounded read-only Booking compatibility missing ${token}`)
  }
  forbidText(failures, compact(bookingModule), 'CommandMetadata::new("reserve"', PATHS.bookingModule, 'legacy Booking reserve must not remain advertised Registry authority')
  forbidText(failures, compact(bookingModule), 'CommandMetadata::new("confirm"', PATHS.bookingModule, 'legacy Booking confirm must not remain advertised Registry authority')

  requirePattern(
    failures,
    compact(descriptors),
    /descriptor!\("reservation_v2",Business,ModuleActivation::Always,[A-Z_]+,ReservationV2\)/,
    PATHS.descriptors,
    'descriptor catalog must register reservation_v2',
  )
  requireText(failures, factory, '(ReservationV2Module, ReservationV2, "reservation_v2")', PATHS.factory, 'ModuleFactory identity must own reservation_v2')
  requireText(failures, factory, 'ReservationV2Module::new(repository_provider.clone())', PATHS.factory, 'ReservationV2 must receive scoped repository provider')
  requireText(failures, factory, '"reservation_v2"', PATHS.factory, 'baseline module set must include reservation_v2')

  for (const token of [
    'half_open_model_capacity_allows_adjacency_and_rejects_overlap',
    'expired_hold_releases_capacity_for_overlapping_demand',
    'confirmed_reservations_cannot_allocate_the_same_device_for_overlapping_windows',
    'migration_exceptions_are_explicit_and_tenant_scoped',
    'preview_can_read_but_cannot_reserve_and_simulation_fails_closed',
    '055_reservation_allocation_v2.sql',
  ]) requireText(failures, tests, token, PATHS.tests, `R1-P4 executable test evidence missing ${token}`)

  for (const token of [
    'RESERVATION_SINGLE_WRITE_AUTHORITY',
    'RESERVATION_UUID_REQUIRED',
    'HALF_OPEN_INTERVAL_REQUIRED',
    'MODEL_CAPACITY_RESERVATION_REQUIRED',
    'RESERVATION_TRANSACTION_SERIALIZATION_REQUIRED',
    'ALLOCATION_OVERLAP_GUARD_REQUIRED',
    'RESERVATION_EXPIRY_REQUIRED',
    'LEGACY_RESERVATION_INFERENCE_FORBIDDEN',
    'BOOKING_WRITE_COMPATIBILITY_ADAPTER_ONLY',
    'TALOS-OPS-030',
  ]) requireText(failures, record, token, PATHS.record, `R1-P4 execution record missing ${token}`)

  requireText(failures, docsIndex, 'R1-P4 Reservation / Allocation', PATHS.docsIndex, 'documentation index must register R1-P4')
  for (const token of [
    'quality:talos-ops:reservation-allocation:test',
    'quality:talos-ops:reservation-allocation',
    'check-r1p4-reservation-allocation-boundary.test.mjs',
    'check-r1p4-reservation-allocation-boundary.mjs',
  ]) requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)

  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const failures = checkR1P4ReservationAllocationBoundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('R1-P4 Reservation / Allocation boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R1-P4 Reservation / Allocation boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
