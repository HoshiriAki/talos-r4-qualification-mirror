#!/usr/bin/env node

import assert from 'node:assert/strict';
import './check-r4-p8-repository-cutover.test.mjs';
import {
  collectPg18MigrationAuthoritySnapshot,
  validatePg18MigrationAuthoritySnapshot,
} from './check-r4-p8-pg18-migration-authority.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectPg18MigrationAuthoritySnapshot());
  mutate(snapshot);
  const errors = validatePg18MigrationAuthoritySnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validatePg18MigrationAuthoritySnapshot(
  collectPg18MigrationAuthoritySnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before P8 migration mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'P7 security migration disappears from production composition',
  (snapshot) => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_070(pool).await?);',
      '// removed 070',
    );
  },
  'run_pg_extension_070(pool).await?',
);

expectFailure(
  'tenant-scoped catalog-name migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_072(pool).await?);',
      '// removed 072',
    );
  },
  'run_pg_extension_072(pool).await?',
);

expectFailure(
  'tenant-scoped device-serial migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_073(pool).await?);',
      '// removed 073',
    );
  },
  'run_pg_extension_073(pool).await?',
);

expectFailure(
  'tenant-scoped asset-purchase migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_074(pool).await?);',
      '// removed 074',
    );
  },
  'run_pg_extension_074(pool).await?',
);

expectFailure(
  'tenant-scoped invoice-number migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_076(pool).await?);',
      '// removed 076',
    );
  },
  'run_pg_extension_076(pool).await?',
);

expectFailure(
  'tenant-scoped tax-config migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_077(pool).await?);',
      '// removed 077',
    );
  },
  'run_pg_extension_077(pool).await?',
);

expectFailure(
  'credit-score tenant invariant migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_078(pool).await?);',
      '// removed 078',
    );
  },
  'run_pg_extension_078(pool).await?',
);

expectFailure(
  'overdue tenant invariant migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_079(pool).await?);',
      '// removed 079',
    );
  },
  'run_pg_extension_079(pool).await?',
);

expectFailure(
  'contract tenant invariant migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_080(pool).await?);',
      '// removed 080',
    );
  },
  'run_pg_extension_080(pool).await?',
);

expectFailure(
  'reservation-rule tenant migration disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_082(pool).await?);',
      '// removed 082',
    );
  },
  'run_pg_extension_082(pool).await?',
);

expectFailure(
  'reservation-rule sequence repair disappears from production composition',
  snapshot => {
    snapshot.dbMod = snapshot.dbMod.replace(
      'executed.extend(r4_migrations::run_pg_extension_083(pool).await?);',
      '// removed 083',
    );
  },
  'run_pg_extension_083(pool).await?',
);

expectFailure(
  'fresh PG18 proof drops reservation-rule sequence advancement',
  snapshot => {
    snapshot.liveTest = snapshot.liveTest.replace(
      'reservation_rules BIGSERIAL must advance beyond the historical explicit seed id',
      'reservation sequence proof removed',
    );
  },
  'reservation_rules BIGSERIAL must advance beyond the historical explicit seed id',
);

expectFailure(
  'fresh PG18 proof drops reservation-rule tenant uniqueness',
  snapshot => {
    snapshot.liveTest = snapshot.liveTest.replace(
      'reservation rules must remain unique inside one tenant',
      'reservation tenant uniqueness proof removed',
    );
  },
  'reservation rules must remain unique inside one tenant',
);

expectFailure(
  'fresh PG18 proof drops contract tenant-order foreign key',
  snapshot => {
    snapshot.liveTest = snapshot.liveTest.replace(
      'contract order FK must include tenant identity',
      'contract order FK proof removed',
    );
  },
  'contract order FK must include tenant identity',
);

expectFailure(
  'diagnostic wrapper bypasses real production migration authority',
  (snapshot) => {
    snapshot.liveTest = snapshot.liveTest.replace(
      'crate::db::run_all_pg_migrations(pool).await',
      'Ok(Vec::new())',
    );
  },
  'crate::db::run_all_pg_migrations(pool).await',
);

expectFailure(
  'fresh PG18 proof bypasses diagnostic migration application',
  (snapshot) => {
    snapshot.liveTest = snapshot.liveTest.replace(
      'run_migration_chain_with_diagnostics(&fixture.pool, "fresh-apply").await?',
      'Vec::new()',
    );
  },
  'run_migration_chain_with_diagnostics(&fixture.pool, "fresh-apply").await?',
);

