#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectModuleFactorySqliteOptionalSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateModuleFactorySqliteOptionalSnapshot(snapshot) {
  const errors = []
  const factory = compact(snapshot.factory)
  const assembler = compact(snapshot.assembler)
  const main = compact(snapshot.main)

  for (const token of [
    'pool:Option<Pool<SqliteConnectionManager>>',
    'pub(crate)fnnew_without_sqlite(',
    'pool:None',
    'fnrequire_sqlite_pool(',
    'CFG_SQLITE_CAPABILITY_REQUIRED',
    'sqlite_free_factory_fails_closed_when_legacy_authority_is_not_injected',
    'sqlite_free_factory_builds_with_explicit_authorities',
  ]) {
    if (!factory.includes(token)) {
      errors.push('ModuleFactory optional-SQLite invariant missing: ' + token)
    }
  }

  if (snapshot.factory.includes('macro_rules! with_pool')) {
    errors.push('ModuleFactory must not retain the dead direct-pool injection macro')
  }

  const selfPoolClones = snapshot.factory.match(/self\.pool\.clone\(\)/g) ?? []
  if (selfPoolClones.length !== 1) {
    errors.push(
      'ModuleFactory may reference self.pool.clone() only inside require_sqlite_pool helper',
    )
  }

  for (const token of [
    'sqlite_pool:Option<Pool<SqliteConnectionManager>>',
    'Some(pool)=>ModuleFactory::new(pool,http_client,provider_config)',
    'None=>ModuleFactory::new_without_sqlite(http_client,provider_config)',
  ]) {
    if (!assembler.includes(token)) {
      errors.push('Registry assembler optional-SQLite root missing: ' + token)
    }
  }

  if (!main.includes(
    'ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(sqlite_pool.clone(),',
  )) {
    errors.push('Runtime must pass the explicitly selected optional SQLite capability to Registry assembly')
  }

  for (const token of [
    'node scripts/check-r4-p8-module-factory-sqlite-optional.test.mjs',
    'node scripts/check-r4-p8-module-factory-sqlite-optional.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head module-factory optional-SQLite qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateModuleFactorySqliteOptionalSnapshot(
    collectModuleFactorySqliteOptionalSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 ModuleFactory optional-SQLite gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 ModuleFactory optional-SQLite gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
