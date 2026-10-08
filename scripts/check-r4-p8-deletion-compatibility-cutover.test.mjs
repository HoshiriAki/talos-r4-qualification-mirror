#!/usr/bin/env node
import assert from 'node:assert/strict'
import {
  collectDeletionCompatibilitySnapshot,
  validateDeletionCompatibilitySnapshot,
} from './check-r4-p8-deletion-compatibility-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectDeletionCompatibilitySnapshot())
  mutate(snapshot)
  const errors = validateDeletionCompatibilitySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(errors.some(error => error.includes(needle)), `${name}: expected ${needle}, got ${errors}`)
}
const baseline = validateDeletionCompatibilitySnapshot(collectDeletionCompatibilitySnapshot())
assert.deepEqual(baseline, [], `baseline must pass: ${baseline.join('; ')}`)

expectFailure('production deletion falls back to SQLite', snapshot => {
  snapshot.main = snapshot.main.replace(
    'DeletionCompatibilityRepository::postgres(pg.clone())',
    'DeletionCompatibilityRepository::new(pool.clone())',
  )
}, 'DeletionCompatibilityRepository::postgres(pg.clone())')

expectFailure('PostgreSQL completion loses SERIALIZABLE isolation', snapshot => {
  snapshot.postgres = snapshot.postgres.replace(
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
  )
}, 'SERIALIZABLE')

expectFailure('PostgreSQL deletion anonymization regains SQLite integer boolean', snapshot => {
  snapshot.postgres = snapshot.postgres.replace(
    'totp_enabled = FALSE',
    'totp_enabled = 0',
  )
}, 'totp_enabled = FALSE')

expectFailure('factory ignores injected deletion module', snapshot => {
  snapshot.factory = snapshot.factory.replace(
    'let deletion_built = match self.deletion_module.clone() {\n            Some(module) => constructed(module),',
    'let deletion_built = match self.deletion_module.clone() {\n            Some(_module) => constructed(with_pool!(FeatureDeletion)),',
  )
}, 'Some(module) => constructed(module)')

expectFailure('factory keeps deletion capability token but severs repository binding', snapshot => {
  const source = snapshot.factory
  snapshot.factory = snapshot.factory.replace(
    'DeletionCompatibilityRepository::new(\n                    self.require_sqlite_pool("deletion compatibility")?,\n                )',
    'DeletionCompatibilityRepository::new(legacy_pool.clone())\n                /* self.require_sqlite_pool("deletion compatibility")? */',
  )
  assert.notEqual(snapshot.factory, source, 'deletion binding mutation anchor missing')
}, 'bind the explicit SQLite capability directly')

expectFailure('deletion completion can run in Simulation', snapshot => {
  const start = snapshot.application.indexOf('CommandMetadata::new(\n                "complete",')
  const end = start >= 0 ? start + 280 : 0
  snapshot.application =
    snapshot.application.slice(0, start)
    + snapshot.application.slice(start, end).replace('SimulationSupport::Blocked', 'SimulationSupport::Supported')
    + snapshot.application.slice(end)
}, 'complete mutation must remain blocked in Simulation')

expectFailure('live proof stops checking last-owner safety', snapshot => {
  snapshot.live = snapshot.live.replace(
    'Err(DeletionCompatibilityError::LastOwner)',
    'Err(DeletionCompatibilityError::MembershipNotFound)',
  )
}, 'Err(DeletionCompatibilityError::LastOwner)')

expectFailure(
  'Deletion production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Deletion gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 deletion compatibility cutover mutation tests passed.')
