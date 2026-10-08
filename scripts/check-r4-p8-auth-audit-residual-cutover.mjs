#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  authRoute: 'backend/src/routes/auth.rs',
  auditService: 'backend/src/services/audit_service.rs',
  state: 'backend/src/state.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

function count(source, token) {
  return source.split(token).length - 1
}

export function collectAuthAuditResidualSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateAuthAuditResidualSnapshot(snapshot) {
  const errors = []
  const auth = runtime(snapshot.authRoute)

  if (count(auth, 'state.pool') !== 0) {
    errors.push('auth route must not retain direct AppState SQLite pool dependency')
  }
  if (auth.includes('audit_service::write_audit_log(')) {
    errors.push('auth route must not use legacy SQLite audit writer')
  }
  if (count(auth, 'audit_service::write_audit_log_with_repository') !== 3) {
    errors.push('auth route must preserve exactly three semantic audit call sites')
  }
  if (count(auth, 'state.audit_compatibility_repository()') !== 3) {
    errors.push('auth route audit writes must use the selected audit authority handle')
  }
  for (const token of ['"auth_login"', '"auth_logout"', '"profile_update"']) {
    if (!auth.includes(token)) errors.push('auth audit contract missing: ' + token)
  }

  for (const token of [
    'pub fn write_audit_log_with_repository',
    'append_authority_event',
    'audit_authority_snapshot',
  ]) {
    if (!snapshot.auditService.includes(token)) {
      errors.push('authority-aware audit service missing: ' + token)
    }
  }
  for (const token of [
    'pub(crate) fn audit_compatibility_repository(&self) -> &AuditCompatibilityRepository',
  ]) {
    if (!snapshot.state.includes(token)) {
      errors.push('selected audit authority handle missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-auth-audit-residual-cutover.test.mjs',
    'node scripts/check-r4-p8-auth-audit-residual-cutover.mjs',
    'live_pg18_audit_compatibility_preserves_legacy_view_and_canonical_store',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-R qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateAuthAuditResidualSnapshot(collectAuthAuditResidualSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 auth audit residual cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 auth audit residual cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
