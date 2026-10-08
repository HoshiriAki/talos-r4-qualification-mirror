#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8RepositoryCutoverSnapshot,
  validateP8RepositoryCutoverSnapshot,
} from './check-r4-p8-repository-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8RepositoryCutoverSnapshot());
  mutate(snapshot);
  const errors = validateP8RepositoryCutoverSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8RepositoryCutoverSnapshot(
  collectP8RepositoryCutoverSnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before P8 repository mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'ScopedRepositories hard-binds SQLite again',
  (snapshot) => {
    snapshot.providerContract = snapshot.providerContract.replace(
      'session: RepositorySession',
      'session: SqliteRepositorySession',
    );
  },
  'must not store SqliteRepositorySession directly',
);

expectFailure(
  'Postgres provider bypasses ExecutionContext binding',
  (snapshot) => {
    snapshot.pgProvider = snapshot.pgProvider.replace(
      'let binding = RepositoryBinding::from_execution(ctx)?;',
      'let binding = unsafe_binding_from_tenant();',
    );
  },
  'scope authority missing',
);

expectFailure(
  'Postgres repository live suite is no longer wired',
  (snapshot) => {
    snapshot.pgMod = snapshot.pgMod.replace('mod qualification_tests;', '// removed');
  },
  'qualification wiring missing',
);

expectFailure(
  'Quote live suite is no longer wired',
  (snapshot) => {
    snapshot.pgMod = snapshot.pgMod.replace('mod quote_qualification_tests;', '// removed');
  },
  'qualification wiring missing',
);

expectFailure(
  'Postgres read transaction loses database read-only enforcement',
  (snapshot) => {
    snapshot.pgSession = snapshot.pgSession.replace('SET TRANSACTION READ ONLY', 'SELECT 1');
  },
  'SET TRANSACTION READ ONLY',
);

expectFailure(
  'Postgres Preview write denial disappears',
  (snapshot) => {
    snapshot.pgSession = snapshot.pgSession.replaceAll(
      'RepositoryError::PreviewWriteDenied',
      'RepositoryError::Postgres("allowed".into())',
    );
  },
  'RepositoryError::PreviewWriteDenied',
);

expectFailure(
  'Customer dispatch drops PostgreSQL branch',
  (snapshot) => {
    snapshot.customerDispatch = snapshot.customerDispatch.replaceAll(
      'PostgresCustomerRepository::new(self.scoped.session())',
      'removed_postgres_customer_adapter()',
    );
  },
  'Customer backend dispatch missing',
);

expectFailure(
  'Customer PG adapter drops one tenant lookup guard',
  (snapshot) => {
    snapshot.customerPg = snapshot.customerPg.replace(
      'FROM customers WHERE tenant_id = $1 AND id = $2',
      'FROM customers WHERE id = $2',
    );
  },
  'Customer tenant authority must cover create/get/write existence lookups',
);

expectFailure(
  'Customer PG adapter gains SQLite dependency',
  (snapshot) => {
    snapshot.customerPg += '\nuse rusqlite::Connection;\n';
  },
  'Customer adapter must not depend on SQLite',
);

expectFailure(
  'repository live proof stops checking Customer tenant isolation',
  (snapshot) => {
    snapshot.pgQualification = snapshot.pgQualification.replace(
      'tenant_b.customers().get(&customer_id)?.is_none()',
      'true',
    );
  },
  'live qualification proof missing',
);

expectFailure(
  'Quote dispatch drops PostgreSQL branch',
  (snapshot) => {
    snapshot.quoteDispatch = snapshot.quoteDispatch.replaceAll(
      'PostgresQuoteRepository::new(self.scoped.session())',
      'removed_postgres_quote_adapter()',
    );
  },
  'Quote backend dispatch missing',
);

expectFailure(
  'Quote PG adapter drops quote tenant authority',
  (snapshot) => {
    snapshot.quotePg = snapshot.quotePg.replace(
      'FROM quotes WHERE tenant_id = $1 AND id = $2',
      'FROM quotes WHERE id = $2',
    );
  },
  'PostgreSQL Quote tenant/parity adapter missing',
);

expectFailure(
  'Quote PG adapter loses serialized conversion',
  (snapshot) => {
    snapshot.quotePg = snapshot.quotePg.replace(
      'self.session.pg_write_serializable(',
      'self.session.pg_write(',
    );
  },
  'PostgreSQL Quote tenant/parity adapter missing',
);

expectFailure(
  'Quote PG adapter gains SQLite dependency',
  (snapshot) => {
    snapshot.quotePg += '\nuse rusqlite::Connection;\n';
  },
  'Quote adapter must not depend on SQLite',
);

expectFailure(
  'Quote PG adapter reintroduces SQLite randomblob SQL',
  (snapshot) => {
    snapshot.quotePg += '\n// randomblob(16)\n';
  },
  'Quote adapter must not depend on SQLite',
);

expectFailure(
  'Quote live proof stops checking cross-tenant quote isolation',
  (snapshot) => {
    snapshot.pgQuoteQualification = snapshot.pgQuoteQualification.replace(
      'tenant_b.quotes().get(&quote_id)?.is_none()',
      'true',
    );
  },
  'Quote live qualification proof missing',
);

