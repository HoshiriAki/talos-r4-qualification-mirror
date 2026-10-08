#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
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

export function collectRegistryRepositoryProviderSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateRegistryRepositoryProviderSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'repository_provider: Option<Arc<dyn RepositoryProvider>>',
    'pub(crate) fn with_repository_provider(',
    'self.repository_provider = Some(repository_provider);',
    'self.require_sqlite_pool("repository provider")?',
    'SqliteRepositoryProvider::new_with_metrics(',
    'CustomerModule::new(repository_provider.clone())',
    'OrderLifecycleV2Module::new(repository_provider.clone())',
    'OrderQueryV2Module::new(repository_provider.clone())',
    'OrderReadCompatibilityModule::new(repository_provider.clone())',
    'ReservationV2Module::new(repository_provider.clone())',
    'R3SettlementModule::new(r3_repository_provider)',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push(`Registry factory repository-provider invariant missing: ${token}`)
    }
  }

  const productionAssembler = between(
    snapshot.assembler,
    'pub(crate) fn assemble_with_metrics_audit_sink_integration_and_staff_module(',
    '\n    }\n}',
  )
  for (const token of [
    'repository_provider: Arc<dyn RepositoryProvider>',
    '.with_repository_provider(repository_provider)',
  ]) {
    if (!productionAssembler.includes(token)) {
      errors.push(`Production Registry assembler provider injection missing: ${token}`)
    }
  }

  const registryCall = between(
    snapshot.main,
    'crate::registry::ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(',
    '.expect("ModuleRegistry assembly failed")',
  )
  if (!registryCall.includes('repository_provider.clone()')) {
    errors.push('Production Registry composition must pass the composition-root RepositoryProvider')
  }

  for (const token of [
    'PostgresRepositoryProvider::new_with_metrics(',
    'SqliteRepositoryProvider::new_with_metrics(',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Composition-root RepositoryProvider selection missing: ${token}`)
    }
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Registry provider cutover must not restore the transitional production barrier')
  }

  for (const token of [
    'node scripts/check-r4-p8-registry-repository-provider-cutover.test.mjs',
    'node scripts/check-r4-p8-registry-repository-provider-cutover.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push(`Exact-head Registry provider qualification missing: ${token}`)
    }
  }

  return errors
}

function main() {
  const errors = validateRegistryRepositoryProviderSnapshot(
    collectRegistryRepositoryProviderSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Registry RepositoryProvider cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Registry RepositoryProvider cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
