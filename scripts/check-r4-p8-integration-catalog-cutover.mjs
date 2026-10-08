#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  integrationMod: 'backend/src/integration/mod.rs',
  authority: 'backend/src/integration/catalog_authority.rs',
  integrationModule: 'backend/src/integration/module.rs',
  registryFactory: 'backend/src/registry/factory.rs',
  registryAssembler: 'backend/src/registry/assembler.rs',
  dispatch: 'backend/src/integration/catalog_repository.rs',
  sqliteRepository: 'backend/src/integration/catalog_repository_sqlite.rs',
  postgresRepository: 'backend/src/integration/catalog_repository_postgres.rs',
  store: 'backend/src/integration/store.rs',
  liveQualification: 'backend/src/integration/catalog_repository_pg_tests.rs',
  moduleQualification: 'backend/src/integration/integration_module_postgres_tests.rs',
  exactHeadWorkflow: '.github/workflows/exact-head-qualification.yml',
  main: 'backend/src/main.rs',
}

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

export function collectP8IntegrationCatalogSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8IntegrationCatalogSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) mod catalog_authority;',
    'pub(crate) mod catalog_repository;',
    'pub(crate) mod catalog_repository_postgres;',
    'pub(crate) mod catalog_repository_sqlite;',
    'mod catalog_repository_pg_tests;',
  ]) {
    if (!snapshot.integrationMod.includes(token)) {
      errors.push(`Integration Catalog module wiring missing: ${token}`)
    }
  }

  for (const token of [
    'pub(crate) struct IntegrationCatalogAuthority',
    'repository: IntegrationCatalogRepository',
    'key_store: Arc<dyn KeyStore>',
    'pub(crate) fn sqlite(',
    'pub(crate) fn postgres(',
    'validate_instance_against_manifest(instance, &manifest)?',
    '.accepts_reference(',
    'self.repository.save_manifest(manifest)',
    'self.repository.save_instance(instance)',
    'self.repository.save_binding(binding, actor_ref)',
  ]) {
    if (!snapshot.authority.includes(token)) {
      errors.push(`Integration Catalog authority invariant missing: ${token}`)
    }
  }
  if (/std::env::|DB_BACKEND/.test(snapshot.authority)) {
    errors.push('Integration Catalog authority must not select a database from environment state')
  }

  for (const token of [
    'catalog: IntegrationCatalogAuthority',
    'new_with_postgres_persistence(',
    'IntegrationCatalogAuthority::postgres(pool.clone(), key_store)',
    'self.catalog.save_manifest(&manifest)?',
    'self.catalog.save_instance(&instance)?',
    'self.catalog.save_binding(&binding, Self::actor_ref(ctx))?',
    'self.catalog.catalog()?.resolve(ctx, &capability)?',
  ]) {
    if (!snapshot.integrationModule.includes(token)) {
      errors.push(`IntegrationModule Catalog composition missing: ${token}`)
    }
  }
  if (snapshot.integrationModule.includes('self.store.')) {
    errors.push('IntegrationModule must not bypass composed persistence through IntegrationStore')
  }

  for (const token of [
    'integration_store: Option<IntegrationStore>',
    'integration_module: Option<IntegrationModule>',
    'with_integration_module(',
    '.integration_module',
  ]) {
    if (!snapshot.registryFactory.includes(token)) {
      errors.push(`Registry factory IntegrationModule injection missing: ${token}`)
    }
  }
  if (!/IntegrationStore::new\(\s*self\.require_sqlite_pool\(\s*"integration compatibility store"\s*\)\?\s*,?\s*\)/s.test(snapshot.registryFactory)) {
    errors.push('Registry factory must bind the explicit SQLite capability directly to IntegrationStore')
  }
  for (const token of [
    'assemble_with_metrics_audit_sink_and_integration_module(',
    '.with_integration_module(integration_module)',
  ]) {
    if (!snapshot.registryAssembler.includes(token)) {
      errors.push(`Registry assembler IntegrationModule injection missing: ${token}`)
    }
  }
  const productionAssembler = between(
    snapshot.registryAssembler,
    'pub(crate) fn assemble_with_metrics_audit_sink_integration_and_staff_module(',
    '\n    }\n}',
  )
  if (
    productionAssembler.includes('IntegrationStore')
    || productionAssembler.includes('.with_integration_store(')
  ) {
    errors.push('Production Registry assembler must not accept SQLite IntegrationStore')
  }
  for (const token of [
    'sqlite_pool.clone().map(|pool| {',
    'let local_integration_store = || {',
    'IntegrationModule::new_with_postgres_persistence(',
    'IntegrationModule::new(local_integration_store())',
    'assemble_with_metrics_audit_sink_integration_and_staff_module(',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Production IntegrationModule composition missing: ${token}`)
    }
  }
  if (!/IntegrationStore::with_key_store_and_metrics\(\s*require_sqlite_pool\(\s*"integration compatibility store"\s*\)\?\s*,\s*integration_key_store\.clone\(\)\s*,\s*metrics\.clone\(\)\s*,?\s*\)/s.test(snapshot.main)) {
    errors.push('Local IntegrationStore composition must bind the explicit SQLite capability directly')
  }

  const compactMain = snapshot.main.replace(/\s+/g, '')
  if (
    compactMain.includes(
      'letintegration_store=pg_pool.is_none().then(||',
    )
  ) {
    errors.push(
      'Production IntegrationModule composition must not restore pg_pool-derived SQLite fallback',
    )
  }

  const registryCall = between(
    snapshot.main,
    'crate::registry::ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(',
    '.expect("ModuleRegistry assembly failed")',
  )
  if (registryCall.includes('integration_store')) {
    errors.push('Production Registry composition must not pass SQLite IntegrationStore')
  }

  for (const token of [
    'enum IntegrationCatalogBackend',
    'Sqlite(SqliteIntegrationCatalogRepository)',
    'Postgres(PostgresIntegrationCatalogRepository)',
    'pub(crate) fn postgres(pool: sqlx::PgPool)',
    'pub(crate) fn save_manifest',
    'pub(crate) fn save_instance',
    'pub(crate) fn save_binding',
    'pub(crate) fn catalog',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Integration Catalog dispatch invariant missing: ${token}`)
    }
  }

  for (const token of [
    'TransactionBehavior::Immediate',
    'provider manifest versions are immutable',
    'provider_binding_history',
  ]) {
    if (!snapshot.sqliteRepository.includes(token)) {
      errors.push(`SQLite Integration Catalog compatibility invariant missing: ${token}`)
    }
  }

  for (const token of [
    'ON CONFLICT (provider_id, version) DO NOTHING',
    'provider manifest versions are immutable',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'WHERE tenant_id=$1 AND id=$2',
    'FOR UPDATE',
    'BindingRevisionStale',
    'CapabilityNotDeclared',
    'provider_binding_history',
    'transaction.commit().await',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY',
  ]) {
    if (!snapshot.postgresRepository.includes(token)) {
      errors.push(`PostgreSQL Integration Catalog invariant missing: ${token}`)
    }
  }

  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresRepository)) {
    errors.push('PostgreSQL Integration Catalog must not depend on SQLite runtime types')
  }

  const saveInstance = between(
    snapshot.store,
    'pub fn save_instance(&self',
    'pub fn save_binding(',
  )
  for (const token of [
    'instance.validate()?',
    'self.catalog_repository.catalog()?',
    'validate_instance_against_manifest(instance, &manifest)?',
    '.key_store',
    '.accepts_reference(',
    'self.catalog_repository.save_instance(instance)',
  ]) {
    if (!saveInstance.includes(token)) {
      errors.push(`IntegrationStore instance admission policy missing after Catalog extraction: ${token}`)
    }
  }

  const saveBinding = between(
    snapshot.store,
    'pub fn save_binding(',
    'pub fn catalog(&self)',
  )
  for (const token of [
    'binding.validate()?',
    'self.catalog_repository.save_binding(binding, actor_ref)',
  ]) {
    if (!saveBinding.includes(token)) {
      errors.push(`IntegrationStore binding delegation invariant missing: ${token}`)
    }
  }

  if (!snapshot.store.includes(
    'catalog_repository: IntegrationCatalogRepository::new(pool.clone())',
  )) {
    errors.push(
      'Partial Integration Catalog parity must keep the existing SQLite composition explicit until runtime cutover',
    )
  }

  for (const token of [
    'live_pg18_integration_catalog_preserves_manifest_binding_history_and_tenant_revision',
    'IntegrationCatalogRepository::postgres',
    'repository.save_manifest(&manifest)?;',
    'assert!(matches!(duplicate, IntegrationError::InvalidManifest(_)));',
    'assert_eq!(stale, IntegrationError::BindingRevisionStale);',
    'assert_eq!(undeclared, IntegrationError::CapabilityNotDeclared);',
    'instance("tenant-b", "rev-b1")',
    '&binding("tenant-b", "fixture.payment.charge", "rev-b1"),',
    'assert_eq!(catalog.instances_for_tenant("tenant-b").len(), 1);',
    'assert_eq!(catalog.bindings_for_tenant("tenant-b").len(), 1);',
    '&binding("tenant-c", "fixture.payment.charge", "rev-1"),',
    'assert_eq!(wrong_tenant, IntegrationError::BindingUnavailable);',
    'assert_eq!(unchanged_history, 1);',
    'assert_eq!(stored_revision, "rev-2");',
    'assert_eq!(final_history, 2);',
    'let recomposed =',
    'IntegrationCatalogRepository::postgres',
    'recomposed_catalog.instances_for_tenant("tenant-b").len()',
    'recomposed_catalog.bindings_for_tenant("tenant-b")[0].config_revision',
    '"rev-b1"'
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Integration Catalog PostgreSQL live proof missing: ${token}`)
    }
  }

  for (const token of [
    'live_pg18_integration_module_composition_routes_management_authorities_to_postgres',
    'IntegrationModule::new_with_postgres_persistence(',
    '"register_manifest"',
    '"upsert_instance"',
    '"upsert_binding"',
    '"plan_operation"',
    '"create_webhook_endpoint"',
    '"create_deposit"',
    'external_operations',
    'webhook_endpoints',
    'integration_deposits',
    'talos_integration_operation_events_total{event=\\\"admitted\\\",state=\\\"ready\\\"} 1',
    'talos_financial_foundation_events_total{event=\\\"deposit_recorded\\\"} 1',
  ]) {
    if (!snapshot.moduleQualification.includes(token)) {
      errors.push(`IntegrationModule PostgreSQL composition live proof missing: ${token}`)
    }
  }
  if (!snapshot.exactHeadWorkflow.includes(
    'live_pg18_integration_module_composition_routes_management_authorities_to_postgres',
  )) {
    errors.push('Exact-Head qualification must execute the IntegrationModule PostgreSQL composition proof')
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Integration Catalog production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Integration Catalog cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8IntegrationCatalogSnapshot(
    collectP8IntegrationCatalogSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Integration Catalog cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Integration Catalog PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