expectFailure(
  'fresh PG18 proof stops checking idempotence',
  (snapshot) => {
    snapshot.liveTest = snapshot.liveTest.replace('second.is_empty()', 'second.len() > 0');
  },
  'second.is_empty()',
);

expectFailure(
  'fresh PG18 proof drops durable workflow migration',
  (snapshot) => {
    snapshot.liveTest = snapshot.liveTest.replace('057_durable_rental_workflow', '057_removed');
  },
  '057_durable_rental_workflow',
);

expectFailure(
  'fresh PG18 proof drops P7 throttle state',
  (snapshot) => {
    snapshot.liveTest = snapshot.liveTest.replace('auth_rate_limit_state', 'removed_rate_state');
  },
  'auth_rate_limit_state',
);

expectFailure(
  'PG migration starts with UTF-8 BOM',
  (snapshot) => {
    snapshot.pgMigrations['999_bom.sql'] = '\uFEFFSELECT 1;';
  },
  'starts with UTF-8 BOM',
);

expectFailure(
  'conditional PG migration compares camelCase information_schema column',
  (snapshot) => {
    snapshot.pgMigrations['999_bad_identifier_check.sql'] = `
      DO $$ BEGIN
        IF NOT EXISTS (
          SELECT 1 FROM information_schema.columns
          WHERE table_name = 'orders' AND column_name = 'trackingNo'
        ) THEN
          ALTER TABLE orders ADD COLUMN trackingNo TEXT;
        END IF;
      END $$;
    `;
  },
  'uppercase information_schema column names',
);

expectFailure(
  'PG migration quotes a folded camelCase identifier',
  (snapshot) => {
    snapshot.pgMigrations['999_bad_quoted_identifier.sql'] = `
      SELECT "trackingNo" FROM orders;
    `;
  },
  'case-sensitive quoted identifiers with uppercase letters',
);

{
  const snapshot = structuredClone(collectPg18MigrationAuthoritySnapshot());
  snapshot.pgMigrations['999_format_literal_is_not_identifier.sql'] = `
    SELECT to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS');
    -- "CommentOnlyCamelCase" must not be treated as executable SQL.
  `;
  const errors = validatePg18MigrationAuthoritySnapshot(snapshot).filter((error) =>
    error.includes('999_format_literal_is_not_identifier.sql'),
  );
  assert.deepEqual(
    errors,
    [],
    `quoted text inside SQL literals/comments must not be classified as identifiers: ${errors.join('; ')}`,
  );
}

{
  const snapshot = structuredClone(collectPg18MigrationAuthoritySnapshot());
  snapshot.pgMigrations['999_legacy_compatibility_view.sql'] = `
    CREATE VIEW legacy_orders AS
    SELECT trackingno AS "trackingNo"
    FROM orders;

    CREATE OR REPLACE FUNCTION insert_legacy_order() RETURNS trigger AS $$
    BEGIN
      INSERT INTO orders (trackingno) VALUES (NEW."trackingNo");
      RETURN NEW;
    END;
    $$ LANGUAGE plpgsql;

    CREATE TRIGGER legacy_orders_insert
    INSTEAD OF INSERT ON legacy_orders
    FOR EACH ROW EXECUTE FUNCTION insert_legacy_order();
  `;
  const errors = validatePg18MigrationAuthoritySnapshot(snapshot).filter((error) =>
    error.includes('999_legacy_compatibility_view.sql'),
  );
  assert.deepEqual(
    errors,
    [],
    `quoted legacy view aliases and NEW references must remain valid compatibility boundaries: ${errors.join('; ')}`,
  );
}

expectFailure(
  'production runtime admission moves before migrations',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
    snapshot.main = snapshot.main.replace(
      'let mut config = AppConfig::from_env();',
      'let mut config = AppConfig::from_env();\n    if config.is_production && pg_pool.is_none() { anyhow::bail!("early"); }',
    );
  },
  'migrations must complete before production runtime admission',
);

expectFailure(
  'Exact-Head drops fresh migration runtime evidence',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'live_pg18_fresh_complete_migration_chain_is_idempotent',
      'skipped_fresh_pg18_migration',
    );
  },
  'Exact-Head must execute P8 fresh PG18 migration evidence',
);

console.log('R4-P8 PostgreSQL migration authority mutation tests passed.');
