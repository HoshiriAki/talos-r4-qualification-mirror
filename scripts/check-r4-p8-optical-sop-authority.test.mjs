#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectOpticalSopAuthoritySnapshot,
  validateOpticalSopAuthoritySnapshot,
} from './check-r4-p8-optical-sop-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectOpticalSopAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateOpticalSopAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateOpticalSopAuthoritySnapshot(collectOpticalSopAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Optical SOP factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'OpticalSopCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureOpticalSop)',
      'Optical SOP factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Optical SOP descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      '        NONE,\n        OpticalSop',
      '        SQLITE,\n        OpticalSop',
      'Optical SOP descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Optical SOP route restores numeric order identity',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      'order_id: String',
      'order_id: i64',
      'Optical SOP route restores numeric order identity',
    )
  },
  'order identity regressed to numeric',
)

expectFailure(
  'Optical SOP route loses current frontend aliases',
  snapshot => {
    assert.ok(
      snapshot.route.includes('#[serde(alias = "inspection_id", alias = "inspectionId")]'),
      'Optical SOP route loses current frontend aliases: mutation anchor missing',
    )
    snapshot.route = snapshot.route.replaceAll(
      '#[serde(alias = "inspection_id", alias = "inspectionId")]',
      '#[serde(alias = "inspectionId")]',
    )
  },
  'HTTP compatibility missing',
)

expectFailure(
  'Optical SOP migration restores numeric order identity',
  snapshot => {
    snapshot.migrationSqlite = replaceRequired(
      snapshot.migrationSqlite,
      'order_id TEXT NOT NULL',
      'order_id INTEGER NOT NULL',
      'Optical SOP migration restores numeric order identity',
    )
  },
  'SQLite Optical SOP migration invariant missing',
)

expectFailure(
  'Optical SOP migration loses tenant-local duplicate protection',
  snapshot => {
    snapshot.migrationSqlite = replaceRequired(
      snapshot.migrationSqlite,
      'UNIQUE(tenant_id, order_id, device_serial_no)',
      'UNIQUE(order_id, device_serial_no)',
      'Optical SOP migration loses tenant-local duplicate protection',
    )
  },
  'SQLite Optical SOP migration invariant missing',
)

expectFailure(
  'Optical SOP PostgreSQL mutation loses serialization',
  snapshot => {
    snapshot.pgMutation = snapshot.pgMutation.replaceAll(
      'pg_write_serializable_repository',
      'pg_write',
    )
  },
  'mutation invariant missing',
)

expectFailure(
  'Optical SOP PostgreSQL mutation loses checklist lock',
  snapshot => {
    snapshot.pgMutation = snapshot.pgMutation.replaceAll('FOR UPDATE', 'NO_ROW_LOCK')
  },
  'mutation invariant missing',
)

expectFailure(
  'Optical SOP completion restores obsolete Damage schema',
  snapshot => {
    snapshot.pgMutation += '\n// reporter_id severity detected_at\n'
  },
  'obsolete Damage schema',
)

expectFailure(
  'Optical SOP frontend loses canonical wizard fields',
  snapshot => {
    snapshot.frontendPage = replaceRequired(
      snapshot.frontendPage,
      "field: 'bodyOk'",
      "field: 'body_ok'",
      'Optical SOP frontend loses canonical wizard fields',
    )
  },
  'frontend wizard compatibility missing',
)

expectFailure(
  'Optical SOP live proof loses idempotent completion evidence',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'assert_eq!(damage_count, 1);',
      'assert_eq!(damage_count, 2);',
      'Optical SOP live proof loses idempotent completion evidence',
    )
  },
  'assert_eq!(damage_count, 1);',
)

console.log('R4-P8 Optical SOP authority mutation tests passed.')
