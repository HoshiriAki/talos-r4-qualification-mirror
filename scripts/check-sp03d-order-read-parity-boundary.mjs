#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

import {
  discoverChangedPaths,
  runtimeSource,
} from './check-sp08-scoped-repository-boundary.mjs'

const RULE = 'TALOS-OPS-021'
const APP_MOD_PATH = 'backend/src/application/mod.rs'
const PARITY_TEST_PATH = 'backend/src/application/order_read_parity_tests.rs'
const PACKAGE_PATH = 'package.json'
const DOC_PATH = 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/sp-03d-order-read-parity-contract.md'
const GENERATED_CI_LOGS = new Set([
  'documentation-link-check.log',
  'repository-layout-check.log',
  'talos-ops-boundary-check.log',
])

function failure(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

export function checkSp03dOrderReadParityBoundary(
  files,
  changedPaths = [],
  { enforceSliceScope = false } = {},
) {
  const normalized = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), source]),
  )
  const runtime = Object.fromEntries(
    Object.entries(normalized).map(([path, source]) => [path, runtimeSource(source)]),
  )
  const failures = []

  for (const path of [APP_MOD_PATH, PARITY_TEST_PATH, PACKAGE_PATH, DOC_PATH]) {
    if (!Object.hasOwn(normalized, path)) failures.push(failure(path, 'required SP-03D evidence is missing'))
  }
  if (failures.length > 0) return failures

  const application = normalized[APP_MOD_PATH]
  const parity = runtime[PARITY_TEST_PATH]
  const packageJson = normalized[PACKAGE_PATH]
  const doc = normalized[DOC_PATH]

  if (!/#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*mod\s+order_read_parity_tests\s*;/.test(application)) {
    failures.push(failure(APP_MOD_PATH, 'parity harness must remain test-only under cfg(test)'))
  }

  if (!/use\s+official_order::FeatureOrder/.test(parity)
      || !/use\s+crate::application::OrderQueryService/.test(parity)
      || !/SystemModule/.test(parity)) {
    failures.push(failure(PARITY_TEST_PATH, 'parity tests must compare the real official module and real application query service'))
  }

  const sharedDatabase = /fn\s+repository_pool\s*\(\s*database_path\s*:\s*&Path/.test(parity)
    && /SqliteConnectionManager::file\(database_path\)/.test(parity)
    && /module\s*\.\s*init\s*\(\s*json!\s*\(\s*\{[\s\S]*?"databaseUrl"\s*:\s*database_path\.to_string_lossy\(\)\.as_ref\(\)/.test(parity)
    && /SqliteRepositoryProvider::new\(\s*repository_pool\.clone\(\)/.test(parity)
    && /fn\s+context\([^)]*tenant/.test(parity)
  if (!sharedDatabase) {
    failures.push(failure(PARITY_TEST_PATH, 'both paths must use the same SQLite database and one tenant ExecutionContext contract'))
  }
  if (/\.pool\s*\.\s*lock\s*\(/.test(parity)) {
    failures.push(failure(PARITY_TEST_PATH, 'official Order must be initialized through SystemModule::init rather than direct pool mutation'))
  }

  if (!/\.execute\(\s*"list_orders"/.test(parity)
      || !/\.execute\(\s*"get_order"/.test(parity)
      || !/\.list\(ctx,/.test(parity)
      || !/\.get_by_id\(ctx,/.test(parity)) {
    failures.push(failure(PARITY_TEST_PATH, 'parity evidence must execute both list and get on both read paths'))
  }

  for (const testName of [
    'projection_fields_match_after_explicit_status_normalization',
    'tenant_scope_remains_equivalent_across_both_read_paths',
    'status_filter_requires_an_explicit_compatibility_adapter',
    'extended_filter_surface_blocks_caller_cutover',
    'not_found_error_contract_blocks_caller_cutover',
    'response_envelope_requires_an_explicit_compatibility_adapter',
    'caller_cutover_remains_blocked_until_all_named_gaps_close',
  ]) {
    if (!new RegExp(`fn\\s+${testName}\\s*\\(`).test(parity)) {
      failures.push(failure(PARITY_TEST_PATH, `missing executable parity case: ${testName}`))
    }
  }

  if (!/official_order::state_machine::display_label/.test(parity)
      || !/"已付款"/.test(parity)
      || !/"paid"/.test(parity)) {
    failures.push(failure(PARITY_TEST_PATH, 'status parity must cover official display mapping and raw persisted values'))
  }

  const officialOnlyFilters = [
    'address',
    'province',
    'startDateFrom',
    'startDateTo',
    'endDateFrom',
    'endDateTo',
    'includedDate',
    'deliveryDateFrom',
    'deliveryDateTo',
    'deliveryDate',
    'pickupMethods',
    'startDate',
    'endDate',
    'serialNo',
    'trackingNo',
  ]
  for (const filter of officialOnlyFilters) {
    if (!parity.includes(`"${filter}"`)) {
      failures.push(failure(PARITY_TEST_PATH, `extended filter inventory must include ${filter}`))
    }
  }

  if (!/BIZ_ORDER_NOT_FOUND/.test(parity)
      || !/repository\.is_none\(\)/.test(parity)) {
    failures.push(failure(PARITY_TEST_PATH, 'not-found parity must record official typed error versus repository absence'))
  }

  for (const gap of [
    'response-envelope',
    'status-normalization',
    'extended-filter-surface',
    'not-found-error-contract',
    'registry-caller-semantics',
  ]) {
    if (!parity.includes(`"${gap}"`)) {
      failures.push(failure(PARITY_TEST_PATH, `named cutover gap is missing: ${gap}`))
    }
  }

  if (!doc.includes('NOT_READY_FOR_CALLER_CUTOVER')) {
    failures.push(failure(DOC_PATH, 'historical assessment decision must remain explicit'))
  }
  for (const heading of [
    'Response envelope',
    'Status normalization',
    'Extended filter surface',
    'Not-found and error contract',
    'Registry caller semantics',
  ]) {
    if (!doc.includes(heading)) {
      failures.push(failure(DOC_PATH, `parity report is missing blocker section: ${heading}`))
    }
  }
  if (!doc.includes('SP-03E Order Read Compatibility Adapter')) {
    failures.push(failure(DOC_PATH, 'historical next boundary must remain the compatibility adapter'))
  }

  if (!packageJson.includes('quality:talos-ops:order-parity:test')
      || !packageJson.includes('quality:talos-ops:order-parity')
      || !packageJson.includes('check-sp03d-order-read-parity-boundary.test.mjs')
      || !packageJson.includes('check-sp03d-order-read-parity-boundary.mjs')) {
    failures.push(failure(PACKAGE_PATH, 'SP-03D checker and fixture commands must be registered'))
  }

  for (const rawPath of changedPaths) {
    const path = normalizePath(rawPath)
    if (path.startsWith('GIT_CHANGED_PATH_DISCOVERY_FAILED:')) {
      failures.push(failure('git', 'changed-path discovery failed closed'))
      continue
    }
    if (!enforceSliceScope || GENERATED_CI_LOGS.has(path)) continue
    const allowed = path === APP_MOD_PATH
      || path === PARITY_TEST_PATH
      || path === PACKAGE_PATH
      || path === 'policy/qualification/legacy-evidence/docs/README.md'
      || path === DOC_PATH
      || path === 'scripts/check-sp03d-order-read-parity-boundary.mjs'
      || path === 'scripts/check-sp03d-order-read-parity-boundary.test.mjs'
    if (!allowed) {
      failures.push(failure(path, 'SP-03D changed paths exceed the authorized test/Gate/docs scope'))
    }
  }

  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of [APP_MOD_PATH, PARITY_TEST_PATH, PACKAGE_PATH, DOC_PATH]) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const root = process.cwd()
  const failures = checkSp03dOrderReadParityBoundary(
    loadFiles(root),
    discoverChangedPaths(root),
  )
  if (failures.length > 0) {
    console.error('SP-03D Order read parity boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-03D Order read parity boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
