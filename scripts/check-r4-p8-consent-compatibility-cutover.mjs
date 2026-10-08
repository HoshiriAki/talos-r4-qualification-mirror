#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const PATHS = {
  sqlite: 'backend/src/repositories/consent_compatibility.rs',
  dispatch: 'backend/src/repositories/consent_compatibility_dispatch.rs',
  postgres: 'backend/src/repositories/consent_compatibility_postgres.rs',
  application: 'backend/src/application/consent_compatibility.rs',
  systemConsent: 'backend/system/admin/src/consent.rs',
  factory: 'backend/src/registry/factory.rs',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  live: 'backend/src/repositories/postgres/consent_compatibility_qualification_tests.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
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

export function collectConsentCompatibilitySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateConsentCompatibilitySnapshot(snapshot) {
  const errors = []

  for (const token of [
    'SqliteConsentCompatibilityRepository',
    'INSERT INTO privacy_consents',
    'SELECT EXISTS(',
    'UPDATE privacy_consents',
    'WHERE tenant_id = ?1 AND user_id = ?2',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push(`SQLite consent compatibility missing: ${token}`)
    }
  }

  for (const token of [
    'enum ConsentCompatibilityBackend',
    'Postgres(PostgresConsentCompatibilityRepository)',
    'pub(crate) struct ConsentCompatibilityRepository',
  ]) {
    if (!snapshot.dispatch.includes(token)) errors.push(`Consent dispatch missing: ${token}`)
  }

  for (const token of [
    'PostgresConsentCompatibilityRepository',
    'INSERT INTO privacy_consents',
    'SELECT EXISTS(',
    'UPDATE privacy_consents',
    'WHERE tenant_id = $1 AND user_id = $2',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push(`PostgreSQL consent compatibility missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgres)) {
    errors.push('PostgreSQL consent compatibility must not depend on SQLite runtime types')
  }

  for (const token of [
    'pub(crate) struct ConsentCompatibilityModule',
    'require_self_service_actor(ctx.actor().id(), &input.user_id)',
    '"record"',
    '"check"',
    '"revoke"',
    '"audit"',
  ]) {
    if (!snapshot.application.includes(token)) {
      errors.push(`Consent compatibility module missing: ${token}`)
    }
  }

  const commandMetadata = between(
    snapshot.application,
    'fn commands(&self) -> Vec<CommandMetadata> {',
    '\n    fn schema(&self) -> ModuleSchema',
  )
  const recordMetadata = between(
    commandMetadata,
    'CommandMetadata::new(\n                "record",',
    'CommandMetadata::new(\n                "check",',
  )
  const revokeMetadata = between(
    commandMetadata,
    'CommandMetadata::new(\n                "revoke",',
    'CommandMetadata::new(\n                "audit",',
  )
  if (!recordMetadata.includes('SimulationSupport::Blocked')) {
    errors.push('Consent record mutation must remain blocked in Simulation')
  }
  if (!revokeMetadata.includes('SimulationSupport::Blocked')) {
    errors.push('Consent revoke mutation must remain blocked in Simulation')
  }
  if (!snapshot.systemConsent.includes('pub fn require_self_service_actor(')) {
    errors.push('Legacy and compatibility consent paths must share the actor self-service guard')
  }

  for (const token of [
    'consent_module: Option<ConsentCompatibilityModule>',
    '(ConsentCompatibilityModule, Consent, "consent")',
    'pub(crate) fn with_consent_module(',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push(`Consent factory composition missing: ${token}`)
    }
  }
  const consentFactoryBlock = between(
    snapshot.factory,
    'let consent_built = match self.consent_module.clone() {',
    '\n        let deletion_built',
  )
  for (const token of [
    'Some(module) => constructed(module)',
    'None => constructed(ConsentCompatibilityModule::new(',
  ]) {
    if (!consentFactoryBlock.includes(token)) {
      errors.push(`Consent factory runtime selection missing: ${token}`)
    }
  }
  if (!/ConsentCompatibilityRepository::new\(\s*self\.require_sqlite_pool\(\s*"consent compatibility"\s*\)\?\s*,?\s*\)/s.test(consentFactoryBlock)) {
    errors.push('Consent factory must bind the explicit SQLite capability directly to ConsentCompatibilityRepository')
  }

  for (const token of [
    'consent_module: ConsentCompatibilityModule',
    '.with_consent_module(consent_module)',
  ]) {
    if (!snapshot.assembler.includes(token)) {
      errors.push(`Consent assembler composition missing: ${token}`)
    }
  }

  for (const token of [
    'ConsentCompatibilityRepository::postgres(pg.clone())',
    'ConsentCompatibilityRepository::new(pool.clone())',
    'ConsentCompatibilityModule::new(consent_compatibility_repository)',
    'consent_module,',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Consent production composition missing: ${token}`)
    }
  }
  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Consent production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Consent cutover must not restore the retired transitional production barrier')
  }

  if (!snapshot.pgMod.includes('mod consent_compatibility_qualification_tests;')) {
    errors.push('Consent PostgreSQL live qualification wiring missing')
  }
  for (const token of [
    'live_pg18_consent_compatibility_preserves_scope_record_check_revoke_and_audit',
    'assert!(!repository.check("tenant-c", &check)?);',
    'assert!(!repository.check("tenant-a", &check)?);',
    'assert!(repository.check("tenant-b", &check)?);',
    'assert_eq!(audit_a[0].revoked_at.as_deref(), Some("03"));',
  ]) {
    if (!snapshot.live.includes(token)) {
      errors.push(`Consent PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-consent-compatibility-cutover.test.mjs',
    'node scripts/check-r4-p8-consent-compatibility-cutover.mjs',
    'live_pg18_consent_compatibility_preserves_scope_record_check_revoke_and_audit',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push(`Exact-head consent qualification missing: ${token}`)
    }
  }

  return errors
}

function main() {
  const errors = validateConsentCompatibilitySnapshot(collectConsentCompatibilitySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 consent compatibility cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 consent compatibility cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
