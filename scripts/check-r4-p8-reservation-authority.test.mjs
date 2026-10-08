#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectReservationAuthoritySnapshot,
  validateReservationAuthoritySnapshot,
} from './check-r4-p8-reservation-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectReservationAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateReservationAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateReservationAuthoritySnapshot(collectReservationAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Reservation factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'ReservationCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureReservation)',
      'Reservation factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Reservation descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "reservation",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Reservation\n    )',
      'descriptor!(\n        "reservation",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Reservation\n    )',
      'Reservation descriptor restores SQLite storage',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Legacy Reservation re-advertises reserve',
  snapshot => {
    snapshot.legacyModule = replaceRequired(
      snapshot.legacyModule,
      'CommandMetadata::new(\n                "list"',
      'CommandMetadata::new(\n                "reserve"',
      'Legacy Reservation re-advertises reserve',
    )
  },
  'write command is advertised: reserve',
)

expectFailure(
  'Reservation compatibility removes retirement error',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      'BIZ_LEGACY_RESERVATION_WRITE_RETIRED',
      'BIZ_RESERVATION_WRITE_ACTIVE',
      'Reservation compatibility removes retirement error',
    )
  },
  'BIZ_LEGACY_RESERVATION_WRITE_RETIRED',
)

expectFailure(
  'Legacy Reservation repository regains record writes',
  snapshot => {
    snapshot.sqlite = replaceRequired(
      snapshot.sqlite,
      '#[cfg(test)]',
      '// INSERT INTO inventory_reservations (...) VALUES (...)\n#[cfg(test)]',
      'Legacy Reservation repository regains record writes',
    )
  },
  'must not regain reservation-record write authority',
)

expectFailure(
  'PostgreSQL Reservation rule update loses serialization',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'pg_write_serializable_repository',
      'pg_write',
      'PostgreSQL Reservation rule update loses serialization',
    )
  },
  'PostgreSQL legacy Reservation invariant missing',
)

expectFailure(
  'PostgreSQL Reservation locked rule read loses integer normalization',
  snapshot => {
    const updateRuleStart = snapshot.postgres.indexOf(
      'pub(in crate::repositories) fn update_rule(',
    )
    assert.ok(updateRuleStart >= 0, 'locked rule mutation section missing')
    const before = snapshot.postgres.slice(0, updateRuleStart)
    const updateRule = snapshot.postgres.slice(updateRuleStart)
    snapshot.postgres = before + replaceRequired(
      updateRule,
      'max_days_ahead::bigint AS max_days_ahead',
      'max_days_ahead',
      'PostgreSQL Reservation locked rule read loses integer normalization',
    )
  },
  'PostgreSQL locked Reservation rule projection missing: max_days_ahead::bigint AS max_days_ahead',
)

expectFailure(
  'PostgreSQL Reservation reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.postgres.includes('tenant_id='), 'tenant mutation anchor missing')
    snapshot.postgres = snapshot.postgres.replaceAll('tenant_id=', 'tenant_scope_removed=')
  },
  'PostgreSQL legacy Reservation invariant missing: tenant_id=',
)

expectFailure(
  'Reservation tenant migration loses tenant column',
  snapshot => {
    snapshot.pgTenantMigration = replaceRequired(
      snapshot.pgTenantMigration,
      'ADD COLUMN IF NOT EXISTS tenant_id TEXT',
      'ADD COLUMN IF NOT EXISTS scope_removed TEXT',
      'Reservation tenant migration loses tenant column',
    )
  },
  'tenant migration invariant missing',
)

expectFailure(
  'Reservation tenant migration leaves production composition',
  snapshot => {
    snapshot.dbMod = replaceRequired(
      snapshot.dbMod,
      'run_pg_extension_082(pool).await?',
      'run_pg_extension_removed(pool).await?',
      'Reservation tenant migration leaves production composition',
    )
  },
  'not in production composition',
)

expectFailure(
  'Reservation sequence migration loses serial alignment',
  snapshot => {
    snapshot.pgSequenceMigration = replaceRequired(
      snapshot.pgSequenceMigration,
      "pg_get_serial_sequence('reservation_rules', 'id')",
      "'reservation_rules_sequence_removed'",
      'Reservation sequence migration loses serial alignment',
    )
  },
  'sequence migration invariant missing',
)

expectFailure(
  'Reservation sequence migration leaves production composition',
  snapshot => {
    snapshot.dbMod = replaceRequired(
      snapshot.dbMod,
      'run_pg_extension_083(pool).await?',
      'run_pg_extension_removed(pool).await?',
      'Reservation sequence migration leaves production composition',
    )
  },
  'sequence migration is not in production composition',
)

expectFailure(
  'Reservation live proof loses cross-tenant rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'get(b.items[0].id)?.is_none()',
      'get(b.items[0].id)?.is_some()',
      'Reservation live proof loses cross-tenant rejection',
    )
  },
  'get(b.items[0].id)?.is_none()',
)

console.log('R4-P8 Reservation authority mutation tests passed.')
