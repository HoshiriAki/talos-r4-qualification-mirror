#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8IntegrationCatalogSnapshot,
  validateP8IntegrationCatalogSnapshot,
} from './check-r4-p8-integration-catalog-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8IntegrationCatalogSnapshot())
  mutate(snapshot)
  const errors = validateP8IntegrationCatalogSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8IntegrationCatalogSnapshot(
  collectP8IntegrationCatalogSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Integration Catalog mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'PostgreSQL manifest persistence stops rejecting duplicate versions',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'ON CONFLICT (provider_id, version) DO NOTHING',
      'ON CONFLICT (provider_id, version) DO UPDATE SET readiness=EXCLUDED.readiness',
    )
  },
  'ON CONFLICT (provider_id, version) DO NOTHING',
)

expectFailure(
  'PostgreSQL binding persistence loses serializable transaction',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL binding persistence loses row lock',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'FOR UPDATE',
      'REMOVED_BINDING_LOCK',
    )
  },
  'FOR UPDATE',
)

expectFailure(
  'PostgreSQL binding lookup loses tenant predicate',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'WHERE tenant_id=$1 AND id=$2',
      'WHERE id=$2',
    )
  },
  'tenant_id=$1',
)

expectFailure(
  'PostgreSQL binding history persistence is removed',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'provider_binding_history',
      'REMOVED_BINDING_HISTORY',
    )
  },
  'provider_binding_history',
)

expectFailure(
  'PostgreSQL Catalog gains SQLite coupling',
  snapshot => {
    snapshot.postgresRepository += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite',
)

expectFailure(
  'IntegrationStore stops validating instance against manifest',
  snapshot => {
    snapshot.store = snapshot.store.replaceAll(
      'validate_instance_against_manifest(instance, &manifest)?',
      '/* removed manifest admission */',
    )
  },
  'validate_instance_against_manifest',
)

expectFailure(
  'IntegrationStore stops enforcing KeyStore reference admission',
  snapshot => {
    snapshot.store = snapshot.store.replaceAll(
      '.accepts_reference(',
      '.accepts_reference_REMOVED(',
    )
  },
  '.accepts_reference(',
)

expectFailure(
  'Live proof stops checking stale revision rejection',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(stale, IntegrationError::BindingRevisionStale);',
      'assert!(true);',
    )
  },
  'BindingRevisionStale',
)

expectFailure(
  'Live proof stops checking tenant-local duplicate identities',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'instance("tenant-b", "rev-b1")',
      'instance("tenant-z", "rev-b1")',
    )
  },
  'tenant-b',
)

expectFailure(
  'Live proof stops checking cross-tenant binding rejection',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(wrong_tenant, IntegrationError::BindingUnavailable);',
      'assert!(true);',
    )
  },
  'wrong_tenant',
)

expectFailure(
  'Live proof stops checking history rollback on rejected writes',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(unchanged_history, 1);',
      'assert!(true);',
    )
  },
  'unchanged_history',
)

expectFailure(
  'Live proof stops checking repository recomposition',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'let recomposed =',
      'let removed_recomposition =',
    )
  },
  'let recomposed =',
)

expectFailure(
  'Partial Catalog parity silently changes IntegrationStore composition',
  snapshot => {
    snapshot.store = snapshot.store.replaceAll(
      'catalog_repository: IntegrationCatalogRepository::new(pool.clone())',
      'catalog_repository: IntegrationCatalogRepository::postgres(pg_pool.clone())',
    )
  },
  'must keep the existing SQLite composition explicit',
)

expectFailure(
  'Production IntegrationModule falls back to SQLite catalog authority',
  snapshot => {
    snapshot.main = snapshot.main.replaceAll(
      'IntegrationModule::new_with_postgres_persistence(',
      'IntegrationModule::new(integration_store.clone()) /* removed PG composition */',
    )
  },
  'Production IntegrationModule composition missing',
)

expectFailure(
  'IntegrationModule bypasses catalog authority through IntegrationStore',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'self.catalog.save_manifest(&manifest)?',
      'self.store.save_manifest(&manifest)?',
    )
  },
  'must not bypass composed persistence through IntegrationStore',
)

