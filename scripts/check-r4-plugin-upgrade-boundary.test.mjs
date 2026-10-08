#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginUpgradeBoundary,
  loadR4PluginUpgradeFiles,
} from './check-r4-plugin-upgrade-boundary.mjs'

const base = loadR4PluginUpgradeFiles(process.cwd())
assert.deepEqual(
  checkR4PluginUpgradeBoundary(base),
  [],
  'baseline R4-P6 plugin upgrade migration evidence must pass before mutations',
)

function mutate(path, needle, replacement) {
  assert.equal(typeof base[path], 'string', `missing fixture path ${path}`)
  assert.ok(base[path].includes(needle), `mutation needle missing in ${path}: ${needle}`)
  return { ...base, [path]: base[path].replace(needle, replacement) }
}

function expectFailure(name, files, evidence) {
  const failures = checkR4PluginUpgradeBoundary(files)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(evidence)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'upgrade review loses preserve-exact-pins strategy',
  mutate(
    'backend/system/core/src/plugin_upgrade.rs',
    'migration_strategy: PluginUpgradeMigrationStrategy::PreserveExactPins',
    'migration_strategy: unsafe { std::mem::zeroed() }',
  ),
  'migration_strategy: PluginUpgradeMigrationStrategy::PreserveExactPins',
)

expectFailure(
  'transition matching stops checking migration strategy',
  mutate(
    'backend/system/core/src/plugin_upgrade.rs',
    'self.migration_strategy == PluginUpgradeMigrationStrategy::PreserveExactPins',
    'true',
  ),
  'self.migration_strategy == PluginUpgradeMigrationStrategy::PreserveExactPins',
)

expectFailure(
  'postgres transition table strategy widens',
  mutate(
    'backend/src/db/migrations/postgres/068_r4_plugin_host_security.sql',
    "migration_strategy = 'preserve_exact_pins'",
    'migration_strategy IS NOT NULL',
  ),
  "migration_strategy = 'preserve_exact_pins'",
)

expectFailure(
  'sqlite transition table strategy widens',
  mutate(
    'backend/src/db/migrations/068_r4_plugin_host_security.sql',
    "migration_strategy = 'preserve_exact_pins'",
    'migration_strategy IS NOT NULL',
  ),
  "migration_strategy = 'preserve_exact_pins'",
)

for (const path of [
  'backend/src/application/plugin_lifecycle_sqlite.rs',
  'backend/src/application/plugin_lifecycle_postgres.rs',
]) {
  expectFailure(
    `${path} stops recording upgrade transition`,
    mutate(path, 'INSERT INTO plugin_upgrade_transitions', 'INSERT INTO ignored_upgrade_transitions'),
    'INSERT INTO plugin_upgrade_transitions',
  )
  expectFailure(
    `${path} stops recording exact previous executable`,
    mutate(
      path,
      'serde_json::to_string(&authorization.previous_executable)',
      'serde_json::to_string(&candidate.identity)',
    ),
    'serde_json::to_string(&authorization.previous_executable)',
  )
  expectFailure(
    `${path} attempts queued work retarget`,
    {
      ...base,
      [path]: `${base[path]}\nfn forbidden_rebind() { let _ = "UPDATE interconnect_plugin_work_pins SET plugin_version='2.0.0'"; }\n`,
    },
    'may not retarget existing P3 durable work pins',
  )
}

console.log('R4-P6 plugin upgrade migration mutation suite passed.')
