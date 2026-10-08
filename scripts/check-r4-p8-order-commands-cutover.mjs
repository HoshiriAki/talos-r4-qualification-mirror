#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  dispatch: 'backend/src/repositories/order_commands_dispatch.rs',
  writer: 'backend/src/repositories/order_commands_postgres.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/order_commands_qualification_tests.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8OrderCommandsSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8OrderCommandsSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'mod order_commands_dispatch;',
    'mod order_commands_postgres;',
    'pub use order_commands_dispatch::ScopedOrderCommandRepository;',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Order Commands PostgreSQL module wiring missing: ${token}`);
    }
  }

  if (!snapshot.provider.includes(
    'use crate::repositories::order_commands_dispatch::ScopedOrderCommandRepository;',
  )) {
    errors.push('Order Commands provider must use backend-neutral dispatch');
  }

  for (const token of [
    'PostgresOrderCommandRepository::new(self.scoped.session())',
    '.create_imported_draft(draft)',
    '.update_draft(',
    'SqliteOrderCommandRepository::new(self.scoped)',
  ]) {
    if (!snapshot.dispatch.includes(token)) {
      errors.push(`Order Commands backend dispatch missing: ${token}`);
    }
  }

  for (const token of [
    'pub(in crate::repositories) struct PostgresOrderCommandRepository',
    'SELECT tenant_id FROM orders WHERE orderno = $1 LIMIT 1 FOR SHARE',
    'import reconciliation orderNo collision',
    'INSERT INTO orders',
    "'draft'",
    'SELECT version FROM order_lifecycle',
    'lifecycle version 1',
    'FOR UPDATE OF l, o',
    'WHERE l.tenant_id = $1 AND l.order_id = $2',
    'stale Order version',
    'commercialStatus is draft',
    'WHERE tenant_id = $8 AND id = $9',
    '23505',
  ]) {
    if (!snapshot.writer.includes(token)) {
      errors.push(`Order Commands PostgreSQL writer invariant missing: ${token}`);
    }
  }
  if (!/self\.session\s*\.pg_write_serializable_repository\s*\(/s.test(snapshot.writer)) {
    errors.push(
      'Order Commands PostgreSQL writer must use rollback-aware SERIALIZABLE repository transactions',
    );
  }
  if (/\brusqlite\b|Sqlite|create_pool/.test(snapshot.writer)) {
    errors.push('Order Commands PostgreSQL writer must not depend on SQLite runtime types');
  }
  if (/updatedAt|updatedat/.test(snapshot.writer)) {
    errors.push('Order Commands PostgreSQL writer must not depend on nonexistent orders.updatedAt');
  }

  if (!snapshot.pgMod.includes('mod order_commands_qualification_tests;')) {
    errors.push('Order Commands PostgreSQL live qualification wiring missing');
  }

  for (const token of [
    'live_pg18_order_commands_preserve_draft_collision_version_scope_and_preview',
    'assert_eq!(row.2, "draft");',
    'assert_eq!(row.3, 1);',
    'same_tenant_collision',
    'cross_tenant_collision',
    '"REPOSITORY_CONTRACT_VIOLATION"',
    '"REPOSITORY_PREVIEW_WRITE_DENIED"',
    '"submit_order"',
    'assert_eq!(final_notes, "updated notes");',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Order Commands PostgreSQL live qualification proof missing: ${token}`);
    }
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push(
      'Order Commands production cutover must retain the fail-closed PostgreSQL authority guard',
    );
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push(
      'Order Commands cutover must not restore the retired transitional production barrier',
    );
  }

  return errors;
}

function main() {
  const errors = validateP8OrderCommandsSnapshot(collectP8OrderCommandsSnapshot());
  if (errors.length > 0) {
    console.error('R4-P8 Order Commands cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 Order Commands cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
