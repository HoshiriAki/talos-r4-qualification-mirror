#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectTenantGovernanceSnapshot,
  validateTenantGovernanceSnapshot,
} from './check-r4-p8-tenant-governance-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectTenantGovernanceSnapshot())
  mutate(snapshot)
  const errors = validateTenantGovernanceSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateTenantGovernanceSnapshot(collectTenantGovernanceSnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Tenant Governance loses platform authority gate',
  snapshot => {
    snapshot.application = replaceRequired(
      snapshot.application,
      'Self::require_platform_authority(ctx)?;',
      '// platform authority gate removed',
      'Tenant Governance loses platform authority gate',
    )
  },
  'require_platform_authority(ctx)?',
)

expectFailure(
  'tenant-scoped RepositoryProvider starts accepting platform scope',
  snapshot => {
    snapshot.binding = replaceRequired(
      snapshot.binding,
      'if data_scope.is_platform() || ctx.tenant_scope().is_platform() {',
      'if false {',
      'tenant-scoped RepositoryProvider starts accepting platform scope',
    )
  },
  'tenant-scoped RepositoryProvider',
)

expectFailure(
  'Tenant Governance factory restores direct Feature SQLite construction',
  snapshot => {
    snapshot.factory += '\n// FeatureTenantGovernance::with_pool(self.pool.clone())\n'
  },
  'direct Feature SQLite construction',
)

expectFailure(
  'Tenant Governance descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tenant_governance" => "feature-tenant-governance", Core, ModuleActivation::Always, NONE, TenantGovernance)',
      'descriptor!("tenant_governance" => "feature-tenant-governance", Core, ModuleActivation::Always, SQLITE, TenantGovernance)',
      'Tenant Governance descriptor restores SQLite storage',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'production composition loses PostgreSQL Governance backend',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'TenantGovernanceRepository::postgres(pg.clone())',
      'TenantGovernanceRepository::new(pool.clone())',
      'production composition loses PostgreSQL Governance backend',
    )
  },
  'backend selection missing',
)

expectFailure(
  'PostgreSQL Governance loses audit compatibility view',
  snapshot => {
    assert.ok(
      snapshot.postgres.includes('FROM audit_logs'),
      'PostgreSQL Governance loses audit compatibility view: mutation anchor missing',
    )
    snapshot.postgres = snapshot.postgres.replaceAll(
      'FROM audit_logs',
      'FROM removed_audit_logs',
    )
  },
  'PostgreSQL Tenant Governance invariant missing',
)

expectFailure(
  'PostgreSQL Governance loses append-only intent write',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'INSERT INTO change_intents',
      'INSERT INTO mutable_intents',
      'PostgreSQL Governance loses append-only intent write',
    )
  },
  'PostgreSQL Tenant Governance invariant missing',
)

expectFailure(
  'Governance audit redaction disappears',
  snapshot => {
    assert.ok(
      snapshot.application.includes('redact_audit_detail'),
      'Governance audit redaction disappears: mutation anchor missing',
    )
    snapshot.application = snapshot.application.replaceAll(
      'redact_audit_detail',
      'removed_audit_redactor',
    )
  },
  'redact_audit_detail',
)

expectFailure(
  'PG18 Governance proof loses append-only trigger check',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      "UPDATE change_intents SET reason='rewritten'",
      "SELECT reason FROM change_intents",
      'PG18 Governance proof loses append-only trigger check',
    )
  },
  'PG18 Tenant Governance evidence missing',
)

console.log('R4-P8 Tenant Governance authority mutation tests passed.')
