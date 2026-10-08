#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/reservation_compatibility.rs',
  legacyModule: 'backend/system/admin/src/reservation.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/legacy_reservation.rs',
  dispatch: 'backend/src/repositories/legacy_reservation_dispatch.rs',
  postgres: 'backend/src/repositories/legacy_reservation_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  r1Boundary: 'scripts/check-r1p4-reservation-allocation-boundary.mjs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/legacy_reservation_qualification_tests.rs',
  pgTenantMigration: 'backend/src/db/migrations/postgres/082_r4_reservation_rule_tenant_invariant.sql',
  pgSequenceMigration: 'backend/src/db/migrations/postgres/083_r4_reservation_rule_sequence_invariant.sql',
  dbMod: 'backend/src/db/mod.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

function section(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  if (start < 0 || end < 0) return ''
  return source.slice(start, end)
}

export function collectReservationAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateReservationAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)
  const commands = compact(
    section(
      snapshot.legacyModule,
      'fn commands(&self) -> Vec<system_core::CommandMetadata> {',
      'fn schema(&self) -> ModuleSchema {',
    ),
  )
  const schema = compact(
    section(
      snapshot.legacyModule,
      'fn schema(&self) -> ModuleSchema {',
      '// ══════════════════════════════════════════════════════════════════════════\n// reserve',
    ),
  )
  const postgresRuleUpdate = compact(
    section(
      snapshot.postgres,
      'pub(in crate::repositories) fn update_rule(',
      '\n}\n\nfn nonempty',
    ),
  )

  for (const token of [
    'ReservationCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.legacy_reservations()',
    '.list(',
    '.get(',
    '.conflicts(',
    '.get_rule()',
    '.update_rule(',
    'BIZ_LEGACY_RESERVATION_WRITE_RETIRED',
    'use reservation_v2',
    'FeatureReservation::new().metadata()',
    'FeatureReservation::new().commands()',
    'FeatureReservation::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Reservation compatibility invariant missing: ' + token)
    }
  }
  if (application.includes('get_or_seed_rule')) {
    errors.push('Reservation rule_get must remain read-only')
  }

  for (const retired of ['reserve', 'release', 'confirm', 'expire']) {
    if (commands.includes(compact('CommandMetadata::new("' + retired + '"'))) {
      errors.push('Legacy Reservation write command is advertised: ' + retired)
    }
    if (schema.includes(compact('name: "' + retired + '".into()'))) {
      errors.push('Legacy Reservation write command remains in schema: ' + retired)
    }
  }
  for (const retained of ['list', 'get', 'check_availability', 'rule_get', 'rule_update']) {
    if (!commands.includes(compact('CommandMetadata::new("' + retained + '"'))) {
      errors.push('Legacy Reservation compatibility command missing: ' + retained)
    }
  }
  if (!commands.includes('SimulationSupport::Blocked')) {
    errors.push('Legacy Reservation compatibility commands must remain Simulation blocked')
  }

  for (const token of [
    'mod legacy_reservation;',
    'mod legacy_reservation_dispatch;',
    'mod legacy_reservation_postgres;',
    'pub use legacy_reservation_dispatch::ScopedLegacyReservationRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Legacy Reservation repository wiring missing: ' + token)
    }
  }
  if (!snapshot.provider.includes(
    "pub fn legacy_reservations(&self) -> ScopedLegacyReservationRepository<'_>",
  )) {
    errors.push('ScopedRepositories must expose legacy Reservation compatibility')
  }
  if (!snapshot.dispatch.includes('PostgresLegacyReservationRepository')
      || !snapshot.dispatch.includes('SqliteLegacyReservationRepository')) {
    errors.push('Legacy Reservation backend dispatch is incomplete')
  }

  const sqliteProduction = snapshot.sqlite.split('#[cfg(test)]')[0]

  for (const token of [
    'WHERE tenant_id=?1 AND id=?2',
    "status IN ('reserved','confirmed')",
    '?3 < end_date AND start_date < ?4',
    'write_immediate',
    'sqlite_legacy_reservation_compatibility_preserves_scope_conflicts_and_rules',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite legacy Reservation invariant missing: ' + token)
    }
  }
  if (sqliteProduction.includes('INSERT INTO inventory_reservations')
      || sqliteProduction.includes('UPDATE inventory_reservations')) {
    errors.push('Legacy Reservation compatibility must not regain reservation-record write authority')
  }

  for (const token of [
    'QueryBuilder::<Postgres>',
    'tenant_id=',
    "status IN ('reserved','confirmed')",
    '$3 < end_date AND start_date < $4',
    'pg_write_serializable_repository',
    'FOR UPDATE',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL legacy Reservation invariant missing: ' + token)
    }
  }
  for (const token of [
    'max_days_ahead::bigint AS max_days_ahead',
    'max_concurrent_per_customer::bigint AS max_concurrent_per_customer',
    'auto_release_minutes::bigint AS auto_release_minutes',
  ]) {
    if (!postgresRuleUpdate.includes(compact(token))) {
      errors.push('PostgreSQL locked Reservation rule projection missing: ' + token)
    }
  }

  if (snapshot.postgres.includes('INSERT INTO inventory_reservations')
      || snapshot.postgres.includes('UPDATE inventory_reservations')) {
    errors.push('PostgreSQL legacy Reservation compatibility must not regain reservation-record write authority')
  }

  if (!factory.includes('ReservationCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Reservation provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureReservation)')) {
    errors.push('Registry Reservation restored direct SQLite construction')
  }
  if (factory.includes('(FeatureReservation,Reservation,"reservation")')) {
    errors.push('Registry Reservation retained stale FeatureReservation type mapping')
  }
  if (!descriptors.includes(
    'descriptor!("reservation",Business,ModuleActivation::Always,NONE,Reservation)',
  )) {
    errors.push('Reservation Registry descriptor must not require SQLite')
  }

  for (const token of [
    'RESERVATION_SINGLE_WRITE_AUTHORITY',
    'LEGACY_RESERVATION_INFERENCE_FORBIDDEN',
    'ReservationV2Module::new(repository_provider.clone())',
  ]) {
    const haystack = token.startsWith('ReservationV2') ? snapshot.factory : snapshot.r1Boundary
    if (!haystack.includes(token)) {
      errors.push('Canonical Reservation V2 authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'ALTER TABLE reservation_rules',
    'ADD COLUMN IF NOT EXISTS tenant_id TEXT',
    'ALTER COLUMN tenant_id SET NOT NULL',
    'ON reservation_rules(tenant_id)',
  ]) {
    if (!snapshot.pgTenantMigration.includes(token)) {
      errors.push('PostgreSQL Reservation tenant migration invariant missing: ' + token)
    }
  }
  if (!snapshot.dbMod.includes('run_pg_extension_082(pool).await?')) {
    errors.push('PostgreSQL Reservation tenant migration is not in production composition')
  }

  for (const token of [
    'setval(',
    "pg_get_serial_sequence('reservation_rules', 'id')",
    'COALESCE(MAX(id), 1)',
    'MAX(id) IS NOT NULL',
  ]) {
    if (!snapshot.pgSequenceMigration.includes(token)) {
      errors.push('PostgreSQL Reservation sequence migration invariant missing: ' + token)
    }
  }
  if (!snapshot.dbMod.includes('run_pg_extension_083(pool).await?')) {
    errors.push('PostgreSQL Reservation sequence migration is not in production composition')
  }

  if (!snapshot.postgresMod.includes('mod legacy_reservation_qualification_tests;')) {
    errors.push('PostgreSQL legacy Reservation qualification module is not registered')
  }
  const pgTest = compact(snapshot.pgTest)
  for (const token of [
    'live_pg18_legacy_reservation_compatibility_preserves_scope_conflicts_rules_and_recomposition',
    'a.items[0].reserved_by, "identity-reservation-a"',
    'get(b.items[0].id)?.is_none()',
    'assert!(adjacent.is_empty())',
    'assert!(scoped_b.legacy_reservations().get_rule()?.is_none())',
    'rule.max_concurrent_per_customer, 7',
  ]) {
    if (!pgTest.includes(compact(token))) {
      errors.push('PG18 legacy Reservation evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-reservation-authority.test.mjs',
    'node scripts/check-r4-p8-reservation-authority.mjs',
    'live_pg18_legacy_reservation_compatibility_preserves_scope_conflicts_rules_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Reservation qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateReservationAuthoritySnapshot(collectReservationAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Reservation authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Reservation authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
