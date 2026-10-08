#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
  metrics: 'backend/src/routes/metrics.rs',
  tenantPreview: 'backend/src/routes/tenant_preview.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectAppStateSqliteOptionalSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateAppStateSqliteOptionalSnapshot(snapshot) {
  const errors = []
  const state = compact(snapshot.state)
  const main = compact(snapshot.main)
  const metrics = compact(snapshot.metrics)
  const tenantPreview = compact(snapshot.tenantPreview)

  for (const token of [
    'pub(crate)structAppStateRepositories',
    'pub(crate)fnnew(',
    'pub(crate)fnnew_local(',
    'letrepositories=AppStateRepositories::local(&pool)',
  ]) {
    if (!state.includes(token)) {
      errors.push('AppState optional-SQLite invariant missing: ' + token)
    }
  }

  if (state.includes('pubpool:Pool<SqliteConnectionManager>')) {
    errors.push('AppState must not expose or retain a raw SQLite pool field')
  }

  const appStateImpl = state.indexOf('implAppState{')
  const neutralStart = state.indexOf('pub(crate)fnnew(', appStateImpl)
  const localStart = state.indexOf('pub(crate)fnnew_local(', appStateImpl)
  if (appStateImpl < 0 || neutralStart < 0 || localStart < 0 || neutralStart >= localStart) {
    errors.push('AppState backend-neutral/local constructor ordering is invalid')
  } else {
    const neutralConstructor = state.slice(neutralStart, localStart)
    if (neutralConstructor.includes('Pool<SqliteConnectionManager>')) {
      errors.push('AppState backend-neutral constructor must not accept a SQLite pool')
    }
    if (neutralConstructor.includes('Repository::new(pool')
        || neutralConstructor.includes('Repository::new(&pool')) {
      errors.push('AppState backend-neutral constructor must not manufacture SQLite repositories')
    }
  }

  for (const forbidden of [
    'set_audit_compatibility_repository',
    'set_auth_repositories',
    'set_machine_authority_repository',
    'set_platform_membership_repository',
    'set_platform_tenant_repository',
    'set_tenant_resolution_repository',
  ]) {
    if (snapshot.state.includes(forbidden) || snapshot.main.includes(forbidden)) {
      errors.push('AppState must not rely on post-construction backend override: ' + forbidden)
    }
  }

  for (const token of [
    'letapp_state_repositories=AppStateRepositories::new(',
    'letmutstate=AppState::new(',
  ]) {
    if (!main.includes(token)) {
      errors.push('main AppState backend-neutral composition missing: ' + token)
    }
  }

  if (main.includes('AppState::new_local(')) {
    errors.push('main must not use the SQLite-local AppState constructor')
  }

  if (!metrics.includes('AppState::new_local(')) {
    errors.push('SQLite route fixture must use the explicit AppState local constructor')
  }

  if (tenantPreview.includes('state.pool') || tenantPreview.includes('state.pool()')) {
    errors.push('tenant-preview route must not access a raw SQLite pool through AppState')
  }
  for (const token of [
    'state.registry.execute("tenant_simulation","session.get"',
    'session.get("baseRevision")',
    'simulation_revision.as_deref()',
  ]) {
    if (!tenantPreview.includes(token)) {
      errors.push('tenant-preview simulation authority derivation missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-appstate-sqlite-optional.test.mjs',
    'node scripts/check-r4-p8-appstate-sqlite-optional.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head AppState optional-SQLite qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateAppStateSqliteOptionalSnapshot(collectAppStateSqliteOptionalSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 AppState optional-SQLite gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 AppState optional-SQLite gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
