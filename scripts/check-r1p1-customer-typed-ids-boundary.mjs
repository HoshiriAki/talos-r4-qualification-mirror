#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const RULE = 'TALOS-OPS-027'
const PATHS = Object.freeze({
  domain: 'backend/src/domain/customer.rs',
  sqliteMigration: 'backend/src/db/migrations/053_customer_domain.sql',
  sqliteRegistry: 'backend/src/db/migrations.rs',
  pgMigration: 'backend/src/db/migrations/postgres/053_customer_domain.sql',
  pgRegistry: 'backend/src/db/migrations_pg.rs',
  repository: 'backend/src/repositories/customer.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  application: 'backend/src/application/customer.rs',
  tests: 'backend/src/application/customer_tests.rs',
  routes: 'backend/src/routes/customers.rs',
  routesIndex: 'backend/src/routes/mod.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  package: 'package.json',
  docsIndex: 'policy/qualification/legacy-evidence/docs/README.md',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p1-customer-typed-ids.md',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(finding(path, evidence))
}

function forbidText(failures, source, token, path, evidence) {
  if (source.includes(token)) failures.push(finding(path, evidence))
}

function requirePattern(failures, source, pattern, path, evidence) {
  if (!pattern.test(source)) failures.push(finding(path, evidence))
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

function structSlice(source, name) {
  const start = source.indexOf(`pub struct ${name}`)
  if (start < 0) return ''
  const next = source.indexOf('\npub struct ', start + 1)
  return source.slice(start, next < 0 ? source.length : next)
}

export function checkR1P1CustomerTypedIdsBoundary(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R1-P1 evidence is missing'))
  }
  if (failures.length > 0) return failures

  const domain = files[PATHS.domain]
  const sqliteMigration = files[PATHS.sqliteMigration]
  const sqliteRegistry = files[PATHS.sqliteRegistry]
  const pgMigration = files[PATHS.pgMigration]
  const pgRegistry = files[PATHS.pgRegistry]
  const repository = files[PATHS.repository]
  const provider = files[PATHS.provider]
  const application = files[PATHS.application]
  const tests = files[PATHS.tests]
  const routes = files[PATHS.routes]
  const routesIndex = files[PATHS.routesIndex]
  const descriptors = files[PATHS.descriptors]
  const factory = files[PATHS.factory]
  const packageJson = files[PATHS.package]
  const docsIndex = files[PATHS.docsIndex]
  const record = files[PATHS.record]

  for (const token of [
    'pub struct CustomerId(String);',
    'Uuid::new_v4().to_string()',
    'Uuid::parse_str',
    'pub enum CustomerStatus',
    'pub enum CustomerRiskStatus',
    'pub enum ContactKind',
    'normalize_contact_value',
    'mask_contact_value',
  ]) requireText(failures, domain, token, PATHS.domain, `Customer domain missing ${token}`)
  const idSlice = structSlice(domain, 'CustomerId')
  forbidText(failures, idSlice, 'phone', PATHS.domain, 'CustomerId must not encode phone identity')
  forbidText(failures, idSlice, 'name', PATHS.domain, 'CustomerId must not encode name identity')

  for (const [source, path] of [[sqliteMigration, PATHS.sqliteMigration], [pgMigration, PATHS.pgMigration]]) {
    for (const token of [
      'CREATE TABLE IF NOT EXISTS customers',
      'customer_contacts',
      'customer_external_identities',
      'customer_history',
      'customer_migration_exceptions',
      'customer_id',
      'legacy_free_text_customer_requires_resolution',
      'legacy_order_has_no_stable_customer_reference',
    ]) requireText(failures, source, token, path, `migration missing ${token}`)
    forbidText(failures, source, 'INSERT INTO customers', path, 'legacy migration must not auto-synthesize Customer identities')
  }
  requireText(failures, sqliteRegistry, 'id: "053_customer_domain"', PATHS.sqliteRegistry, 'SQLite migration registry must own 053')
  requireText(failures, sqliteRegistry, 'migrations/053_customer_domain.sql', PATHS.sqliteRegistry, 'SQLite registry must embed 053')
  requireText(failures, pgRegistry, 'id: "053_customer_domain"', PATHS.pgRegistry, 'PostgreSQL migration registry must own 053')
  requireText(failures, pgRegistry, 'migrations/postgres/053_customer_domain.sql', PATHS.pgRegistry, 'PostgreSQL registry must embed 053')

  for (const token of [
    'pub struct ScopedCustomerRepository',
    'self.session.binding().tenant_id()',
    'pub fn create(',
    'pub fn duplicate_candidates(',
    'pub fn resolve_migration_exception(',
    'pub fn anonymize(',
    'masked_value:',
    '"orders" =>',
    '"blacklist" =>',
    '"violations" =>',
    '"credit_scores" =>',
    '"overdue_records" =>',
    '"contracts" =>',
  ]) requireText(failures, repository, token, PATHS.repository, `scoped Customer repository missing ${token}`)
  forbidText(failures, repository, 'Pool<SqliteConnectionManager>', PATHS.repository, 'Customer repository must not own a raw pool')
  requireText(failures, provider, 'pub fn customers(&self) -> ScopedCustomerRepository', PATHS.provider, 'ScopedRepositories must expose Customer only after bind(ctx)')

  const projection = structSlice(repository, 'CustomerContactProjection')
  requireText(failures, projection, 'masked_value', PATHS.repository, 'normal Customer projection must expose masked contact only')
  forbidText(failures, projection, 'raw_value', PATHS.repository, 'normal Customer projection must never expose raw contact values')
  forbidText(failures, projection, 'normalized_value', PATHS.repository, 'normal Customer projection must never expose normalized contact values')

  for (const token of [
    'pub struct CustomerModule',
    '"list_customers"',
    '"get_customer"',
    '"find_duplicate_candidates"',
    '"create_customer"',
    '"add_contact"',
    '"add_external_identity"',
    '"list_migration_exceptions"',
    '"resolve_migration_exception"',
    '"anonymize_customer"',
    'AccessRequirement::TenantAdmin',
    'EffectClass::DatabaseRead',
    'EffectClass::DatabaseWrite',
    'SimulationSupport::Blocked',
    'self.repositories.bind(ctx)?',
  ]) requireText(failures, application, token, PATHS.application, `Customer Registry module missing ${token}`)

  requireText(failures, descriptors, 'Customer,', PATHS.descriptors, 'descriptor factory identity must include Customer')
  requirePattern(
    failures,
    compact(descriptors),
    /descriptor!\("customer",Business,ModuleActivation::Always,[A-Z_]+,Customer\)/,
    PATHS.descriptors,
    'descriptor catalog must register customer',
  )
  requireText(failures, factory, '(CustomerModule, Customer, "customer")', PATHS.factory, 'ModuleFactory construction identity must own Customer')
  requireText(failures, factory, 'CustomerModule::new(repository_provider.clone())', PATHS.factory, 'Customer must receive RepositoryProvider from Composition Root')

  requireText(failures, routes, 'TrustedTenantUser', PATHS.routes, 'Customer user routes must use trusted tenant resolution')
  requireText(failures, routes, 'TrustedTenantAdmin', PATHS.routes, 'Customer admin routes must use trusted tenant resolution')
  requireText(failures, routes, 'tenant_user.context()', PATHS.routes, 'Customer route must forward trusted ExecutionContext')
  requireText(failures, routes, 'tenant_admin.context()', PATHS.routes, 'Customer admin route must forward trusted ExecutionContext')
  requireText(failures, routes, '"/api/v2/customers"', PATHS.routes, 'Customer V2 route must exist')
  forbidText(failures, routes, 'make_ctx(', PATHS.routes, 'Customer route must not reconstruct ExecutionContext after trusted resolution')
  forbidText(failures, routes, 'tenant_user: TenantUser', PATHS.routes, 'legacy TenantUser extractor is forbidden on new Customer routes')
  forbidText(failures, routes, 'tenant_admin: TenantAdmin', PATHS.routes, 'legacy TenantAdmin extractor is forbidden on new Customer routes')
  forbidText(failures, routes, 'SELECT ', PATHS.routes, 'route-layer SQL is forbidden')
  forbidText(failures, routes, 'INSERT ', PATHS.routes, 'route-layer SQL is forbidden')
  requireText(failures, routesIndex, '.merge(customers::customer_routes())', PATHS.routesIndex, 'Customer routes must be mounted')

  for (const token of [
    'customer_is_tenant_scoped_and_pii_is_masked',
    'preview_is_read_only_and_simulation_is_fail_closed',
    'migration_exception_resolution_is_tenant_bound_and_admin_only',
    'anonymization_removes_contacts_but_preserves_customer_identity',
    '"tenant-a"',
    '"tenant-b"',
    '"***8000"',
    'ExecutionMode::ReadOnlyPreview',
    'ExecutionMode::Simulation',
  ]) requireText(failures, tests, token, PATHS.tests, `Customer tests missing ${token}`)

  for (const token of [
    'CUSTOMER_DOMAIN_IMPLEMENTED',
    'CUSTOMER_ID_IS_BUSINESS_AUTHORITY',
    'IDENTITY_CUSTOMER_SEPARATION_PRESERVED',
    'LEGACY_CUSTOMER_INFERENCE_FORBIDDEN',
    'SCOPED_CUSTOMER_REPOSITORY_REQUIRED',
    'PII_MASKING_REQUIRED',
    'DUPLICATE_IS_CANDIDATE_NOT_AUTO_MERGE',
    'TALOS-OPS-027',
    'R1-P2',
  ]) requireText(failures, record, token, PATHS.record, `R1-P1 record missing ${token}`)
  for (const falseClaim of [
    'status: R1_COMPLETE',
    'status: ORDER_V2_COMPLETE',
    'status: POSTGRESQL_PARITY_COMPLETE',
    'status: SIMULATION_STORAGE_COMPLETE',
  ]) forbidText(failures, record, falseClaim, PATHS.record, `R1-P1 must not claim ${falseClaim}`)

  requireText(failures, docsIndex, 'R1-P1 Customer', PATHS.docsIndex, 'documentation index must register R1-P1')
  for (const token of [
    'quality:talos-ops:customer-typed-ids:test',
    'quality:talos-ops:customer-typed-ids',
    'check-r1p1-customer-typed-ids-boundary.test.mjs',
    'check-r1p1-customer-typed-ids-boundary.mjs',
  ]) requireText(failures, packageJson, token, PATHS.package, `package command missing ${token}`)

  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const failures = checkR1P1CustomerTypedIdsBoundary(loadFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('R1-P1 Customer / Typed IDs boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R1-P1 Customer / Typed IDs boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
