#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P3-INTERCONNECT-STABLE-CORE'

const PATHS = Object.freeze({
  contract: 'backend/system/core/src/transport/interconnect.rs',
  transportMod: 'backend/system/core/src/transport/mod.rs',
  runtime: 'backend/src/application/interconnect/mod.rs',
  postgres: 'backend/src/application/interconnect/postgres.rs',
  driver: 'backend/src/application/interconnect_driver.rs',
  facets: 'backend/src/application/interconnect_facets.rs',
  pluginAdmission: 'backend/src/application/interconnect_plugin.rs',
  appMod: 'backend/src/application/mod.rs',
  sqliteMigration: 'backend/src/db/migrations/067_r4_interconnect_fabric.sql',
  pgMigration: 'backend/src/db/migrations/postgres/067_r4_interconnect_fabric.sql',
  dbMod: 'backend/src/db/mod.rs',
  migrationExtension: 'backend/src/db/r4_migrations.rs',
  main: 'backend/src/main.rs',
})

const REQUIRED = Object.freeze({
  contract: [
    'pub struct MessageEnvelope',
    'pub struct Subject',
    'pub struct ContentRef',
    'pub struct PluginExecutableRef',
    'pub enum TransportCapability',
    'pub struct DriverDescriptor',
    'CapabilityUnavailable',
    'provider instance and binding revision together',
  ],
  runtime: [
    'ModuleRegistry',
    'registry.execute(',
    'StaleClaimGeneration',
    'UnknownOutcome',
    'InProcessDriver',
  ],
  postgres: [
    'FOR UPDATE SKIP LOCKED',
    'claim_generation',
    'interconnect_outbox',
    'append_event_tx',
    'decode_envelope',
    'run_all_pg_migrations',
    'enqueue_work_in_transaction',
    'existing_available',
    'generic work admission cannot reuse plugin-backed work identity',
  ],
  driver: ['pub trait InterconnectDriver', 'require_driver_capabilities'],
  facets: ['FROZEN_EXTENSION_FACETS', 'KafkaCompatibleLog', 'VerifiedConsensusInterop'],
  pluginAdmission: [
    'pub struct PluginWorkAdmission',
    'PluginExecutableRef',
    'InProcessPluginWorkQueue',
    'enqueue_postgres_plugin_work',
    'interconnect_plugin_work_pins',
    'different executable/package identity',
    'provider instance and binding revision together',
    'driver.pool().begin()',
    'enqueue_work_in_transaction',
    'work_inserted',
    'FOR UPDATE',
  ],
  dbMod: ['run_all_sqlite_migrations', 'run_all_pg_migrations'],
  migrationExtension: ['067_r4_interconnect_fabric', 'schema_migrations'],
  main: ['run_all_sqlite_migrations', 'run_all_pg_migrations'],
  sqliteMigration: ['interconnect_events', 'interconnect_work', 'interconnect_plugin_work_pins', 'interconnect_state_heads', 'interconnect_outbox'],
  pgMigration: ['interconnect_events', 'interconnect_work', 'interconnect_plugin_work_pins', 'interconnect_state_heads', 'interconnect_outbox'],
})

const FORBIDDEN_PROVIDER_CORE = [
  /struct\s+Sf\w*Pipeline\b/,
  /struct\s+Wechat\w*Pipeline\b/,
  /struct\s+Alipay\w*Pipeline\b/,
  /mod\s+sf[_a-zA-Z0-9]*pipeline\b/,
  /mod\s+wechat[_a-zA-Z0-9]*pipeline\b/,
  /mod\s+alipay[_a-zA-Z0-9]*pipeline\b/,
]

const FORBIDDEN_BROKER_DEPENDENCIES = [
  /^\s*(rdkafka|async-nats|pulsar|lapin|redis|zenoh|rumqttc|dust-dds)\s*=/m,
]

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

export function checkR4Interconnect({ files, cargoToml }) {
  const failures = []
  for (const [key, path] of Object.entries(PATHS)) {
    if (!Object.hasOwn(files, path)) {
      failures.push(finding(path, `required R4-P3 ${key} evidence is missing`))
    }
  }
  if (failures.length) return failures

  for (const [key, tokens] of Object.entries(REQUIRED)) {
    const path = PATHS[key]
    const source = files[path]
    for (const token of tokens) {
      if (!source.includes(token)) failures.push(finding(path, `missing required token: ${token}`))
    }
  }

  if (!files[PATHS.transportMod].includes('pub mod interconnect;')) {
    failures.push(finding(PATHS.transportMod, 'Interconnect contract must be compiled by system-core'))
  }
  for (const token of [
    'pub mod interconnect;',
    'pub mod interconnect_driver;',
    'pub mod interconnect_facets;',
  ]) {
    if (!files[PATHS.appMod].includes(token)) failures.push(finding(PATHS.appMod, `application module missing ${token}`))
  }

  // R4-P3 freezes the durable plugin-work pinning implementation, not a public
  // bypass around later permission policy. R4-P6 composes that implementation
  // behind PluginHostOperations, so the low-level module must remain compiled
  // while becoming crate-private.
  if (!/^\s*mod\s+interconnect_plugin;\s*$/m.test(files[PATHS.appMod])) {
    failures.push(finding(PATHS.appMod, 'R4-P3 plugin work admission module must remain compiled as a private application module'))
  }
  if (/^\s*pub\s+mod\s+interconnect_plugin;\s*$/m.test(files[PATHS.appMod])) {
    failures.push(finding(PATHS.appMod, 'R4-P3 low-level plugin work admission must remain private behind the R4-P6 permission facade'))
  }

  const coreSources = [files[PATHS.contract], files[PATHS.runtime], files[PATHS.driver]].join('\n')
  for (const pattern of FORBIDDEN_PROVIDER_CORE) {
    if (pattern.test(coreSources)) failures.push(finding('backend/{system/core,src/application}', `provider-specific Core pipeline matched ${pattern}`))
  }
  for (const pattern of FORBIDDEN_BROKER_DEPENDENCIES) {
    if (pattern.test(cargoToml)) failures.push(finding('backend/Cargo.toml', `R4-P3 extension facet pulled a broker dependency into stable core: ${pattern}`))
  }

  if (files[PATHS.runtime].includes('ExternalOperation {') || files[PATHS.runtime].includes('enum OperationState')) {
    failures.push(finding(PATHS.runtime, 'Interconnect must not reimplement R2 ExternalOperation authority'))
  }

  return failures
}

function loadRepository(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return { files, cargoToml: readFileSync(join(root, 'backend/Cargo.toml'), 'utf8') }
}

function main() {
  const failures = checkR4Interconnect(loadRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P3 Interconnect stable-core check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P3 Interconnect stable-core check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
