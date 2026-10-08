#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  lifecycleDispatch: 'backend/src/repositories/lifecycle_dispatch.rs',
  lifecycleWrite: 'backend/src/repositories/lifecycle_postgres_write.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/lifecycle_write_qualification_tests.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8LifecycleWriteSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8LifecycleWriteSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    '#[cfg(feature = "postgres")]',
    'mod lifecycle_postgres_write;',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Lifecycle PostgreSQL writer module wiring missing: ${token}`);
    }
  }

  for (const token of [
    'PostgresLifecycleWriteRepository::new(self.scoped.session()).apply_action(',
    'PostgresOrderLifecycleRepository::new(self.scoped.session())',
    '.operational_view(order_id)',
  ]) {
    if (!snapshot.lifecycleDispatch.includes(token)) {
      errors.push(`Lifecycle PostgreSQL write dispatch missing: ${token}`);
    }
  }
  if (
    /return\s+PostgresOrderLifecycleRepository::new\(self\.scoped\.session\(\)\)\.apply_action\(/s.test(
      snapshot.lifecycleDispatch,
    )
  ) {
    errors.push('Lifecycle dispatch must not route PostgreSQL writes through the legacy unavailable read adapter');
  }

  for (const token of [
    'pub(in crate::repositories) struct PostgresLifecycleWriteRepository',
    'FOR UPDATE',
    'WHERE tenant_id = $8 AND order_id = $9 AND version = $10',
    'INSERT INTO order_lifecycle_history',
    'UPDATE orders SET status = $1 WHERE tenant_id = $2 AND id = $3',
    'INSERT INTO domain_outbox',
    'ON CONFLICT (tenant_id, idempotency_key) DO NOTHING',
    'optimistic lifecycle version conflict',
    'optimistic lifecycle update lost its expected version',
    'can_mark_ready_to_ship blocked',
  ]) {
    if (!snapshot.lifecycleWrite.includes(token)) {
      errors.push(`Lifecycle PostgreSQL serialized writer invariant missing: ${token}`);
    }
  }
  if (
    !/self\.session\s*\.pg_write_serializable_repository\s*\(/s.test(
      snapshot.lifecycleWrite,
    )
  ) {
    errors.push(
      'Lifecycle PostgreSQL serialized writer invariant missing: pg_write_serializable_repository transaction ownership',
    );
  }
  if (/\brusqlite\b|Sqlite|create_pool/.test(snapshot.lifecycleWrite)) {
    errors.push('Lifecycle PostgreSQL writer must not depend on SQLite runtime types');
  }

  for (const token of [
    'mod lifecycle_write_qualification_tests;',
    'mod qualification_tests;',
    'mod quote_qualification_tests;',
  ]) {
    if (!snapshot.pgMod.includes(token)) {
      errors.push(`Lifecycle PostgreSQL live qualification wiring missing: ${token}`);
    }
  }

  for (const token of [
    'live_pg18_lifecycle_write_preserves_version_history_outbox_and_scope',
    '"submit_order"',
    '"confirm_order"',
    'assert_eq!(submitted.lifecycle.version, 2);',
    'assert_eq!(confirmed.lifecycle.version, 3);',
    '"REPOSITORY_CONTRACT_VIOLATION"',
    'tenant_b.lifecycles().get("lifecycle-order-a")?.is_none()',
    'FROM domain_outbox',
    '"OrderConfirmed"',
    '"REPOSITORY_PREVIEW_WRITE_DENIED"',
    'assert_eq!(version_after_preview, 3);',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Lifecycle PostgreSQL live qualification proof missing: ${token}`);
    }
  }
  if (
    !/tenant_b\s*\.lifecycles\(\)\s*\.history\(\s*"lifecycle-order-a"\s*\)\?\s*\.is_empty\(\)/s.test(
      snapshot.liveQualification,
    )
  ) {
    errors.push(
      'Lifecycle PostgreSQL live qualification proof missing: tenant B history isolation assertion',
    );
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push(
      'Lifecycle production cutover must retain the fail-closed PostgreSQL authority guard',
    );
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Lifecycle cutover must not restore the retired transitional production barrier');
  }

  return errors;
}

function main() {
  const errors = validateP8LifecycleWriteSnapshot(collectP8LifecycleWriteSnapshot());
  if (errors.length > 0) {
    console.error('R4-P8 lifecycle write cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 lifecycle write cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
