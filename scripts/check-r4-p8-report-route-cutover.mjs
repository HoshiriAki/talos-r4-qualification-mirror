#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/reports.rs',
  official: 'backend/official/order/src/report.rs',
  servicesMod: 'backend/src/services/mod.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}
const LEGACY_SERVICE = 'backend/src/services/report_service.rs'

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectReportRouteSnapshot() {
  return {
    ...Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)])),
    legacyServiceExists: existsSync(path.join(ROOT, LEGACY_SERVICE)),
  }
}

export function validateReportRouteSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'TrustedTenantUser',
    '.registry',
    '"report"',
    '"get_revenue_data"',
    'tenant_user.context()',
    'RevenueData',
    'FeatureReport::build_excel_bytes(&data)',
    'format_export_timestamp()',
  ]) {
    if (!snapshot.route.includes(token)) errors.push('Report HTTP route cutover missing: ' + token)
  }

  for (const forbidden of ['state.pool', 'report_service', 'rusqlite', 'SqliteConnectionManager']) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('Report HTTP route must not retain direct SQLite/legacy coupling: ' + forbidden)
    }
  }

  for (const token of [
    'pub fn build_excel_bytes(data: &RevenueData) -> Result<Vec<u8>, String>',
    'let buffer = Self::build_excel_bytes(data)?;',
    'pub fn build_excel(data: &RevenueData, label: &str) -> Result<ExportExcelOutput, String>',
  ]) {
    if (!snapshot.official.includes(token)) errors.push('Shared report workbook builder missing: ' + token)
  }

  if (snapshot.servicesMod.includes('pub mod report_service;')) {
    errors.push('Legacy report_service module must be retired from backend services')
  }
  if (snapshot.legacyServiceExists) {
    errors.push('Legacy SQLite report_service.rs must be removed after HTTP route cutover')
  }

  for (const token of [
    'node scripts/check-r4-p8-report-route-cutover.test.mjs',
    'node scripts/check-r4-p8-report-route-cutover.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push('Exact-head report route qualification missing: ' + token)
  }

  return errors
}

function main() {
  const errors = validateReportRouteSnapshot(collectReportRouteSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 report HTTP route cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 report HTTP route cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
