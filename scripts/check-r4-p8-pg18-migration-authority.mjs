#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  collectP8RepositoryCutoverSnapshot,
  validateP8RepositoryCutoverSnapshot,
} from './check-r4-p8-repository-cutover.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  dbMod: 'backend/src/db/mod.rs',
  liveTest: 'backend/src/db/p8_pg18_qualification.rs',
  main: 'backend/src/main.rs',
  exactHead: '.github/workflows/exact-head-qualification.yml',
  pgMigrations: 'backend/src/db/migrations/postgres',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

function readPgMigrations() {
  const directory = path.join(ROOT, PATHS.pgMigrations);
  return Object.fromEntries(
    fs
      .readdirSync(directory)
      .filter((name) => name.endsWith('.sql'))
      .sort()
      .map((name) => [name, fs.readFileSync(path.join(directory, name), 'utf8')]),
  );
}

function stripSqlStringsAndComments(sql) {
  let output = '';
  let index = 0;
  let state = 'normal';

  while (index < sql.length) {
    const char = sql[index];
    const next = sql[index + 1];

    if (state === 'normal') {
      if (char === "'") {
        output += ' ';
        state = 'single';
        index += 1;
        continue;
      }
      if (char === '-' && next === '-') {
        output += '  ';
        state = 'line-comment';
        index += 2;
        continue;
      }
      if (char === '/' && next === '*') {
        output += '  ';
        state = 'block-comment';
        index += 2;
        continue;
      }
      output += char;
      index += 1;
      continue;
    }

    if (state === 'single') {
      if (char === "'" && next === "'") {
        output += '  ';
        index += 2;
        continue;
      }
      if (char === "'") {
        output += ' ';
        state = 'normal';
        index += 1;
        continue;
      }
      output += char === '\n' ? '\n' : ' ';
      index += 1;
      continue;
    }

    if (state === 'line-comment') {
      if (char === '\n') {
        output += '\n';
        state = 'normal';
      } else {
        output += ' ';
      }
      index += 1;
      continue;
    }

    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        output += '  ';
        state = 'normal';
        index += 2;
        continue;
      }
      output += char === '\n' ? '\n' : ' ';
      index += 1;
    }
  }

  return output;
}

function collectQuotedViewAliases(executableSql) {
  const aliases = new Set();
  for (const view of executableSql.matchAll(
    /CREATE\s+(?:OR\s+REPLACE\s+)?VIEW\b[\s\S]*?\bAS\b([\s\S]*?);/gi,
  )) {
    for (const alias of view[1].matchAll(/\bAS\s+"([^"\r\n]*[A-Z][^"\r\n]*)"/g)) {
      aliases.add(alias[1]);
    }
  }
  return aliases;
}

function findUnsafeQuotedCamelIdentifiers(executableSql) {
  const viewAliases = collectQuotedViewAliases(executableSql);
  const unsafe = [];

  for (const match of executableSql.matchAll(/"([^"\r\n]*[A-Z][^"\r\n]*)"/g)) {
    const identifier = match[1];
    const before = executableSql.slice(Math.max(0, match.index - 48), match.index);
    const isOutputAlias = /\bAS\s*$/i.test(before);
    const isCompatibilityViewReference =
      /\b(?:NEW|OLD)\.\s*$/i.test(before) && viewAliases.has(identifier);

    if (!isOutputAlias && !isCompatibilityViewReference) {
      unsafe.push(identifier);
    }
  }

  return [...new Set(unsafe)];
}

export function collectPg18MigrationAuthoritySnapshot() {
  return {
    dbMod: read(PATHS.dbMod),
    liveTest: read(PATHS.liveTest),
    main: read(PATHS.main),
    exactHead: read(PATHS.exactHead),
    pgMigrations: readPgMigrations(),
  };
}

