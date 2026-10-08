#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  sqlite: 'backend/src/repositories/report_compatibility.rs',
  dispatch: 'backend/src/repositories/report_compatibility_dispatch.rs',
  postgres: 'backend/src/repositories/report_compatibility_postgres.rs',
  application: 'backend/src/application/report_compatibility.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  legacy: 'backend/official/order/src/report.rs',
  live: 'backend/src/repositories/postgres/report_compatibility_qualification_tests.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.resolve(process.cwd(), relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

function countOccurrences(source, token) {
  return source.split(token).length - 1
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectReportCompatibilitySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateReportCompatibilitySnapshot(snapshot) {
  const errors = []

  for (const token of [
    'SqliteReportCompatibilityRepository',
    'SELECT COUNT(*)',
    'COUNT(DISTINCT od.serialNo)',
    'GROUP BY o.province',
    'substr(o.startDate, 1, 7)',
    'o.tenant_id = ?',
  ]) {
    if (!snapshot.sqlite.includes(token)) errors.push(`SQLite report compatibility missing: ${token}`)
  }

  for (const token of [
    'ReportCompatibilityBackend',
    'Postgres(PostgresReportCompatibilityRepository)',
    'pub(crate) struct ReportCompatibilityRepository',
  ]) {
    if (!snapshot.dispatch.includes(token)) errors.push(`Report dispatch missing: ${token}`)
  }

  for (const token of [
    'PostgresReportCompatibilityRepository',
    'QueryBuilder::<Postgres>',
    'COUNT(*)::BIGINT',
    'COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION',
    'COUNT(DISTINCT od.serialno)::BIGINT',
    'GROUP BY o.province',
    'substr(o.startdate, 1, 7)',
    'o.tenant_id = ',
  ]) {
    if (!snapshot.postgres.includes(token)) errors.push(`PostgreSQL report compatibility missing: ${token}`)
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL report compatibility must not depend on SQLite runtime types')
  }

  for (const [token, expected] of [
    ['COUNT(*)::BIGINT', 3],
    ['COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION', 3],
    ['COALESCE(AVG(o.totalprice), 0)::DOUBLE PRECISION', 3],
  ]) {
    const actual = countOccurrences(snapshot.postgres, token)
    if (actual !== expected) {
      errors.push(`PostgreSQL report aggregate cast count mismatch: ${token} expected ${expected}, got ${actual}`)
    }
  }

  for (const token of [
    'pub(crate) struct ReportCompatibilityModule',
    'FeatureReport::build_excel(&data, &input.label)',
    '"get_revenue_data"',
    '"export_excel"',
    'SimulationSupport::Supported',
  ]) {
    if (!snapshot.application.includes(token)) errors.push(`Report compatibility module missing: ${token}`)
  }
  if (!snapshot.legacy.includes('pub fn build_excel(')) {
    errors.push('Legacy and compatibility report paths must share the Excel builder')
  }

  for (const token of [
    'report_module: Option<ReportCompatibilityModule>',
    '(ReportCompatibilityModule, Report, "report")',
    'pub(crate) fn with_report_module(',
  ]) {
    if (!snapshot.factory.includes(token)) errors.push(`Report factory composition missing: ${token}`)
  }
  const reportFactoryBlock = between(
    snapshot.factory,
    'let report_built = match self.report_module.clone() {',
    '\n        let logistics_concrete',
  )
  const compactReportFactoryBlock = compact(reportFactoryBlock)
  for (const token of [
    'Some(module) => constructed(module)',
    'None => constructed(ReportCompatibilityModule::new(',
  ]) {
    if (!compactReportFactoryBlock.includes(compact(token))) {
      errors.push(`Report factory runtime selection missing: ${token}`)
    }
  }
  if (!/ReportCompatibilityRepository::new\(\s*self\.require_sqlite_pool\(\s*"report compatibility"\s*\)\?\s*,?\s*\)/s.test(reportFactoryBlock)) {
    errors.push('Report factory must bind the explicit SQLite capability directly to ReportCompatibilityRepository')
  }

  for (const token of [
    'report_module: ReportCompatibilityModule',
    '.with_report_module(report_module)',
  ]) {
    if (!snapshot.assembler.includes(token)) errors.push(`Report assembler composition missing: ${token}`)
  }

  for (const token of [
    'ReportCompatibilityRepository::postgres(pg.clone())',
    'ReportCompatibilityRepository::new(pool.clone())',
    'ReportCompatibilityModule::new(report_compatibility_repository)',
    'report_module,',
  ]) {
    if (!snapshot.main.includes(token)) errors.push(`Report production composition missing: ${token}`)
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Report production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Report cutover must not restore the retired transitional production barrier')
  }

  if (!snapshot.pgMod.includes('mod report_compatibility_qualification_tests;')) {
    errors.push('Report PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_report_compatibility_preserves_tenant_scope_and_aggregates',
    'INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at)',
    'assert_eq!(full.summary.total_orders, 2);',
    'assert_eq!(full.summary.devices_rented, 2);',
    'assert_eq!(filtered.summary.total_orders, 1);',
    'assert_eq!(foreign.summary.total_orders, 1);',
  ]) {
    if (!snapshot.live.includes(token)) errors.push(`Report PostgreSQL live proof missing: ${token}`)
  }

  for (const token of [
    'node scripts/check-r4-p8-report-compatibility-cutover.test.mjs',
    'node scripts/check-r4-p8-report-compatibility-cutover.mjs',
    'live_pg18_report_compatibility_preserves_tenant_scope_and_aggregates',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push(`Exact-head report qualification missing: ${token}`)
  }

  return errors
}

function main() {
  const errors = validateReportCompatibilitySnapshot(collectReportCompatibilitySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 report compatibility cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 report compatibility cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
