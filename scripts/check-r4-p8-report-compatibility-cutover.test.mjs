#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectReportCompatibilitySnapshot,
  validateReportCompatibilitySnapshot,
} from './check-r4-p8-report-compatibility-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectReportCompatibilitySnapshot())
  mutate(snapshot)
  const errors = validateReportCompatibilitySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateReportCompatibilitySnapshot(collectReportCompatibilitySnapshot())
assert.deepEqual(baseline, [], `baseline must pass: ${baseline.join('; ')}`)

expectFailure(
  'Production report falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'ReportCompatibilityRepository::postgres(pg.clone())',
      'ReportCompatibilityRepository::new(pool.clone())',
    )
  },
  'ReportCompatibilityRepository::postgres(pg.clone())',
)

expectFailure(
  'Factory ignores injected report module',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'let report_built = match self.report_module.clone() {\n            Some(module) => constructed(module),',
      'let report_built = match self.report_module.clone() {\n            Some(_module) => constructed(with_pool!(FeatureReport)),',
    )
  },
  'Some(module) => constructed(module)',
)

expectFailure(
  'Factory removes report SQLite fallback binding',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'self.require_sqlite_pool("report compatibility")?',
      'REMOVED_REPORT_SQLITE_FALLBACK',
    )
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'Factory keeps capability token but severs report repository binding',
  snapshot => {
    const source = snapshot.factory
    snapshot.factory = snapshot.factory.replace(
      'ReportCompatibilityRepository::new(\n                    self.require_sqlite_pool("report compatibility")?,\n                )',
      'ReportCompatibilityRepository::new(legacy_pool.clone())\n                /* self.require_sqlite_pool("report compatibility")? */',
    )
    assert.notEqual(snapshot.factory, source, 'report binding mutation anchor missing')
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'PostgreSQL report aggregation drops explicit numeric casts',
  snapshot => {
    snapshot.postgres = snapshot.postgres.replace(
      'COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION',
      'COALESCE(SUM(o.totalprice), 0)',
    )
  },
  'COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION',
)

expectFailure(
  'Live report fixture drops tenant prerequisites',
  snapshot => {
    snapshot.live = snapshot.live.replace(
      'INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at)',
      'INSERT INTO missing_tenants (id, name, slug, status, plan, created_at, updated_at)',
    )
  },
  'INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at)',
)

expectFailure(
  'PostgreSQL report adapter regains SQLite coupling',
  snapshot => {
    snapshot.postgres += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite runtime types',
)

expectFailure(
  'Compatibility report duplicates Excel builder',
  snapshot => {
    snapshot.legacy = snapshot.legacy.replace(
      'pub fn build_excel(',
      'fn build_excel(',
    )
  },
  'share the Excel builder',
)

expectFailure(
  'Report production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Report gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 report compatibility cutover mutation tests passed.')
