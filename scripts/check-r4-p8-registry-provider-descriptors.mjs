#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectRegistryProviderDescriptorSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateRegistryProviderDescriptorSnapshot(snapshot) {
  const errors = []
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'descriptor!("customer",Business,ModuleActivation::Always,NONE,Customer)',
    'descriptor!("order_lifecycle_v2",Business,ModuleActivation::Always,NONE,OrderLifecycleV2)',
    'descriptor!("order_query_v2",Business,ModuleActivation::Always,NONE,OrderQueryV2)',
    'descriptor!("order_read_compatibility",Business,ModuleActivation::Always,NONE,OrderReadCompatibility)',
    'descriptor!("reservation_v2",Business,ModuleActivation::Always,NONE,ReservationV2)',
    'descriptor!("r3_settlement",Business,ModuleActivation::Always,NONE,R3Settlement)',
  ]) {
    if (!descriptors.includes(token)) {
      errors.push('provider-backed Registry descriptor requirement missing: ' + token)
    }
  }

  for (const token of [
    'CustomerModule::new(repository_provider.clone())',
    'OrderLifecycleV2Module::new(repository_provider.clone())',
    'OrderQueryV2Module::new(repository_provider.clone())',
    'OrderReadCompatibilityModule::new(repository_provider.clone())',
    'ReservationV2Module::new(repository_provider.clone())',
    'R3SettlementModule::new(r3_repository_provider)',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('provider-backed Registry factory proof missing: ' + token)
    }
  }

  for (const forbidden of [
    'with_pool!(FeatureCustomer)',
    'with_pool!(FeatureOrderLifecycleV2)',
    'with_pool!(FeatureOrderQueryV2)',
    'with_pool!(FeatureOrderReadCompatibility)',
    'with_pool!(FeatureReservationV2)',
    'with_pool!(FeatureR3Settlement)',
  ]) {
    if (snapshot.factory.includes(forbidden)) {
      errors.push('provider-backed Registry runtime restored SQLite construction: ' + forbidden)
    }
  }

  if (snapshot.descriptors.includes('const R3_SETTLEMENT: &[ModuleRequirement] = &[ModuleRequirement::SqlitePool];')) {
    errors.push('r3_settlement descriptor retains the retired SQLite requirement constant')
  }

  for (const token of [
    'node scripts/check-r4-p8-registry-provider-descriptors.test.mjs',
    'node scripts/check-r4-p8-registry-provider-descriptors.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-AD qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateRegistryProviderDescriptorSnapshot(
    collectRegistryProviderDescriptorSnapshot(),
  )
  if (errors.length) {
    console.error('R4-P8 provider-backed Registry descriptor gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 provider-backed Registry descriptor gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