expectFailure(
  'Registry factory drops explicit IntegrationModule injection',
  snapshot => {
    snapshot.registryFactory = snapshot.registryFactory.replaceAll(
      'with_integration_module(',
      'removed_integration_module_injection(',
    )
  },
  'Registry factory IntegrationModule injection missing',
)

expectFailure(
  'Registry factory keeps capability token but severs IntegrationStore binding',
  snapshot => {
    const source = snapshot.registryFactory
    snapshot.registryFactory = snapshot.registryFactory.replace(
      'IntegrationStore::new(\n                    self.require_sqlite_pool("integration compatibility store")?,\n                )',
      'IntegrationStore::new(legacy_pool.clone())\n                /* self.require_sqlite_pool("integration compatibility store")? */',
    )
    assert.notEqual(snapshot.registryFactory, source, 'integration factory binding mutation anchor missing')
  },
  'bind the explicit SQLite capability directly to IntegrationStore',
)

expectFailure(
  'Local IntegrationStore keeps capability token but severs constructor binding',
  snapshot => {
    const source = snapshot.main
    snapshot.main = snapshot.main.replace(
      'IntegrationStore::with_key_store_and_metrics(\n            require_sqlite_pool("integration compatibility store")?,',
      'IntegrationStore::with_key_store_and_metrics(\n            legacy_pool.clone(),\n            /* require_sqlite_pool("integration compatibility store")? */',
    )
    assert.notEqual(snapshot.main, source, 'integration local binding mutation anchor missing')
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'Registry assembler drops composed IntegrationModule',
  snapshot => {
    snapshot.registryAssembler = snapshot.registryAssembler.replaceAll(
      '.with_integration_module(integration_module)',
      '.with_integration_store(integration_store.clone())',
    )
  },
  'Registry assembler IntegrationModule injection missing',
)

expectFailure(
  'Production Registry assembler regains SQLite IntegrationStore parameter',
  snapshot => {
    snapshot.registryAssembler = snapshot.registryAssembler.replace(
      'pub(crate) fn assemble_with_metrics_audit_sink_integration_and_staff_module(\n        sqlite_pool: Option<Pool<SqliteConnectionManager>>,\n        http_client: Arc<dyn HttpClient>,',
      'pub(crate) fn assemble_with_metrics_audit_sink_integration_and_staff_module(\n        sqlite_pool: Option<Pool<SqliteConnectionManager>>,\n        http_client: Arc<dyn HttpClient>,\n        integration_store: IntegrationStore,',
    )
  },
  'must not accept SQLite IntegrationStore',
)

expectFailure(
  'Production Registry call passes SQLite IntegrationStore again',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'http_client.clone(),\n            integration_module,',
      'http_client.clone(),\n            integration_store.clone(),\n            integration_module,',
    )
  },
  'must not pass SQLite IntegrationStore',
)

expectFailure(
  'PostgreSQL runtime eagerly constructs SQLite IntegrationStore',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'sqlite_pool.clone().map(|pool| {',
      'Some(crate::integration::store::IntegrationStore::with_key_store_and_metrics(',
    )
  },
  'Production IntegrationModule composition missing',
)

expectFailure(
  'Integration catalog restores pg_pool-derived SQLite fallback',
  snapshot => {
    snapshot.main +=
      '\n// let integration_store = pg_pool.is_none().then(|| local_integration_store());\n'
  },
  'must not restore pg_pool-derived SQLite fallback',
)

expectFailure(
  'IntegrationModule live proof stops exercising PostgreSQL composition',
  snapshot => {
    snapshot.moduleQualification = snapshot.moduleQualification.replaceAll(
      'IntegrationModule::new_with_postgres_persistence(',
      'IntegrationModule::new_REMOVED(',
    )
  },
  'IntegrationModule PostgreSQL composition live proof missing',
)

expectFailure(
  'Exact-Head stops executing IntegrationModule PostgreSQL composition proof',
  snapshot => {
    snapshot.exactHeadWorkflow = snapshot.exactHeadWorkflow.replaceAll(
      'live_pg18_integration_module_composition_routes_management_authorities_to_postgres',
      'removed_integration_module_composition_proof',
    )
  },
  'Exact-Head qualification must execute',
)

expectFailure(
  'Integration Catalog production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Integration Catalog gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Integration Catalog cutover mutation tests passed.')
