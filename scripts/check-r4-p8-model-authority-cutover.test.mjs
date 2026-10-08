#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectModelAuthoritySnapshot,
  validateModelAuthoritySnapshot,
} from './check-r4-p8-model-authority-cutover.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replaceAll(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectModelAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateModelAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateModelAuthoritySnapshot(collectModelAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'model route restores legacy model service',
  snapshot => {
    snapshot.route += '\n// model_service::update_model_full(&state.pool, scope, id, body, actor)\n'
  },
  'model_service::',
)

expectFailure(
  'model route bypasses application authority for repository provider',
  snapshot => {
    snapshot.route += '\nfn regression(state: &crate::state::AppState) { let _ = state.application_services().repository_provider(); }\n'
  },
  '.repository_provider()',
)

expectFailure(
  'factory restores FeatureModel SQLite authority',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'ModelCompatibilityModule::new(repository_provider.clone())',
      'with_pool!(FeatureModel)',
    )
  },
  'ModelCompatibilityModule::new(repository_provider.clone())',
)

expectFailure(
  'provider hides scoped model authority',
  snapshot => {
    snapshot.provider = replaceRequired(
      snapshot.provider,
      "pub fn models(&self) -> ScopedModelRepository<'_>",
      "fn removed_models(&self) -> ScopedModelRepository<'_>",
      'provider hides scoped model authority',
    )
  },
  'ScopedRepositories must expose',
)

expectFailure(
  'PostgreSQL point read loses tenant scope',
  snapshot => {
    snapshot.postgres = replaceRequired(
      snapshot.postgres,
      'WHERE dm.tenant_id = $1 AND dm.id = $2',
      'WHERE dm.id = $2',
      'PostgreSQL point read loses tenant scope',
    )
  },
  'WHERE dm.tenant_id = $1 AND dm.id = $2',
)

expectFailure(
  'PG18 proof loses referenced-delete rejection',
  snapshot => {
    snapshot.pgTest = replaceRequired(
      snapshot.pgTest,
      'ModelMutationError::Referenced(1)',
      'ModelMutationError::NotFound',
      'PG18 proof loses referenced-delete rejection',
    )
  },
  'ModelMutationError::Referenced(1)',
)

console.log('R4-P8 model authority cutover mutation tests passed.')
