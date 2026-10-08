#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  orders: 'backend/src/routes/orders.rs',
  auditRoute: 'backend/src/routes/audit.rs',
  auditService: 'backend/src/services/audit_service.rs',
  auditRepository: 'backend/src/repositories/audit_compatibility.rs',
  auditDispatch: 'backend/src/repositories/audit_compatibility_dispatch.rs',
  auditPostgres: 'backend/src/repositories/audit_compatibility_postgres.rs',
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
  pgTest: 'backend/src/repositories/postgres/audit_compatibility_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

function count(source, token) {
  return source.split(token).length - 1
}

export function collectOrderHttpResidualSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateOrderHttpResidualSnapshot(snapshot) {
  const errors = []
  const orders = runtime(snapshot.orders)
  const auditRoute = runtime(snapshot.auditRoute)

  if (count(orders, 'state.pool') !== 0) {
    errors.push('order route must not retain any direct AppState SQLite pool dependency')
  }
  for (const forbidden of [
    'audit_service::write_audit_log(',
    'order_compatibility_support::find_order_by_id',
    'rusqlite::',
    'SELECT id, orderNo FROM orders',
  ]) {
    if (orders.includes(forbidden)) {
      errors.push('order route residual SQLite closure regressed: ' + forbidden)
    }
  }
  for (const token of [
    'audit_service::write_audit_log_with_repository',
    'state.audit_compatibility_repository()',
    '.order_queries()',
    '.get_by_id(&ctx, &id)',
  ]) {
    if (!orders.includes(token)) errors.push('order route cutover missing: ' + token)
  }
  if (count(orders, 'audit_service::write_audit_log_with_repository') < 12) {
    errors.push('order route must preserve all established semantic audit call sites')
  }

  for (const forbidden of ['state.pool', 'audit_service::query_audit_logs']) {
    if (auditRoute.includes(forbidden)) {
      errors.push('audit HTTP route must not retain direct SQLite read coupling: ' + forbidden)
    }
  }
  if (count(auditRoute, '"audit"') < 2 || count(auditRoute, '"list_audit_logs"') < 2) {
    errors.push('audit HTTP list/export must both use Registry audit authority')
  }
  if (!auditRoute.includes('"detail": detail')) {
    errors.push('audit HTTP list response must preserve parsed detail compatibility')
  }

  for (const token of [
    'fn audit_authority_snapshot',
    'pub fn write_audit_log_with_repository',
    'append_authority_event',
    'safe_detail_json(&detail)',
  ]) {
    if (!snapshot.auditService.includes(token)) {
      errors.push('authority-aware audit service missing: ' + token)
    }
  }

  for (const token of ['AuditAuthorityWrite', 'append_authority_event', 'INSERT INTO audit_events']) {
    if (!snapshot.auditRepository.includes(token)) {
      errors.push('SQLite authority audit repository missing: ' + token)
    }
    if (token !== 'AuditAuthorityWrite' && !snapshot.auditPostgres.includes(token)) {
      errors.push('PostgreSQL authority audit repository missing: ' + token)
    }
  }
  for (const token of ['roles_snapshot', 'capabilities_snapshot', 'tenant_membership_id']) {
    if (!snapshot.auditPostgres.includes(token)) {
      errors.push('PostgreSQL authority audit provenance missing: ' + token)
    }
  }
  if (!snapshot.auditDispatch.includes('append_authority_event')) {
    errors.push('audit backend dispatch does not expose authority-aware append')
  }

  for (const token of [
    'pub(crate) struct AppStateRepositories',
    'audit_compatibility_repository: AuditCompatibilityRepository',
    'pub(crate) fn audit_compatibility_repository(&self) -> &AuditCompatibilityRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push('AppState selected audit authority missing: ' + token)
    }
  }
  if (snapshot.state.includes('set_audit_compatibility_repository')) {
    errors.push('AppState selected audit authority must not restore post-construction override')
  }
  if (!snapshot.main.includes(
    'AuditCompatibilityModule::new(audit_compatibility_repository.clone())',
  )) {
    errors.push(
      'composition-root audit authority missing: AuditCompatibilityModule::new(audit_compatibility_repository.clone())',
    )
  }
  if (!/AppStateRepositories::new\(\s*audit_compatibility_repository,/.test(snapshot.main)) {
    errors.push('composition-root AppState bundle must consume the selected audit authority')
  }

  for (const token of [
    'AuditAuthorityWrite',
    'membership-tenant-audit',
    'repository.append_authority_event',
    'assert_eq!(authority.0, "tenant")',
    'assert_eq!(authority.3.as_deref(), Some("membership-tenant-audit"))',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('live PG18 authority audit evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-order-http-residual-cutover.test.mjs',
    'node scripts/check-r4-p8-order-http-residual-cutover.mjs',
    'live_pg18_audit_compatibility_preserves_legacy_view_and_canonical_store',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-O qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateOrderHttpResidualSnapshot(collectOrderHttpResidualSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 order HTTP residual cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 order HTTP residual cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
