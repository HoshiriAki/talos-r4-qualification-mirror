#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  providerContract: 'backend/src/repositories/contracts/provider.rs',
  neutralSession: 'backend/src/repositories/session.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  pgProvider: 'backend/src/repositories/postgres/provider.rs',
  pgSession: 'backend/src/repositories/postgres/session.rs',
  pgQualification: 'backend/src/repositories/postgres/qualification_tests.rs',
  pgQuoteQualification: 'backend/src/repositories/postgres/quote_qualification_tests.rs',
  sqliteMod: 'backend/src/repositories/sqlite/mod.rs',
  customerDispatch: 'backend/src/repositories/customer_dispatch.rs',
  customerPg: 'backend/src/repositories/customer_postgres.rs',
  quoteDispatch: 'backend/src/repositories/quote_dispatch.rs',
  quotePg: 'backend/src/repositories/quote_postgres.rs',
  orderDispatch: 'backend/src/repositories/order_read_dispatch.rs',
  orderPg: 'backend/src/repositories/order_read_postgres.rs',
  lifecycleDispatch: 'backend/src/repositories/lifecycle_dispatch.rs',
  lifecyclePg: 'backend/src/repositories/lifecycle_postgres.rs',
  reservationDispatch: 'backend/src/repositories/reservation_dispatch.rs',
  reservationPg: 'backend/src/repositories/reservation_postgres.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8RepositoryCutoverSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8RepositoryCutoverSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'session: RepositorySession',
    'RepositorySession::sqlite(',
    'pub(in crate::repositories) fn postgres(',
    'RepositorySession::postgres(',
    'pub(in crate::repositories) fn session(&self) -> &RepositorySession',
    'use crate::repositories::customer_dispatch::ScopedCustomerRepository;',
    'use crate::repositories::quote_dispatch::ScopedQuoteRepository;',
    'use crate::repositories::reservation_dispatch::ScopedReservationRepository;',
  ]) {
    if (!snapshot.providerContract.includes(token)) {
      errors.push(`ScopedRepositories backend-neutral contract missing: ${token}`);
    }
  }
  if (snapshot.providerContract.includes('session: SqliteRepositorySession')) {
    errors.push('ScopedRepositories must not store SqliteRepositorySession directly');
  }

  for (const token of [
    'enum RepositorySession',
    'Sqlite(SqliteSessionBackend)',
    'Postgres(PostgresRepositorySession)',
    'RepositoryError::AdapterUnavailable(',
    'self.postgres_backend()?.read(operation)',
    'self.postgres_backend()?.write(operation)',
    'pub(in crate::repositories) fn pg_write_serializable_repository',
  ]) {
    if (!snapshot.neutralSession.includes(token)) {
      errors.push(`neutral RepositorySession missing: ${token}`);
    }
  }
  if (
    !/self\.postgres_backend\(\)\?\s*\.write_serializable_repository\(operation\)/s.test(
      snapshot.neutralSession,
    )
  ) {
    errors.push(
      'neutral RepositorySession missing: postgres backend repository-aware serializable delegation',
    );
  }

  for (const token of [
    '#[cfg(all(test, feature = "postgres"))]',
    'mod qualification_tests;',
    'mod quote_qualification_tests;',
  ]) {
    if (!snapshot.pgMod.includes(token)) {
      errors.push(`PostgreSQL repository qualification wiring missing: ${token}`);
    }
  }

  for (const token of [
    'pub struct PostgresRepositoryProvider',
    'fn bind(&self, ctx: &ExecutionContext)',
    'let binding = RepositoryBinding::from_execution(ctx)?;',
    'ScopedRepositories::postgres(',
  ]) {
    if (!snapshot.pgProvider.includes(token)) {
      errors.push(`PostgresRepositoryProvider scope authority missing: ${token}`);
    }
  }
  if (/std::env|DB_BACKEND|tenant_id\s*:/.test(snapshot.pgProvider)) {
    errors.push('PostgresRepositoryProvider must not choose backend or accept raw tenant authority');
  }

  for (const token of [
    'SET TRANSACTION READ ONLY',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'RepositoryAccess::ReadWrite',
    'RepositoryError::PreviewWriteDenied',
    'database.code().as_deref() == Some("25006")',
    'RuntimeFlavor::MultiThread',
    'tokio::task::block_in_place',
    'requires an active Tokio runtime',
    'requires the multi-thread Tokio runtime',
    'write_serializable_repository',
    'Future<Output = Result<T, RepositoryError>>',
  ]) {
    if (!snapshot.pgSession.includes(token)) {
      errors.push(`PostgreSQL session invariant missing: ${token}`);
    }
  }
  if (/Builder::new_(?:current_thread|multi_thread)|std::thread::spawn|std::thread::scope/.test(snapshot.pgSession)) {
    errors.push('PostgreSQL repository bridge must not move an existing PgPool onto a second runtime');
  }

  for (const token of [
    'type SqliteRepositorySession =',
    'crate::repositories::session::RepositorySession',
    'SqliteRepositorySession as SqliteSessionBackend',
  ]) {
    if (!snapshot.sqliteMod.includes(token)) {
      errors.push(`transitional SQLite family compatibility alias missing: ${token}`);
    }
  }

  for (const token of [
    'if self.scoped.session().is_postgres()',
    'PostgresCustomerRepository::new(self.scoped.session())',
    'SqliteCustomerRepository::new(self.scoped)',
  ]) {
    if (!snapshot.customerDispatch.includes(token)) {
      errors.push(`Customer backend dispatch missing: ${token}`);
    }
  }

  for (const token of [
    'FROM customers WHERE tenant_id = $1 AND id = $2',
    'FROM customer_contacts',
    'FROM customer_migration_exceptions',
    'self.session.pg_read(',
    'self.session.pg_write(',
    'self.session.pg_write_serializable(',
    'WHERE c.tenant_id = $1 AND cc.kind = $2 AND cc.normalized_value = $3',
    'WHERE tenant_id = $3 AND id = $4 AND status = \'pending\'',
  ]) {
    if (!snapshot.customerPg.includes(token)) {
      errors.push(`PostgreSQL Customer tenant/parity adapter missing: ${token}`);
    }
  }
  const customerTenantLookupCount =
    snapshot.customerPg.match(/FROM customers WHERE tenant_id = \$1 AND id = \$2/g)?.length ?? 0;
  if (customerTenantLookupCount < 2) {
    errors.push(
      `PostgreSQL Customer tenant authority must cover create/get/write existence lookups (found ${customerTenantLookupCount}, expected at least 2)`,
    );
  }
  if (/Sqlite|rusqlite|create_pool/.test(snapshot.customerPg)) {
    errors.push('PostgreSQL Customer adapter must not depend on SQLite runtime types');
  }

  for (const token of [
    'if self.scoped.session().is_postgres()',
    'PostgresQuoteRepository::new(self.scoped.session())',
    'SqliteQuoteRepository::new(self.scoped)',
  ]) {
    if (!snapshot.quoteDispatch.includes(token)) {
      errors.push(`Quote backend dispatch missing: ${token}`);
    }
  }

  for (const token of [
    'FROM accessory_catalog WHERE tenant_id = $1 AND id = $2',
    'SELECT EXISTS(SELECT 1 FROM customers WHERE tenant_id = $1 AND id = $2)',
    'FROM quotes WHERE tenant_id = $1 AND id = $2',
    'FROM quote_lines WHERE tenant_id = $1 AND quote_id = $2',
    'self.session.pg_read(',
    'self.session.pg_write(',
    'self.session.pg_write_serializable(',
    "AND converted_order_id IS NULL AND expires_at > $3",
    'INSERT INTO orders',
    'INSERT INTO order_lines',
    "WHERE tenant_id = $3 AND id = $4 AND status = 'confirmed'",
  ]) {
    if (!snapshot.quotePg.includes(token)) {
      errors.push(`PostgreSQL Quote tenant/parity adapter missing: ${token}`);
    }
  }
  if (/Sqlite|rusqlite|randomblob|create_pool/.test(snapshot.quotePg)) {
    errors.push('PostgreSQL Quote adapter must not depend on SQLite runtime or SQL functions');
  }

  for (const token of [
    'if self.scoped.session().is_postgres()',
    'PostgresOrderReadRepository::new(self.scoped.session()).list(request)',
    'PostgresOrderReadRepository::new(self.scoped.session()).get_by_id(id)',
    'SqliteOrderReadRepository::new(self.scoped).list(request)',
  ]) {
    if (!snapshot.orderDispatch.includes(token)) {
      errors.push(`Order Read backend dispatch missing: ${token}`);
    }
  }

  for (const token of [
    'o.tenant_id = ',
    'FROM orders o WHERE o.tenant_id = $1 AND o.id = $2',
    'WHERE tenant_id = $1 AND orderid = $2',
    'self.session.pg_read(',
    'o.orderno',
    'o.startdate',
    'o.totalprice',
    'o.createdat',
  ]) {
    if (!snapshot.orderPg.includes(token)) {
      errors.push(`PostgreSQL Order Read tenant/parity adapter missing: ${token}`);
    }
  }
  if (/Sqlite|rusqlite|create_pool/.test(snapshot.orderPg)) {
    errors.push('PostgreSQL Order Read adapter must not depend on SQLite runtime types');
  }

  for (const token of [
    'if self.scoped.session().is_postgres()',
    'PostgresOrderLifecycleRepository::new(self.scoped.session())',
    'SqliteOrderLifecycleRepository::new(self.scoped)',
  ]) {
    if (!snapshot.lifecycleDispatch.includes(token)) {
      errors.push(`Lifecycle backend dispatch missing: ${token}`);
    }
  }

  for (const token of [
    'FROM order_lifecycle WHERE tenant_id = $1 AND order_id = $2',
    'FROM rental_reservations',
    'FROM reservation_requirements',
    'FROM allocations a',
    'self.session.pg_read(',
    'reservation_not_confirmed',
    'allocation_incomplete',
  ]) {
    if (!snapshot.lifecyclePg.includes(token)) {
      errors.push(`PostgreSQL Lifecycle read/parity adapter missing: ${token}`);
    }
  }
  if (/Sqlite|rusqlite|create_pool/.test(snapshot.lifecyclePg)) {
    errors.push('PostgreSQL Lifecycle adapter must not depend on SQLite runtime types');
  }

  for (const token of [
    'if self.scoped.session().is_postgres()',
    'PostgresReservationRepository::new(self.scoped.session())',
    'SqliteReservationRepository::new(self.scoped)',
  ]) {
    if (!snapshot.reservationDispatch.includes(token)) {
      errors.push(`Reservation backend dispatch missing: ${token}`);
    }
  }

  for (const token of [
    'FROM rental_reservations',
    'FROM reservation_requirements',
    'FROM allocations',
    'tenant_id = $1',
    'serialno',
    'modelid',
    'self.session.pg_read(',
  ]) {
    if (!snapshot.reservationPg.includes(token)) {
      errors.push(`PostgreSQL Reservation read/parity adapter missing: ${token}`);
    }
  }
  if (/Sqlite|rusqlite|create_pool/.test(snapshot.reservationPg)) {
    errors.push('PostgreSQL Reservation adapter must not depend on SQLite runtime types');
  }

  for (const token of [
    'live_pg18_scoped_order_read_preserves_tenant_preview_and_mode_boundaries',
    'tenant_a.customers().create(&NewCustomerRecord',
    'tenant_b.customers().get(&customer_id)?.is_none()',
    'preview.customers().get(&customer_id)?.is_some()',
    'tenant_a.lifecycles().operational_view("order-a")?',
    'tenant_a.lifecycles().get("order-b")?.is_none()',
    'tenant_b.reservations().get(&reservation_id)?.is_none()',
    'capacity.reserved_capacity, 1',
    'INSERT INTO allocations',
    'REPOSITORY_PREVIEW_WRITE_DENIED',
    'REPOSITORY_SIMULATION_UNSUPPORTED',
  ]) {
    if (!snapshot.pgQualification.includes(token)) {
      errors.push(`PostgreSQL repository live qualification proof missing: ${token}`);
    }
  }
  if (
    !/tenant_a\s*\.reservations\(\)\s*\.allocation_complete_for_order\(\s*"order-a"\s*\)\?/.test(
      snapshot.pgQualification,
    )
  ) {
    errors.push('PostgreSQL Reservation allocation-completeness live proof missing');
  }
  if (!/tenant_a\.reservations\(\)\.expire_due\(\)\?,\s*0/.test(snapshot.pgQualification)) {
    errors.push('PostgreSQL Reservation normal-mode write qualification missing');
  }

  for (const token of [
    'live_pg18_quote_cutover_preserves_tenant_snapshot_and_conversion_authority',
    'tenant_b.quotes().get_accessory("accessory-a")?.is_none()',
    'tenant_b.quotes().get(&quote_id)?.is_none()',
    'create_order_from_quote(&quote_id, "quote conversion fixture")?',
    'tenant_b.orders().get_by_id(&converted.order_id)?.is_none()',
    'FROM order_lines WHERE tenant_id = \'tenant-a\' AND order_id = $1',
    'preview.quotes().get(&quote_id)?.is_some()',
    'REPOSITORY_PREVIEW_WRITE_DENIED',
  ]) {
    if (!snapshot.pgQuoteQualification.includes(token)) {
      errors.push(`PostgreSQL Quote live qualification proof missing: ${token}`);
    }
  }

  const repositoryCompositionStart = snapshot.main.indexOf(
    'let (repository_provider, workflow_worker_tenant_source)',
  );
  const repositoryCompositionEnd = snapshot.main.indexOf(
    'let integration_store =',
    repositoryCompositionStart,
  );
  const repositoryComposition =
    repositoryCompositionStart >= 0 && repositoryCompositionEnd > repositoryCompositionStart
      ? snapshot.main.slice(repositoryCompositionStart, repositoryCompositionEnd)
      : '';
  for (const token of [
    'PostgresRepositoryProvider::new_with_metrics(',
    'SqliteRepositoryProvider::new_with_metrics(',
    'WorkflowWorkerTenantSource::postgres(pg)',
    'let pool = require_sqlite_pool("repository provider and workflow tenant source")?;',
    'WorkflowWorkerTenantSource::new(pool)',
  ]) {
    if (!repositoryComposition.includes(token)) {
      errors.push(`RepositoryProvider production composition missing: ${token}`);
    }
  }

  const localRepositoryBindingPattern =
    /let pool = require_sqlite_pool\("repository provider and workflow tenant source"\)\?;\s*\(\s*Arc::new\(SqliteRepositoryProvider::new_with_metrics\(\s*pool\.clone\(\),\s*metrics\.clone\(\),?\s*\)\),\s*WorkflowWorkerTenantSource::new\(pool\),?\s*\)/g;
  const localRepositoryBindingCount =
    repositoryComposition.match(localRepositoryBindingPattern)?.length ?? 0;
  if (localRepositoryBindingCount !== 2) {
    errors.push(
      `RepositoryProvider local fallback must bind the explicit SQLite capability through provider and workflow tenant source in both compile paths (found ${localRepositoryBindingCount}, expected 2)`,
    );
  }
  if (snapshot.main.includes(
    'let repository_provider: Arc<dyn RepositoryProvider> =\n        Arc::new(SqliteRepositoryProvider::new(pool.clone()));',
  )) {
    errors.push('RepositoryProvider composition regressed to fixed SQLite authority');
  }

  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('P8-D repository cutover must not restore the transitional production barrier');
  }

  return errors;
}

function main() {
  const errors = validateP8RepositoryCutoverSnapshot(collectP8RepositoryCutoverSnapshot());
  if (errors.length > 0) {
    console.error('R4-P8 repository cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 repository cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
