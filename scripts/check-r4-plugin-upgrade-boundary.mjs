#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-UPGRADE-MIGRATION'

const PATHS = Object.freeze({
  core: 'backend/system/core/src/plugin_upgrade.rs',
  sqlite: 'backend/src/application/plugin_lifecycle_sqlite.rs',
  postgres: 'backend/src/application/plugin_lifecycle_postgres.rs',
  sqliteMigration: 'backend/src/db/migrations/068_r4_plugin_host_security.sql',
  postgresMigration: 'backend/src/db/migrations/postgres/068_r4_plugin_host_security.sql',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireTokens(failures, path, source, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) failures.push(finding(path, `missing required token: ${token}`))
  }
}

export function checkR4PluginUpgradeBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required upgrade migration evidence is missing'))
  }
  if (failures.length) return failures

  const core = files[PATHS.core]
  requireTokens(failures, PATHS.core, core, [
    'pub enum PluginUpgradeMigrationStrategy',
    'PreserveExactPins',
    'migration_strategy: PluginUpgradeMigrationStrategy',
    'self.migration_strategy == PluginUpgradeMigrationStrategy::PreserveExactPins',
    'migration_strategy: PluginUpgradeMigrationStrategy::PreserveExactPins',
  ])

  for (const migrationPath of [PATHS.sqliteMigration, PATHS.postgresMigration]) {
    requireTokens(failures, migrationPath, files[migrationPath], [
      'CREATE TABLE IF NOT EXISTS plugin_upgrade_transitions',
      'candidate_installation_id',
      'previous_executable_json',
      'candidate_identity_json',
      'permission_diff_json',
      'approved_additions_json',
      "migration_strategy = 'preserve_exact_pins'",
    ])
  }

  for (const lifecyclePath of [PATHS.sqlite, PATHS.postgres]) {
    const source = files[lifecyclePath]
    requireTokens(failures, lifecyclePath, source, [
      'if let Some(authorization) = upgrade',
      'INSERT INTO plugin_upgrade_transitions',
      "'preserve_exact_pins'",
      'serde_json::to_string(&authorization.previous_executable)',
      'serde_json::to_string(&candidate.identity)',
      'serde_json::to_string(authorization.review.permission_diff())',
      'serde_json::to_string(authorization.review.approved_additions())',
    ])

    const insertIndex = source.indexOf('INSERT INTO plugin_upgrade_transitions')
    const commitIndex = source.indexOf('tx.commit()', insertIndex)
    if (insertIndex < 0 || commitIndex < 0 || insertIndex > commitIndex) {
      failures.push(finding(lifecyclePath, 'upgrade decision is not persisted before candidate activation transaction commit'))
    }

    if (/UPDATE\s+interconnect_plugin_work_pins/i.test(source)) {
      failures.push(finding(lifecyclePath, 'plugin lifecycle may not retarget existing P3 durable work pins'))
    }
  }

  return failures
}

export function loadR4PluginUpgradeFiles(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const failures = checkR4PluginUpgradeBoundary(loadR4PluginUpgradeFiles(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin upgrade migration boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin upgrade migration boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
