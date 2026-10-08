#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectDepositAuthoritySnapshot,
  validateDepositAuthoritySnapshot,
} from './check-r4-p8-deposit-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDepositAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateDepositAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateDepositAuthoritySnapshot(collectDepositAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'Deposit factory restores direct SQLite construction',
  snapshot => {
    snapshot.factory = replaceRequired(
      snapshot.factory,
      'DepositCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureDeposit)',
      'Deposit factory restores direct SQLite construction',
    )
  },
  'direct SQLite construction',
)

expectFailure(
  'Deposit descriptor restores SQLite requirement',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("deposit", Business, ModuleActivation::Always, NONE, Deposit)',
      'descriptor!("deposit", Business, ModuleActivation::Always, SQLITE, Deposit)',
      'Deposit descriptor restores SQLite requirement',
    )
  },
  'descriptor must not require SQLite',
)

expectFailure(
  'Deposit SQLite device count drifts from serialNo schema',
  snapshot => {
    snapshot.sqlite = replaceRequired(
      snapshot.sqlite,
      'JOIN devices d ON d.serialNo=od.serialNo',
      'JOIN devices d ON d.deviceSerialNo=od.serialNo',
      'Deposit SQLite device count drifts from serialNo schema',
    )
  },
  'devices.serialNo',
)

expectFailure(
  'Deposit PostgreSQL writes lose serialization',
  snapshot => {
    snapshot.pgCollect = replaceRequired(
      snapshot.pgCollect,
      'pg_write_serializable_repository',
      'pg_write',
      'Deposit PostgreSQL writes lose serialization',
    )
  },
  'pg_write_serializable_repository',
)

expectFailure(
  'Deposit PostgreSQL row lock disappears',
  snapshot => {
    snapshot.pgRelease = replaceRequired(
      snapshot.pgRelease,
      'FOR UPDATE',
      'NO_ROW_LOCK',
      'Deposit PostgreSQL row lock disappears',
    )
  },
  'FOR UPDATE',
)

expectFailure(
  'Deposit calculate metadata claims read only',
  snapshot => {
    snapshot.legacy = replaceRequired(
      snapshot.legacy,
      '&[EffectClass::DatabaseRead, EffectClass::DatabaseWrite],\n                SimulationSupport::Blocked',
      '&[EffectClass::DatabaseRead],\n                SimulationSupport::Supported',
      'Deposit calculate metadata claims read only',
    )
  },
  'effect metadata',
)

expectFailure(
  'Deposit live proof loses cross tenant failure',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'Err(DepositMutationError::OrderNotFound)',
      'Err(DepositMutationError::NotFound)',
      'Deposit live proof loses cross tenant failure',
    )
  },
  'OrderNotFound',
)

console.log('R4-P8 Deposit authority mutation tests passed.')
