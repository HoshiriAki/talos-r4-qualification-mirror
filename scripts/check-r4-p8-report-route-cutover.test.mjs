#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectReportRouteSnapshot,
  validateReportRouteSnapshot,
} from './check-r4-p8-report-route-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectReportRouteSnapshot())
  mutate(snapshot)
  const errors = validateReportRouteSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateReportRouteSnapshot(collectReportRouteSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'HTTP report route loses trusted tenant extraction',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      'TrustedTenantUser',
      'AuthUser',
      'HTTP report route loses trusted tenant extraction',
    )
  },
  'TrustedTenantUser',
)

expectFailure(
  'HTTP report route regains direct SQLite access',
  snapshot => {
    snapshot.route += '\nfn regression(state: &AppState) { let _ = state.pool.get(); }\n'
  },
  'state.pool',
)

expectFailure(
  'HTTP report route bypasses Registry report query',
  snapshot => {
    snapshot.route = replaceRequired(
      snapshot.route,
      '"get_revenue_data"',
      '"legacy_get_revenue_data"',
      'HTTP report route bypasses Registry report query',
    )
  },
  '"get_revenue_data"',
)

expectFailure(
  'Shared workbook bytes builder becomes private',
  snapshot => {
    snapshot.official = replaceRequired(
      snapshot.official,
      'pub fn build_excel_bytes(data: &RevenueData) -> Result<Vec<u8>, String>',
      'fn build_excel_bytes(data: &RevenueData) -> Result<Vec<u8>, String>',
      'Shared workbook bytes builder becomes private',
    )
  },
  'pub fn build_excel_bytes',
)

expectFailure(
  'Legacy report service is re-exported',
  snapshot => {
    snapshot.servicesMod += '\npub mod report_service;\n'
  },
  'Legacy report_service module',
)

expectFailure(
  'Legacy SQLite report service file reappears',
  snapshot => {
    snapshot.legacyServiceExists = true
  },
  'report_service.rs',
)

console.log('R4-P8 report HTTP route cutover mutation tests passed.')
