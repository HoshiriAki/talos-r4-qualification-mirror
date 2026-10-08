#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectConsentCompatibilitySnapshot,
  validateConsentCompatibilitySnapshot,
} from './check-r4-p8-consent-compatibility-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectConsentCompatibilitySnapshot())
  mutate(snapshot)
  const errors = validateConsentCompatibilitySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateConsentCompatibilitySnapshot(collectConsentCompatibilitySnapshot())
assert.deepEqual(baseline, [], `baseline must pass: ${baseline.join('; ')}`)

expectFailure(
  'Production consent falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'ConsentCompatibilityRepository::postgres(pg.clone())',
      'ConsentCompatibilityRepository::new(pool.clone())',
    )
  },
  'ConsentCompatibilityRepository::postgres(pg.clone())',
)

expectFailure(
  'Factory ignores injected consent module',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'let consent_built = match self.consent_module.clone() {\n            Some(module) => constructed(module),',
      'let consent_built = match self.consent_module.clone() {\n            Some(_module) => constructed(with_pool!(FeatureConsent)),',
    )
  },
  'Some(module) => constructed(module)',
)

expectFailure(
  'Factory keeps consent capability token but severs repository binding',
  snapshot => {
    const source = snapshot.factory
    snapshot.factory = snapshot.factory.replace(
      'ConsentCompatibilityRepository::new(\n                    self.require_sqlite_pool("consent compatibility")?,\n                )',
      'ConsentCompatibilityRepository::new(legacy_pool.clone())\n                /* self.require_sqlite_pool("consent compatibility")? */',
    )
    assert.notEqual(snapshot.factory, source, 'consent binding mutation anchor missing')
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'Consent record regains unsafe simulation support',
  snapshot => {
    snapshot.application = snapshot.application.replace(
      '"record",\n                AccessRequirement::Authenticated,\n                &[EffectClass::DatabaseWrite],\n                SimulationSupport::Blocked,',
      '"record",\n                AccessRequirement::Authenticated,\n                &[EffectClass::DatabaseWrite],\n                SimulationSupport::Supported,',
    )
  },
  'Consent record mutation must remain blocked in Simulation',
)

expectFailure(
  'Consent revoke regains unsafe simulation support',
  snapshot => {
    snapshot.application = snapshot.application.replace(
      '"revoke",\n                AccessRequirement::Authenticated,\n                &[EffectClass::DatabaseWrite],\n                SimulationSupport::Blocked,',
      '"revoke",\n                AccessRequirement::Authenticated,\n                &[EffectClass::DatabaseWrite],\n                SimulationSupport::Supported,',
    )
  },
  'Consent revoke mutation must remain blocked in Simulation',
)

expectFailure(
  'Shared consent self-service guard becomes private',
  snapshot => {
    snapshot.systemConsent = snapshot.systemConsent.replace(
      'pub fn require_self_service_actor(',
      'fn require_self_service_actor(',
    )
  },
  'share the actor self-service guard',
)

expectFailure(
  'PostgreSQL consent adapter regains SQLite coupling',
  snapshot => {
    snapshot.postgres += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite runtime types',
)

expectFailure(
  'Live proof stops checking tenant isolation',
  snapshot => {
    snapshot.live = snapshot.live.replace(
      'assert!(!repository.check("tenant-c", &check)?);',
      'assert!(repository.check("tenant-c", &check)?);',
    )
  },
  'assert!(!repository.check("tenant-c", &check)?);',
)

expectFailure(
  'Consent production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Consent gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 consent compatibility cutover mutation tests passed.')