export function validatePg18MigrationAuthoritySnapshot(snapshot) {
  const errors = [];

  for (const token of [
    '#[cfg(all(test, feature = "postgres"))]',
    'mod p8_pg18_qualification;',
    'run_pg_extension_067(pool).await?',
    'run_pg_extension_068(pool).await?',
    'run_pg_extension_069(pool).await?',
    'run_pg_extension_070(pool).await?',
    'run_pg_extension_071(pool).await?',
    'run_pg_extension_072(pool).await?',
    'run_pg_extension_073(pool).await?',
    'run_pg_extension_074(pool).await?',
    'run_pg_extension_076(pool).await?',
    'run_pg_extension_077(pool).await?',
    'run_pg_extension_078(pool).await?',
    'run_pg_extension_079(pool).await?',
    'run_pg_extension_080(pool).await?',
    'run_pg_extension_082(pool).await?',
    'run_pg_extension_083(pool).await?',
  ]) {
    if (!snapshot.dbMod.includes(token)) {
      errors.push(`PostgreSQL production migration composition missing: ${token}`);
    }
  }

  for (const token of [
    'live_pg18_fresh_complete_migration_chain_is_idempotent',
    'crate::db::run_all_pg_migrations(pool).await',
    'run_migration_chain_with_diagnostics(&fixture.pool, "fresh-apply").await?',
    'run_migration_chain_with_diagnostics(&fixture.pool, "repeat-apply").await?',
    'after last applied migration',
    'second.is_empty()',
    '001_create_core_tables',
    '052_identity_authority_foundation',
    '057_durable_rental_workflow',
    '066_r3_machine_api',
    '067_r4_interconnect_fabric',
    '068_r4_plugin_host_security',
    '069_r4_platform_security_hardening',
    '070_r4_session_revocation_invariant',
    '071_r4_r3_terminal_blocker_trigger_fix',
    '072_r4_tenant_scoped_catalog_names',
    '073_r4_tenant_scoped_device_serials',
    '074_r4_tenant_scoped_asset_purchases',
    '076_r4_tenant_scoped_invoice_numbers',
    '077_r4_tenant_scoped_tax_config',
    '078_r4_credit_score_tenant_invariant',
    '079_r4_overdue_tenant_invariant',
    '080_r4_contract_tenant_invariant',
    '082_r4_reservation_rule_tenant_invariant',
    '083_r4_reservation_rule_sequence_invariant',
    'reservation_rules tenant_id must exist and be NOT NULL in the active schema',
    'reservation_rules BIGSERIAL must advance beyond the historical explicit seed id',
    'reservation rules must remain unique inside one tenant',
    'SHARED-SERIAL',
    'asset purchase serials must remain unique inside one tenant',
    'legacy order-device serial FK must include tenant identity',
    'contract order FK must include tenant identity',
    'Shared Model Name',
    'Shared Warehouse Name',
    'auth_rate_limit_state',
    'interconnect_state_heads',
    'plugin_packages',
    'trg_auth_password_change_revoke_sessions',
  ]) {
    if (!snapshot.liveTest.includes(token)) {
      errors.push(`fresh PostgreSQL 18 qualification proof missing: ${token}`);
    }
  }

  for (const [name, sql] of Object.entries(snapshot.pgMigrations ?? {})) {
    if (sql.charCodeAt(0) === 0xfeff) {
      errors.push(`PostgreSQL migration ${name} starts with UTF-8 BOM; embedded SQL must be BOM-free`);
    }

    const uppercaseColumnChecks = [
      ...sql.matchAll(/column_name\s*=\s*'([^']*[A-Z][^']*)'/g),
    ].map((match) => match[1]);
    if (uppercaseColumnChecks.length > 0) {
      errors.push(
        `PostgreSQL migration ${name} checks folded unquoted identifiers with uppercase information_schema column names: ${uppercaseColumnChecks.join(', ')}`,
      );
    }

    const executableSql = stripSqlStringsAndComments(sql);
    const unsafeQuotedCamelIdentifiers = findUnsafeQuotedCamelIdentifiers(executableSql);
    if (unsafeQuotedCamelIdentifiers.length > 0) {
      errors.push(
        `PostgreSQL migration ${name} uses case-sensitive quoted identifiers with uppercase letters against the unquoted folded schema: ${unsafeQuotedCamelIdentifiers.join(', ')}`,
      );
    }
  }

  const migrate = snapshot.main.indexOf('db::run_all_pg_migrations(&pool).await');
  const migrationFailure = snapshot.main.indexOf('PG migration error: {}');
  const admission = snapshot.main.indexOf(
    'if config.is_production && pg_pool.is_none()',
  );
  const sqliteOpen = snapshot.main.indexOf('create_pool(&config.db_path)');
  if (migrate < 0 || migrationFailure < 0 || admission < 0 || sqliteOpen < 0) {
    errors.push('main startup must expose migration, failure, production-authority and SQLite-open checkpoints');
  } else {
    if (migrate > admission) {
      errors.push('PostgreSQL migrations must complete before production runtime admission');
    }
    if (migrationFailure > admission) {
      errors.push('PostgreSQL migration failure must abort before production runtime admission');
    }
    if (admission > sqliteOpen) {
      errors.push('production PostgreSQL authority must be established before local SQLite capability opens');
    }
  }

  for (const token of [
    'Live PostgreSQL 18 fresh production migration evidence',
    'live_pg18_fresh_complete_migration_chain_is_idempotent',
  ]) {
    if (!snapshot.exactHead.includes(token)) {
      errors.push(`Exact-Head must execute P8 fresh PG18 migration evidence: ${token}`);
    }
  }

  return errors;
}

function main() {
  const errors = [
    ...validatePg18MigrationAuthoritySnapshot(collectPg18MigrationAuthoritySnapshot()),
    ...validateP8RepositoryCutoverSnapshot(collectP8RepositoryCutoverSnapshot()),
  ];
  if (errors.length > 0) {
    console.error('R4-P8 PostgreSQL migration/repository cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 PostgreSQL migration/repository cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
