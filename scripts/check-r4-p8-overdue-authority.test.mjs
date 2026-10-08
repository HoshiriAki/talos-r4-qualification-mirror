#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectOverdueAuthoritySnapshot,
  validateOverdueAuthoritySnapshot,
} from './check-r4-p8-overdue-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectOverdueAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateOverdueAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateOverdueAuthoritySnapshot(collectOverdueAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('Overdue factory restores SQLite', snapshot => {
  snapshot.factory = replaceRequired(
    snapshot.factory,
    'OverdueCompatibilityModule::new(repository_provider.clone())',
    'with_pool!(FeatureOverdue)',
    'Overdue factory restores SQLite',
  )
}, 'direct SQLite construction')

expectFailure('Overdue descriptor restores SQLite', snapshot => {
  snapshot.descriptors = replaceRequired(
    snapshot.descriptors,
    'descriptor!("overdue", Business, ModuleActivation::Always, NONE, Overdue)',
    'descriptor!("overdue", Business, ModuleActivation::Always, SQLITE, Overdue)',
    'Overdue descriptor restores SQLite',
  )
}, 'descriptor must not require SQLite')

expectFailure('Overdue financial delegation regresses', snapshot => {
  snapshot.application = replaceRequired(
    snapshot.application,
    'R3 settlement owns additional-charge effects',
    'legacy module owns payment writes',
    'Overdue financial delegation regresses',
  )
}, 'R3 settlement financial delegation')

expectFailure('Overdue detect loses advisory lock', snapshot => {
  snapshot.pgMutation = replaceRequired(
    snapshot.pgMutation,
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'overdue_lock_removed($1)',
    'Overdue detect loses advisory lock',
  )
}, 'mutation invariant missing')

expectFailure('Overdue mutations lose serialization', snapshot => {
  snapshot.pgMutation = snapshot.pgMutation.replaceAll(
    'pg_write_serializable_repository',
    'pg_write',
  )
}, 'mutation invariant missing')

expectFailure('Overdue reads lose tenant scope', snapshot => {
  snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
}, 'read invariant missing')

expectFailure('Overdue reads lose REAL normalization', snapshot => {
  snapshot.pgRead = snapshot.pgRead.replaceAll(
    'total_fee::double precision AS total_fee',
    'total_fee',
  )
}, 'read invariant missing')

expectFailure('Overdue migration drops notification tenant column repair', snapshot => {
  snapshot.pgMigration = replaceRequired(
    snapshot.pgMigration,
    'ALTER TABLE overdue_notification_log\n    ADD COLUMN IF NOT EXISTS tenant_id TEXT;',
    '-- notification tenant column repair removed',
    'Overdue migration drops notification tenant column repair',
  )
}, 'tenant invariant migration missing')

expectFailure('Overdue migration restores non-tenant notification uniqueness', snapshot => {
  snapshot.pgMigration = replaceRequired(
    snapshot.pgMigration,
    'ON overdue_notification_log(tenant_id, overdue_id, escalation_level)',
    'ON overdue_notification_log(overdue_id, escalation_level)',
    'Overdue migration restores non-tenant notification uniqueness',
  )
}, 'tenant invariant migration missing')

expectFailure('Overdue live proof loses waiver identity parent', snapshot => {
  snapshot.pgTest = replaceRequired(
    snapshot.pgTest,
    'INSERT INTO identities',
    'INSERT INTO removed_identities',
    'Overdue live proof loses waiver identity parent',
  )
}, 'PG18 Overdue authority evidence missing')

expectFailure('Overdue live proof bypasses order tenant boundary', snapshot => {
  snapshot.pgTest = replaceRequired(
    snapshot.pgTest,
    "scoped_order.tenant_id='tenant-overdue-a'",
    "price.tenant_id='tenant-overdue-a'",
    'Overdue live proof bypasses order tenant boundary',
  )
}, 'PG18 Overdue authority evidence missing')

expectFailure('Overdue live proof loses delegated financial boundary', snapshot => {
  snapshot.pgTest = replaceRequired(
    snapshot.pgTest,
    'delegated["financialEffectApplied"], false',
    'delegated["financialEffectApplied"], true',
    'Overdue live proof loses delegated financial boundary',
  )
}, 'financialEffectApplied')

console.log('R4-P8 Overdue authority mutation tests passed.')
