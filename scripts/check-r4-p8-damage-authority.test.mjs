#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDamageAuthoritySnapshot,
  validateDamageAuthoritySnapshot,
} from './check-r4-p8-damage-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDamageAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateDamageAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDamageAuthoritySnapshot(collectDamageAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Damage factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'DamageCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureDamage)',
      'Damage factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Damage descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("damage", Business, ModuleActivation::Always, NONE, Damage)',
      'descriptor!("damage", Business, ModuleActivation::Always, SQLITE, Damage)',
      'Damage descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Damage report loses serialization',
  snapshot => {
    snapshot.pgReport = replaceRequired(
      snapshot.pgReport,
      'pg_write_serializable_repository',
      'pg_write',
      'Damage report loses serialization',
    )
  },
  'report invariant missing',
)

expectFailure(
  'Damage transition loses row lock',
  snapshot => {
    snapshot.pgTransition = replaceRequired(
      snapshot.pgTransition,
      'FOR UPDATE',
      'NO_ROW_LOCK',
      'Damage transition loses row lock',
    )
  },
  'transition invariant missing',
)

expectFailure(
  'Damage reads lose tenant scope',
  snapshot => {
    assert.ok(
      snapshot.pgRead.includes('tenant_id='),
      'Damage reads lose tenant scope: mutation anchor missing',
    )
    snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
  },
  'tenant-scoped',
)

expectFailure(
  'Damage live proof loses cross tenant rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'Err(DamageMutationError::NotFound)',
      'Err(DamageMutationError::ResourceNotFound)',
      'Damage live proof loses cross tenant rejection',
    )
  },
  'NotFound',
)

console.log('R4-P8 Damage authority mutation tests passed.')
