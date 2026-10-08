#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  route: 'backend/src/routes/dashboard.rs',
  authority: 'backend/src/application/dashboard_read_authority.rs',
  services: 'backend/src/application/services.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/dashboard_read.rs',
  postgres: 'backend/src/repositories/dashboard_read_postgres.rs',
  dispatch: 'backend/src/repositories/dashboard_read_dispatch.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/dashboard_read_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectDashboardSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDashboardSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'state.pool',
    'dashboard_service::',
    'order_compatibility_support::get_dashboard_stats',
    'order_compatibility_support::get_daily_order_counts',
    'AuthUser',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('dashboard route retains legacy/global authority: ' + forbidden)
    }
  }

  for (const token of [
    'TenantUser',
    'make_ctx(&user, state.http_client.clone())',
    '.application_services()',
    '.dashboards()',
    '.overview(&ctx)',
    '.daily_order_counts(&ctx, days)',
    '.order_trend(&ctx, &q.granularity, q.days)',
    '.revenue_trend(&ctx, &q.granularity, q.days)',
    '.cancel_trend(&ctx, q.days)',
    '.device_status_distribution(&ctx)',
    '.province_stats(&ctx, &q.r#type, q.days)',
    '.model_ranking(&ctx, q.days, q.limit)',
    '.warehouse_stats(&ctx)',
  ]) {
    if (!snapshot.route.includes(token)) {
      errors.push('dashboard HTTP tenant-scoped cutover missing: ' + token)
    }
  }

  for (const token of [
    'DashboardReadAuthorityService',
    'repository_provider: Arc<dyn RepositoryProvider>',
    'self.repository_provider.bind(ctx)',
    '.dashboards().overview(',
    '.dashboards().order_windows(',
    '.dashboards().order_daily(',
    '.dashboards().revenue_daily(',
    '.dashboards().cancel_daily(',
    '.dashboards().device_status_distribution()',
    '.dashboards().province_pie(',
    '.dashboards().province_trend(',
    '.dashboards().model_ranking(',
    '.dashboards().warehouse_stats()',
    'fn overview_json(',
    'fn rebucket_counts(',
    'fn rebucket_revenue(',
  ]) {
    if (!snapshot.authority.includes(token)) {
      errors.push('dashboard application authority missing: ' + token)
    }
  }

  for (const token of [
    'pub fn dashboards(&self) -> DashboardReadAuthorityService',
    'DashboardReadAuthorityService::new(self.repository_provider())',
  ]) {
    if (!snapshot.services.includes(token)) {
      errors.push('ApplicationServices dashboard factory missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn dashboards(&self) -> ScopedDashboardReadRepository<'_>")) {
    errors.push('ScopedRepositories must expose the scoped dashboard authority')
  }

  for (const token of [
    'WHERE tenant_id=?1',
    'WHERE od.tenant_id=?1',
    'FROM audit_events',
    'WHERE tenant_id=?1 AND action=\'order_delete\'',
    'substr(occurred_at,1,10)',
    'WHERE od.tenant_id=?1 AND substr(o.createdAt,1,10)>=?2',
    'WHERE w.tenant_id=?1',
    'o.tenant_id=w.tenant_id',
    'd.tenant_id=od.tenant_id',
    'dm.tenant_id=d.tenant_id',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite dashboard tenant boundary missing: ' + token)
    }
  }

  for (const token of [
    'WHERE tenant_id=$1',
    'WHERE od.tenant_id=$1',
    'FROM audit_events',
    'WHERE tenant_id=$1 AND action=\'order_delete\'',
    'substr(occurred_at,1,10)',
    'WHERE od.tenant_id=$1 AND substr(o.createdat,1,10)>=$2',
    'WHERE w.tenant_id=$1',
    'o.tenant_id=w.tenant_id',
    'd.tenant_id=od.tenant_id',
    'dm.tenant_id=d.tenant_id',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL dashboard tenant boundary missing: ' + token)
    }
  }

  for (const token of [
    'pub fn overview(',
    'pub fn order_windows(',
    'pub fn order_daily(',
    'pub fn revenue_daily(',
    'pub fn cancel_daily(',
    'pub fn device_status_distribution(',
    'pub fn province_pie(',
    'pub fn province_trend(',
    'pub fn model_ranking(',
    'pub fn warehouse_stats(',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push('dashboard backend dispatch missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod dashboard_read_qualification_tests;')) {
    errors.push('PostgreSQL dashboard qualification module is not registered')
  }

  for (const token of [
    'live_pg18_dashboard_reads_are_tenant_scoped_across_all_aggregate_families',
    'assert_eq!(overview_a.total_devices, 2)',
    'assert_eq!(overview_b.total_devices, 1)',
    'Dashboard Model A',
    'Dashboard Model B',
    'dashboard-warehouse-a',
    'dashboard-warehouse-b',
    '150.0',
    '999.0',
    'INSERT INTO audit_events',
    "'dashboard-audit-a','system','tenant-dashboard-a','order_delete'",
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 dashboard tenant-isolation evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-dashboard-cutover.test.mjs',
    'node scripts/check-r4-p8-dashboard-cutover.mjs',
    'live_pg18_dashboard_reads_are_tenant_scoped_across_all_aggregate_families',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-Z qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDashboardSnapshot(collectDashboardSnapshot())
  if (errors.length) {
    console.error('R4-P8 dashboard cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 dashboard cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
