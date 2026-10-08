#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  orderRoute: 'backend/src/routes/orders.rs',
  application: 'backend/src/application/device_candidates.rs',
  services: 'backend/src/application/services.rs',
  sqlite: 'backend/src/repositories/device_candidate.rs',
  dispatch: 'backend/src/repositories/device_candidate_dispatch.rs',
  postgres: 'backend/src/repositories/device_candidate_postgres.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  legacyService: 'backend/src/services/device_service.rs',
  securityGate: 'scripts/check-r4-platform-security-hardening.mjs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/device_candidate_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

export function collectDeviceCandidateCutoverSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateDeviceCandidateCutoverSnapshot(snapshot) {
  const errors = []
  const route = runtime(snapshot.orderRoute)

  if (route.includes('state.pool')) {
    errors.push('order HTTP route must have zero direct AppState SQLite pool dependencies')
  }
  for (const token of [
    'state.application_services().device_candidates()',
    '.list_for_import(&ctx)',
    '.resolve_for_import(&input_serial_no, &device_candidates)',
    'DeviceCandidateQueryError::TooManyCandidates { max }',
  ]) {
    if (!route.includes(token)) errors.push('order device candidate cutover missing: ' + token)
  }
  for (const forbidden of [
    'get_device_match_candidates',
    'device_service::resolve_device_serial_no_for_import',
  ]) {
    if (route.includes(forbidden)) errors.push('legacy device candidate route coupling remains: ' + forbidden)
  }

  for (const token of [
    'repository_provider.bind(ctx)',
    'scoped.device_candidates().list_for_import()',
    'raw_serials.len() > DEVICE_IMPORT_MATCH_CANDIDATES_MAX',
    'normalize_serial_no_for_match',
    'normalized_serial_no.ends_with(&cleaned)',
  ]) {
    if (!snapshot.application.includes(token)) {
      errors.push('device candidate application authority missing: ' + token)
    }
  }

  for (const token of [
    'pub const DEVICE_IMPORT_MATCH_CANDIDATES_MAX: usize = 50_000',
    'WHERE tenant_id = ?1',
    'ORDER BY serialNo',
    'LIMIT ?2',
  ]) {
    if (!snapshot.sqlite.includes(token)) errors.push('SQLite device candidate adapter missing: ' + token)
  }
  for (const token of ['WHERE tenant_id = $1', 'ORDER BY serialNo', 'LIMIT $2']) {
    if (!snapshot.postgres.includes(token)) errors.push('PostgreSQL device candidate adapter missing: ' + token)
  }
  for (const token of [
    'self.scoped.session().is_postgres()',
    'PostgresDeviceCandidateRepository::new',
    'DEVICE_IMPORT_MATCH_CANDIDATES_MAX + 1',
  ]) {
    if (!snapshot.dispatch.includes(token)) errors.push('device candidate dispatch missing: ' + token)
  }
  if (!snapshot.provider.includes('pub fn device_candidates(&self) -> ScopedDeviceCandidateRepository')) {
    errors.push('ScopedRepositories must expose device candidate authority')
  }
  if (!snapshot.services.includes('pub fn device_candidates(&self) -> DeviceCandidateQueryService')) {
    errors.push('ApplicationServices must expose device candidate facade')
  }

  for (const retired of [
    'pub fn get_device_match_candidates',
    'pub fn resolve_device_serial_no_for_import',
    'DEVICE_IMPORT_MATCH_CANDIDATES_MAX',
  ]) {
    if (snapshot.legacyService.includes(retired)) {
      errors.push('legacy device candidate implementation must remain retired: ' + retired)
    }
  }

  for (const token of [
    'deviceCandidateSqlite',
    'deviceCandidatePostgres',
    'authenticated ExecutionContext',
  ]) {
    if (!snapshot.securityGate.includes(token)) {
      errors.push('P7 device candidate security proof was not migrated: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod device_candidate_qualification_tests;')) {
    errors.push('PostgreSQL device candidate qualification module is not registered')
  }
  for (const token of [
    'tenant_a.device_candidates().list_for_import()',
    'tenant_b.device_candidates().list_for_import()',
    'device-request-a-recomposed',
  ]) {
    if (!snapshot.pgTest.includes(token)) errors.push('PG18 device candidate evidence missing: ' + token)
  }

  for (const token of [
    'node scripts/check-r4-p8-device-candidate-cutover.test.mjs',
    'node scripts/check-r4-p8-device-candidate-cutover.mjs',
    'live_pg18_device_candidate_cutover_preserves_tenant_scope_order_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push('Exact-head P8-Q qualification missing: ' + token)
  }

  return errors
}

function main() {
  const errors = validateDeviceCandidateCutoverSnapshot(collectDeviceCandidateCutoverSnapshot())
  if (errors.length) {
    console.error('R4-P8 device candidate cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 device candidate cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
