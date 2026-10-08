#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { checkR4Interconnect } from './check-r4-interconnect-fabric.mjs'

const ROOT = process.cwd()
const paths = {
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
}

function fixture() {
  const files = Object.fromEntries(Object.values(paths).map(path => [path, readFileSync(join(ROOT, path), 'utf8')]))
  return { files, cargoToml: readFileSync(join(ROOT, 'backend/Cargo.toml'), 'utf8') }
}

function expectMutation(mutator, expectedNeedle) {
  const sample = fixture()
  mutator(sample)
  const failures = checkR4Interconnect(sample)
  assert.ok(failures.some(item => `${item.path} ${item.evidence}`.includes(expectedNeedle)), `expected mutation failure containing ${expectedNeedle}, got ${JSON.stringify(failures)}`)
}

assert.deepEqual(checkR4Interconnect(fixture()), [])

expectMutation(sample => {
  sample.files[paths.runtime] = sample.files[paths.runtime].replace('registry.execute(', 'registry_execute_removed(')
}, 'registry.execute(')

expectMutation(sample => {
  sample.files[paths.postgres] = sample.files[paths.postgres].replaceAll('FOR UPDATE SKIP LOCKED', 'FOR UPDATE')
}, 'FOR UPDATE SKIP LOCKED')

expectMutation(sample => {
  sample.files[paths.postgres] = sample.files[paths.postgres].replaceAll('claim_generation', 'claim_epoch_removed')
}, 'claim_generation')

expectMutation(sample => {
  sample.files[paths.postgres] = sample.files[paths.postgres].replaceAll('existing_available', 'existing_availability_removed')
}, 'existing_available')

expectMutation(sample => {
  sample.files[paths.postgres] = sample.files[paths.postgres].replaceAll('enqueue_work_in_transaction', 'enqueue_work_without_tx')
}, 'enqueue_work_in_transaction')

expectMutation(sample => {
  sample.files[paths.postgres] = sample.files[paths.postgres].replaceAll('generic work admission cannot reuse plugin-backed work identity', 'generic/plugin identity guard removed')
}, 'generic work admission cannot reuse plugin-backed work identity')

expectMutation(sample => {
  sample.files[paths.pgMigration] = sample.files[paths.pgMigration].replaceAll('interconnect_outbox', 'removed_outbox')
}, 'interconnect_outbox')

expectMutation(sample => {
  sample.files[paths.pluginAdmission] = sample.files[paths.pluginAdmission].replaceAll('interconnect_plugin_work_pins', 'removed_plugin_pins')
}, 'interconnect_plugin_work_pins')

expectMutation(sample => {
  sample.files[paths.pluginAdmission] = sample.files[paths.pluginAdmission].replace('driver.pool().begin()', 'driver.pool_without_tx()')
}, 'driver.pool().begin()')

expectMutation(sample => {
  sample.files[paths.pluginAdmission] = sample.files[paths.pluginAdmission].replaceAll('work_inserted', 'work_insert_state_removed')
}, 'work_inserted')

expectMutation(sample => {
  sample.files[paths.pluginAdmission] = sample.files[paths.pluginAdmission].replace('provider instance and binding revision together', 'provider pin pair rule removed')
}, 'provider instance and binding revision together')

expectMutation(sample => {
  sample.files[paths.contract] = sample.files[paths.contract].replace('provider instance and binding revision together', 'provider identity pair rule removed')
}, 'provider instance and binding revision together')

expectMutation(sample => {
  sample.files[paths.contract] += '\nstruct SfShipmentPipeline;\n'
}, 'provider-specific Core pipeline')

expectMutation(sample => {
  sample.cargoToml += '\nrdkafka = "0.99"\n'
}, 'broker dependency')

expectMutation(sample => {
  sample.files[paths.runtime] += '\nenum OperationState { UnknownOutcome }\n'
}, 'reimplement R2 ExternalOperation authority')

expectMutation(sample => {
  sample.files[paths.appMod] = sample.files[paths.appMod].replace(
    /^mod interconnect_plugin;$/m,
    'pub mod interconnect_plugin;',
  )
}, 'low-level plugin work admission must remain private')

expectMutation(sample => {
  delete sample.files[paths.migrationExtension]
}, 'required R4-P3 migrationExtension evidence is missing')

expectMutation(sample => {
  delete sample.files[paths.pluginAdmission]
}, 'required R4-P3 pluginAdmission evidence is missing')

console.log('R4-P3 Interconnect structural mutation tests passed.')
