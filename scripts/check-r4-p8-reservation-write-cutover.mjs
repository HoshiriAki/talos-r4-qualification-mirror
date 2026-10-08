#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  reservationDispatch: 'backend/src/repositories/reservation_dispatch.rs',
  reservationWrite: 'backend/src/repositories/reservation_postgres_write.rs',
  neutralSession: 'backend/src/repositories/session.rs',
  pgSession: 'backend/src/repositories/postgres/session.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/reservation_write_qualification_tests.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8ReservationWriteSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8ReservationWriteSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    '#[cfg(feature = "postgres")]',
    'mod reservation_postgres_write;',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Reservation PostgreSQL writer module wiring missing: ${token}`);
    }
  }

  if (!snapshot.reservationDispatch.includes(
    'use crate::repositories::reservation_postgres_write::PostgresReservationWriteRepository;',
  )) {
    errors.push('Reservation PostgreSQL writer dispatch import missing');
  }
  const writerDispatchCount =
    snapshot.reservationDispatch.match(
      /PostgresReservationWriteRepository::new\(self\.scoped\.session\(\)\)/g,
    )?.length ?? 0;
  if (writerDispatchCount < 7) {
    errors.push(
      `Reservation PostgreSQL write dispatch incomplete: found ${writerDispatchCount}, expected at least 7 writer routes`,
    );
  }

  if (!snapshot.neutralSession.includes(
    'pub(in crate::repositories) fn pg_write_serializable_repository',
  )) {
    errors.push(
      'RepositorySession rollback-aware PostgreSQL contract missing: pg_write_serializable_repository',
    );
  }
  if (
    !/self\.postgres_backend\(\)\?\s*\.write_serializable_repository\(operation\)/s.test(
      snapshot.neutralSession,
    )
  ) {
    errors.push(
      'RepositorySession rollback-aware PostgreSQL contract missing: postgres backend delegation',
    );
  }

  const repositoryTxStart = snapshot.pgSession.indexOf('fn write_serializable_repository');
  const repositoryTx =
    repositoryTxStart >= 0 ? snapshot.pgSession.slice(repositoryTxStart) : '';
  for (const token of [
    'Future<Output = Result<T, RepositoryError>>',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'let _ = transaction.rollback().await;',
    'Err(error)',
  ]) {
    if (!repositoryTx.includes(token)) {
      errors.push(`PostgreSQL repository-error rollback transaction missing: ${token}`);
    }
  }

  const reservationInsertCount =
    snapshot.reservationWrite.match(/INSERT INTO rental_reservations/g)?.length ?? 0;
  if (reservationInsertCount < 2) {
    errors.push(
      `Reservation PostgreSQL reservation insert paths incomplete: found ${reservationInsertCount}, expected at least 2`,
    );
  }

  for (const token of [
    'pub(in crate::repositories) struct PostgresReservationWriteRepository',
    'pg_write_serializable_repository',
    'INSERT INTO reservation_requirements',
    'INSERT INTO allocations',
    'FOR UPDATE',
    'FOR SHARE',
    "UPDATE allocations SET status = 'released'",
    'ReservationConfirmed',
    'AllocationComplete',
    'ON CONFLICT (tenant_id, idempotency_key) DO NOTHING',
    'insufficient model capacity',
    'device already allocated in overlapping interval',
    'allocation exceeds reservation model requirement',
    'WHERE tenant_id = $1 AND id = $2',
  ]) {
    if (!snapshot.reservationWrite.includes(token)) {
      errors.push(`Reservation PostgreSQL serialized writer invariant missing: ${token}`);
    }
  }
  if (/\brusqlite\b|Sqlite|create_pool|write_unavailable/.test(snapshot.reservationWrite)) {
    errors.push('Reservation PostgreSQL writer must not depend on SQLite or fail-closed stubs');
  }

  if (!snapshot.pgMod.includes('mod reservation_write_qualification_tests;')) {
    errors.push('Reservation PostgreSQL live qualification wiring missing');
  }

  for (const token of [
    'live_pg18_reservation_write_preserves_atomic_batch_outbox_and_scope',
    '.create_from_order(',
    '.confirm(&reservation_id, None)?',
    '.allocate_devices_batch(&[',
    'ReservationConfirmed',
    'AllocationComplete',
    'tenant_b.reservations().get(&reservation_id)?.is_none()',
    'assert_eq!(rollback_allocations, 0);',
    'assert_eq!(rollback_completion_events, 0);',
    'REPOSITORY_CONTRACT_VIOLATION',
    'REPOSITORY_PREVIEW_WRITE_DENIED',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Reservation PostgreSQL live qualification proof missing: ${token}`);
    }
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push(
      'Reservation production cutover must retain the fail-closed PostgreSQL authority guard',
    );
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push(
      'Reservation cutover must not restore the retired transitional production barrier',
    );
  }

  return errors;
}

function main() {
  const errors = validateP8ReservationWriteSnapshot(
    collectP8ReservationWriteSnapshot(),
  );
  if (errors.length > 0) {
    console.error('R4-P8 Reservation write cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 Reservation/Allocation write cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
