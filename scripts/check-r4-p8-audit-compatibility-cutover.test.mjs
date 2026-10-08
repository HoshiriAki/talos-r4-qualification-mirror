#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectAuditCompatibilitySnapshot,
  validateAuditCompatibilitySnapshot,
} from './check-r4-p8-audit-compatibility-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectAuditCompatibilitySnapshot())
  mutate(snapshot)
  const errors = validateAuditCompatibilitySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateAuditCompatibilitySnapshot(collectAuditCompatibilitySnapshot())
assert.deepEqual(baseline, [], `baseline must pass: ${baseline.join('; ')}`)

expectFailure(
  'Production audit falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'AuditCompatibilityRepository::postgres(pg.clone())',
      'AuditCompatibilityRepository::new(pool.clone())',
    )
  },
  'AuditCompatibilityRepository::postgres(pg.clone())',
)

expectFailure(
  'Production audit module stops sharing the selected repository with AppState',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'AuditCompatibilityModule::new(audit_compatibility_repository.clone())',
      'AuditCompatibilityModule::new(audit_compatibility_repository)',
    )
  },
  'AuditCompatibilityModule::new(audit_compatibility_repository.clone())',
)

expectFailure(
  'AppState stops retaining the selected audit authority',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,/,
      'AppStateRepositories::new(AuditCompatibilityRepository::new(pool.clone()),',
    )
  },
  'Audit AppState bundle must consume the selected Audit repository',
)

expectFailure(
  'Factory ignores injected audit module',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'let audit_built = match self.audit_module.clone() {\n            Some(module) => constructed(module),',
      'let audit_built = match self.audit_module.clone() {\n            Some(_module) => constructed(with_pool!(FeatureAudit)),',
    )
  },
  'Some(module) => constructed(module)',
)

expectFailure(
  'PostgreSQL audit adapter regains SQLite coupling',
  snapshot => {
    snapshot.postgres += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite runtime types',
)

expectFailure(
  'Shared audit redaction helper becomes private',
  snapshot => {
    snapshot.systemAudit = snapshot.systemAudit.replace(
      'pub fn safe_audit_detail_json(',
      'fn safe_audit_detail_json(',
    )
  },
  'share the redaction helper',
)

expectFailure(
  'Live proof stops checking canonical audit_events write-through',
  snapshot => {
    snapshot.live = snapshot.live.replace(
      'SELECT authority_kind, action, resource_type',
      'SELECT action, resource_type, resource_id',
    )
  },
  'SELECT authority_kind, action, resource_type',
)

expectFailure(
  'Audit PostgreSQL qualification module is no longer registered',
  snapshot => {
    snapshot.pgMod = snapshot.pgMod.replace(
      'mod audit_compatibility_qualification_tests;',
      '// audit compatibility qualification module removed',
    )
  },
  'Audit PostgreSQL live qualification wiring missing',
)

expectFailure(
  'Audit production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Audit gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 audit compatibility cutover mutation tests passed.')
