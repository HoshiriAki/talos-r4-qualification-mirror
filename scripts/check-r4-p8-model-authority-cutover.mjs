#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/device_models.rs',
  authority: 'backend/src/application/model_authority.rs',
  services: 'backend/src/application/services.rs',
  module: 'backend/src/application/model_compatibility.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/model.rs',
  dispatch: 'backend/src/repositories/model_dispatch.rs',
  postgres: 'backend/src/repositories/model_postgres.rs',
  factory: 'backend/src/registry/factory.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/model_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function runtime(source) {
  return source.split('#[cfg(test)]')[0]
}

export function collectModelAuthoritySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateModelAuthoritySnapshot(snapshot) {
  const errors = []
  const route = runtime(snapshot.route)

  for (const forbidden of [
    'model_service::',
    'update_model_full(&state.pool',
    'update_model_pricing(&state.pool',
  ]) {
    if (route.includes(forbidden)) {
      errors.push('model route retains direct SQLite authority: ' + forbidden)
    }
  }
  for (const forbidden of [
    'crate::repositories',
    '.repository_provider()',
    'ModelPatch',
    'ModelPricingPatch',
  ]) {
    if (route.includes(forbidden)) {
      errors.push('model route crossed the SP-08 application boundary: ' + forbidden)
    }
  }
  for (const token of [
    '.application_services()',
    '.model_authority()',
    'ModelAuthorityError',
  ]) {
    if (!route.includes(token)) errors.push('model route application cutover missing: ' + token)
  }

  for (const token of [
    'ModelAuthorityService',
    'repository_provider: Arc<dyn RepositoryProvider>',
    'self.repository_provider.bind(ctx)',
    '.models()',
    'ModelPricingPatch::Clear',
    'ModelPricingPatch::Replace',
  ]) {
    if (!snapshot.authority.includes(token)) {
      errors.push('model application authority missing: ' + token)
    }
  }
  for (const token of [
    'pub fn model_authority(&self) -> ModelAuthorityService',
    'ModelAuthorityService::new(self.repository_provider())',
  ]) {
    if (!snapshot.services.includes(token)) {
      errors.push('ApplicationServices model authority factory missing: ' + token)
    }
  }

  for (const token of [
    'ModelCompatibilityModule',
    'self.repository_provider',
    '.bind(ctx)',
    '.models()',
    'FeatureModel::new().metadata()',
    'FeatureModel::new().commands()',
    'FeatureModel::new().schema()',
  ]) {
    if (!snapshot.module.includes(token)) {
      errors.push('model compatibility module missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn models(&self) -> ScopedModelRepository<'_>")) {
    errors.push('ScopedRepositories must expose the scoped model authority')
  }
  if (!snapshot.dispatch.includes('PostgresModelRepository')
      || !snapshot.dispatch.includes('SqliteModelRepository')) {
    errors.push('model repository backend dispatch is incomplete')
  }

  for (const token of [
    'WHERE dm.tenant_id = ?1',
    'WHERE tenant_id = ?1 AND modelId = ?2',
    'WHERE tenant_id = ?1 AND id = ?2',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite model authority missing tenant boundary: ' + token)
    }
  }

  for (const token of [
    'WHERE dm.tenant_id = $1',
    'WHERE dm.tenant_id = $1 AND dm.id = $2',
    'WHERE tenant_id = $1 AND modelid = $2',
    'WHERE tenant_id = $1 AND id = $2',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL model authority missing tenant boundary: ' + token)
    }
  }

  if (snapshot.factory.includes('let model_concrete = with_pool!(FeatureModel);')) {
    errors.push('ModuleFactory must not construct the SQLite-backed FeatureModel authority')
  }
  for (const token of [
    '(ModelCompatibilityModule, Model, "model")',
    'ModelCompatibilityModule::new(repository_provider.clone())',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('ModuleFactory model authority composition missing: ' + token)
    }
  }

  if (!snapshot.pgMod.includes('mod model_qualification_tests;')) {
    errors.push('PostgreSQL model qualification module is not registered')
  }
  for (const token of [
    'live_pg18_model_authority_preserves_scope_pricing_references_and_recomposition',
    'scoped_b.models().get("model-a")?.is_none()',
    'updated model survives provider recomposition',
    'ModelMutationError::Referenced(1)',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 model authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-model-authority-cutover.test.mjs',
    'node scripts/check-r4-p8-model-authority-cutover.mjs',
    'live_pg18_model_authority_preserves_scope_pricing_references_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-T qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateModelAuthoritySnapshot(collectModelAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 model authority cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 model authority cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
