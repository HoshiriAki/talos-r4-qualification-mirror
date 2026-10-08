#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-RUNTIME-COMPOSITION'

const PATHS = Object.freeze({
  runtime: 'backend/src/application/plugin_runtime_services.rs',
  admissionService: 'backend/src/application/plugin_execution_admission_service.rs',
  appMod: 'backend/src/application/mod.rs',
  services: 'backend/src/application/services.rs',
  main: 'backend/src/main.rs',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function block(source, startToken) {
  const start = source.indexOf(startToken)
  if (start < 0) return ''
  let depth = 0
  let opened = false
  for (let index = start; index < source.length; index += 1) {
    if (source[index] === '{') {
      opened = true
      depth += 1
    } else if (source[index] === '}') {
      depth -= 1
      if (opened && depth === 0) return source.slice(start, index + 1)
    }
  }
  return source.slice(start)
}

function requireTokens(failures, path, source, tokens, scope = '') {
  for (const token of tokens) {
    if (!source.includes(token)) {
      const prefix = scope ? `${scope} ` : ''
      failures.push(finding(path, `${prefix}missing required token: ${token}`))
    }
  }
}

export function checkR4PluginRuntimeComposition({ files }) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required runtime composition evidence is missing'))
  }
  if (failures.length) return failures

  const runtime = files[PATHS.runtime]
  const sqlite = block(runtime, 'pub(crate) fn sqlite(')
  const postgres = block(runtime, 'pub(crate) fn postgres(')

  if (!runtime.includes('pub(crate) struct PluginRuntimeSecurityProfile')) {
    failures.push(finding(PATHS.runtime, 'runtime security profile is not host-composition-owned'))
  }
  if (!runtime.includes('PluginEgressBudgetRegistry::new()')) {
    failures.push(finding(PATHS.runtime, 'runtime no longer creates one host-owned shared egress budget registry'))
  }

  requireTokens(failures, PATHS.runtime, sqlite, [
    'fabric: Arc<InterconnectFabric>',
    'let runtime_binding = PluginExecutionRuntimeBinding::new();',
    'PluginHostOperations::development(',
    'runtime_binding.clone()',
    'SqlitePluginAdmissionService::new(',
    'SqlitePluginExecutionStorageService::new(',
  ], 'sqlite runtime')
  if (sqlite.includes('operations: Arc<PluginHostOperations>')) {
    failures.push(finding(PATHS.runtime, 'sqlite runtime accepts caller-selected plugin operations'))
  }

  requireTokens(failures, PATHS.runtime, postgres, [
    'fabric: Arc<InterconnectFabric>',
    'let runtime_binding = PluginExecutionRuntimeBinding::new();',
    'PluginHostOperations::postgres(',
    'PostgresDurableDriver::new(pool.clone())',
    'PostgresPluginAdmissionService::new(',
    'PostgresPluginExecutionStorageService::new(',
    'PostgresPluginBackgroundExecutionGate::new_bound(',
    'runtime_binding.clone()',
  ], 'postgres runtime')
  if (postgres.includes('operations: Arc<PluginHostOperations>')) {
    failures.push(finding(PATHS.runtime, 'postgres runtime accepts caller-selected plugin operations'))
  }
  if (/\bpub\s+fn\s+(?:sqlite|postgres)\s*\(/.test(runtime)) {
    failures.push(finding(PATHS.runtime, 'runtime persistence constructors became externally public'))
  }

  const admissionService = files[PATHS.admissionService]
  for (const typeName of ['SqlitePluginAdmissionService', 'PostgresPluginAdmissionService']) {
    if (!admissionService.includes(`pub(crate) struct ${typeName}`)) {
      failures.push(finding(PATHS.admissionService, `${typeName} is not crate-private`))
    }
    if (admissionService.includes(`pub struct ${typeName}`)) {
      failures.push(finding(PATHS.admissionService, `${typeName} became an externally public partial admission surface`))
    }
  }

  const sqliteAdmission = block(admissionService, 'impl SqlitePluginAdmissionService')
  const postgresAdmission = block(admissionService, 'impl PostgresPluginAdmissionService')
  for (const [scope, adapter] of [
    ['sqlite admission', sqliteAdmission],
    ['postgres admission', postgresAdmission],
  ]) {
    requireTokens(failures, PATHS.admissionService, adapter, [
      'runtime_binding: PluginExecutionRuntimeBinding',
      'self.runtime_binding.clone()',
      'PluginExecutionAdmission::new(',
    ], scope)
  }
  if (/\bpub\s+(?:async\s+)?fn\s+(?:new|admit)\s*\(/.test(admissionService)) {
    failures.push(finding(PATHS.admissionService, 'partial admission constructor or admit method became externally public'))
  }

  const appMod = files[PATHS.appMod]
  if (appMod.includes('pub use plugin_execution_admission_service::')) {
    failures.push(finding(PATHS.appMod, 'partial admission service was publicly re-exported'))
  }
  if (!appMod.includes('pub use plugin_runtime_services::PluginRuntimeServices;')) {
    failures.push(finding(PATHS.appMod, 'complete plugin runtime is not the application-facing execution type'))
  }

  // SP-06/SP-08 freeze ApplicationServices as the exact four-handle aggregate.
  // P6 must not attach runtime state there merely because the runtime is complete.
  const services = files[PATHS.services]
  for (const forbidden of [
    'PluginRuntimeServices',
    'PluginHostOperations',
    'plugin_runtime',
    'with_plugin_runtime',
    'plugin_host_operations',
    'with_plugin_host_operations',
  ]) {
    if (services.includes(forbidden)) {
      failures.push(finding(PATHS.services, `frozen ApplicationServices absorbed plugin runtime surface: ${forbidden}`))
    }
  }

  const main = files[PATHS.main]
  if (!main.includes('ApplicationServices::production(')) {
    failures.push(finding(PATHS.main, 'default executable no longer uses the frozen ApplicationServices production composition'))
  }
  for (const forbidden of [
    'PluginRuntimeServices::sqlite(',
    'PluginRuntimeServices::postgres(',
    '.with_plugin_runtime(',
    '.with_plugin_host_operations(',
  ]) {
    if (main.includes(forbidden)) {
      failures.push(finding(PATHS.main, `default executable mounted plugin runtime without a reviewed deployment profile: ${forbidden}`))
    }
  }

  return failures
}

export function loadRepository(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return { files }
}

function main() {
  const failures = checkR4PluginRuntimeComposition(loadRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin runtime composition boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin runtime composition boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