expectFailure(
  'Order Read list drops tenant predicate',
  (snapshot) => {
    snapshot.orderPg = snapshot.orderPg.replace('o.tenant_id = ', '1 = ');
  },
  'o.tenant_id = ',
);

expectFailure(
  'Order Read get drops tenant predicate',
  (snapshot) => {
    snapshot.orderPg = snapshot.orderPg.replace(
      'FROM orders o WHERE o.tenant_id = $1 AND o.id = $2',
      'FROM orders o WHERE o.id = $2',
    );
  },
  'FROM orders o WHERE o.tenant_id = $1 AND o.id = $2',
);

expectFailure(
  'Order Read PG adapter gains SQLite dependency',
  (snapshot) => {
    snapshot.orderPg += '\nuse rusqlite::Connection;\n';
  },
  'must not depend on SQLite',
);

expectFailure(
  'Lifecycle get drops tenant predicate',
  (snapshot) => {
    snapshot.lifecyclePg = snapshot.lifecyclePg.replace(
      'FROM order_lifecycle WHERE tenant_id = $1 AND order_id = $2',
      'FROM order_lifecycle WHERE order_id = $2',
    );
  },
  'PostgreSQL Lifecycle read/parity adapter missing',
);

expectFailure(
  'Lifecycle PG adapter gains SQLite dependency',
  (snapshot) => {
    snapshot.lifecyclePg += '\nuse rusqlite::Connection;\n';
  },
  'Lifecycle adapter must not depend on SQLite',
);

expectFailure(
  'repository live proof stops checking Lifecycle tenant isolation',
  (snapshot) => {
    snapshot.pgQualification = snapshot.pgQualification.replace(
      'tenant_a.lifecycles().get("order-b")?.is_none()',
      'true',
    );
  },
  'live qualification proof missing',
);

expectFailure(
  'Reservation dispatch drops PostgreSQL branch',
  (snapshot) => {
    snapshot.reservationDispatch = snapshot.reservationDispatch.replaceAll(
      'PostgresReservationRepository::new(self.scoped.session())',
      'removed_postgres_reservation_adapter()',
    );
  },
  'Reservation backend dispatch missing',
);

expectFailure(
  'Reservation PG adapter drops tenant authority',
  (snapshot) => {
    snapshot.reservationPg = snapshot.reservationPg.replaceAll('tenant_id = $1', '1 = 1');
  },
  'PostgreSQL Reservation read/parity adapter missing',
);

expectFailure(
  'Reservation PG adapter gains SQLite dependency',
  (snapshot) => {
    snapshot.reservationPg += '\nuse rusqlite::Connection;\n';
  },
  'Reservation adapter must not depend on SQLite',
);

expectFailure(
  'repository live proof stops checking Reservation tenant isolation',
  (snapshot) => {
    snapshot.pgQualification = snapshot.pgQualification.replace(
      'tenant_b.reservations().get(&reservation_id)?.is_none()',
      'true',
    );
  },
  'live qualification proof missing',
);

expectFailure(
  'repository live proof stops checking Reservation allocation completeness',
  (snapshot) => {
    snapshot.pgQualification = snapshot.pgQualification.replace(
      /tenant_a\s*\.reservations\(\)\s*\.allocation_complete_for_order\(\s*"order-a"\s*\)\?/,
      'true',
    );
  },
  'Reservation allocation-completeness live proof missing',
);

expectFailure(
  'production repository composition falls back to SQLite',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'PostgresRepositoryProvider::new_with_metrics(',
      'SqliteRepositoryProvider::new_with_metrics(',
    );
  },
  'PostgresRepositoryProvider::new_with_metrics(',
);

expectFailure(
  'one local repository compile path bypasses explicit sqlite capability',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'let pool = require_sqlite_pool("repository provider and workflow tenant source")?;',
      'let pool = create_pool(&config.db_path)?;',
    );
  },
  'both compile paths',
);

expectFailure(
  'local repository keeps capability token but severs provider binding',
  (snapshot) => {
    const source = snapshot.main;
    snapshot.main = snapshot.main.replace(
      'SqliteRepositoryProvider::new_with_metrics(\n                    pool.clone(),',
      'SqliteRepositoryProvider::new_with_metrics(\n                    legacy_pool.clone(),',
    );
    assert.notEqual(snapshot.main, source, 'repository provider binding mutation anchor missing');
  },
  'bind the explicit SQLite capability through provider and workflow tenant source',
);

expectFailure(
  'repository composition returns to fixed SQLite provider',
  (snapshot) => {
    snapshot.main +=
      '\nlet repository_provider: Arc<dyn RepositoryProvider> =\n        Arc::new(SqliteRepositoryProvider::new(pool.clone()));\n';
  },
  'fixed SQLite authority',
);

expectFailure(
  'repository cutover restores transitional admission barrier',
  (snapshot) => {
    snapshot.main +=
      '\nconst FORBIDDEN_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n';
  },
  'must not restore the transitional production barrier',
);

console.log('R4-P8 repository cutover mutation tests passed.');
