#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectNotifyAuthoritySnapshot,
  validateNotifyAuthoritySnapshot,
} from './check-r4-p8-notify-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectNotifyAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateNotifyAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateNotifyAuthoritySnapshot(collectNotifyAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Notify factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'NotifyCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureNotification)',
      'Notify factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Notify descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("notify", Business, ModuleActivation::Always, NONE, Notify)',
      'descriptor!("notify", Business, ModuleActivation::Always, SQLITE, Notify)',
      'Notify descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Notify PG reads lose tenant scope',
  snapshot => {
    assert.ok(snapshot.pgRead.includes('tenant_id=$1'), 'tenant read mutation anchor missing')
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=$1', 'tenant_scope_removed=$1')
  },
  'read invariant missing',
)

expectFailure(
  'Notify PG writes lose serialization',
  snapshot => {
    assert.ok(
      snapshot.pgMutation.includes('pg_write_serializable_repository'),
      'Notify PG writes lose serialization: mutation anchor missing',
    )
    snapshot.pgMutation = snapshot.pgMutation.replaceAll(
      'pg_write_serializable_repository',
      'pg_write',
    )
  },
  'write invariant missing',
)

expectFailure(
  'Notify live proof loses cross-tenant template rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'Err(NotificationMutationError::TemplateNotFound)',
      'Err(NotificationMutationError::Storage(_))',
      'Notify live proof loses cross-tenant template rejection',
    )
  },
  'TemplateNotFound',
)

expectFailure(
  'Notify live proof loses cross-tenant order hydration rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      '.order_context("notify-order-b")?\n                .is_none()',
      '.order_context("notify-order-b")?\n                .is_some()',
      'Notify live proof loses cross-tenant order hydration rejection',
    )
  },
  'cross-tenant order hydration rejection',
)

console.log('R4-P8 Notify authority mutation tests passed.')
