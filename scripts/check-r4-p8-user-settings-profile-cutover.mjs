#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/user_settings.rs',
  module: 'backend/src/application/user_settings_compatibility.rs',
  sqlite: 'backend/src/repositories/user_settings_profile.rs',
  dispatch: 'backend/src/repositories/user_settings_profile_dispatch.rs',
  postgres: 'backend/src/repositories/user_settings_profile_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  error: 'backend/src/error.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/user_settings_profile_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

export function collectUserSettingsProfileSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateUserSettingsProfileSnapshot(snapshot) {
  const errors = []
  const route = runtime(snapshot.route)

  for (const token of [
    'make_ctx(&auth.0, state.http_client.clone())',
    '"user_settings"',
    '"get_settings"',
    '"save_settings"',
    'AppError::from_error_payload',
  ]) {
    if (!route.includes(token)) errors.push('user settings route cutover missing: ' + token)
  }
  for (const forbidden of ['state.pool', 'user_settings_service', 'rusqlite']) {
    if (route.includes(forbidden)) {
      errors.push('user settings route must not retain SQLite/service coupling: ' + forbidden)
    }
  }

  for (const token of [
    'UserSettingsCompatibilityModule',
    '.user_id()',
    'actor != requested',
    'AUTH_USER_SETTINGS_SELF_ONLY',
    'SimulationSupport::Blocked',
    '.get(user_id)',
    '.save(user_id',
  ]) {
    if (!snapshot.module.includes(token)) errors.push('identity-profile module missing: ' + token)
  }

  for (const [name, source] of [
    ['SQLite', snapshot.sqlite],
    ['PostgreSQL', snapshot.postgres],
  ]) {
    for (const token of ['user_settings', 'userId', 'settingsJson', 'updatedAt']) {
      if (!source.includes(token)) errors.push(name + ' user settings adapter missing: ' + token)
    }
    if (source.includes('tenant_id')) {
      errors.push(name + ' identity-profile adapter must not use legacy tenant_id as authority')
    }
  }
  if (!snapshot.postgres.includes('ON CONFLICT (userId) DO UPDATE')) {
    errors.push('PostgreSQL user settings adapter must preserve identity-keyed upsert')
  }
  if (!snapshot.dispatch.includes('UserSettingsProfileBackend')
      || !snapshot.dispatch.includes('PostgresUserSettingsProfileRepository')) {
    errors.push('user settings backend dispatch is incomplete')
  }

  for (const token of [
    'UserSettingsCompatibilityModule',
    'with_user_settings_module',
  ]) {
    if (!snapshot.factory.includes(token)) errors.push('user settings factory composition missing: ' + token)
  }
  if (!/UserSettingsProfileRepository::new\(\s*self\.require_sqlite_pool\(\s*"user settings compatibility"\s*\)\?\s*,?\s*\)/s.test(snapshot.factory)) {
    errors.push('user settings factory must bind the explicit SQLite capability directly to UserSettingsProfileRepository')
  }
  if (snapshot.factory.includes('FeatureUserSettings')) {
    errors.push('placeholder FeatureUserSettings must not remain in factory construction')
  }
  if (!snapshot.assembler.includes('.with_user_settings_module(user_settings_module)')) {
    errors.push('Registry assembler must inject user settings compatibility module')
  }

  for (const token of [
    'UserSettingsProfileRepository::postgres(pg.clone())',
    'UserSettingsProfileRepository::new(pool.clone())',
    'UserSettingsCompatibilityModule::new(user_settings_profile_repository)',
    'user_settings_module,',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push('production composition root missing user settings authority: ' + token)
    }
  }

  for (const token of [
    '"AUTH_USER_SETTINGS_SELF_ONLY"',
    'AppError::Forbidden',
    '"VAL_USER_SETTINGS_INPUT"',
    'AppError::BadRequest(message)',
  ]) {
    if (!snapshot.error.includes(token)) errors.push('HTTP error compatibility missing: ' + token)
  }

  if (!snapshot.pgMod.includes('mod user_settings_profile_qualification_tests;')) {
    errors.push('PostgreSQL user settings qualification module is not registered')
  }
  for (const token of [
    'legacy_tenant_marker.is_none()',
    'another identity must not inherit the first identity profile',
    'save must remain an identity-keyed upsert',
  ]) {
    if (!snapshot.pgTest.includes(token)) errors.push('PG18 identity-profile evidence missing: ' + token)
  }

  for (const token of [
    'node scripts/check-r4-p8-user-settings-profile-cutover.test.mjs',
    'node scripts/check-r4-p8-user-settings-profile-cutover.mjs',
    'live_pg18_user_settings_profile_preserves_identity_global_upsert_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push('Exact-head P8-P qualification missing: ' + token)
  }

  return errors
}

function main() {
  const errors = validateUserSettingsProfileSnapshot(collectUserSettingsProfileSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 user settings profile cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 user settings profile cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
