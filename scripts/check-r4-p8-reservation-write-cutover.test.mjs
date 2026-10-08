#!/usr/bin/env node

import assert from 'node:assert/strict';
import './check-r4-p8-order-commands-cutover.test.mjs';
import {
  collectP8ReservationWriteSnapshot,
  validateP8ReservationWriteSnapshot,
} from './check-r4-p8-reservation-write-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8ReservationWriteSnapshot());
  mutate(snapshot);
  const errors = validateP8ReservationWriteSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8ReservationWriteSnapshot(
  collectP8ReservationWriteSnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Reservation write mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'Reservation writer loses rollback-aware serializable ownership',
  (snapshot) => {
    snapshot.reservationWrite = snapshot.reservationWrite.replaceAll(
      'pg_write_serializable_repository',
      'pg_write_serializable',
    );
  },
  'pg_write_serializable_repository',
);

expectFailure(
  'Neutral session bypasses rollback-aware PostgreSQL delegation',
  (snapshot) => {
    snapshot.neutralSession = snapshot.neutralSession.replace(
      /self\.postgres_backend\(\)\?\s*\.write_serializable_repository\(operation\)/s,
      'removed_repository_serializable_delegate(operation)',
    );
  },
  'postgres backend delegation',
);

expectFailure(
  'Repository-aware transaction stops rolling back business errors',
  (snapshot) => {
    const start = snapshot.pgSession.indexOf('fn write_serializable_repository');
    snapshot.pgSession =
      snapshot.pgSession.slice(0, start) +
      snapshot.pgSession
        .slice(start)
        .replaceAll('let _ = transaction.rollback().await;', '');
  },
  'transaction.rollback',
);

expectFailure(
  'Reservation dispatch bypasses PostgreSQL writer',
  (snapshot) => {
    snapshot.reservationDispatch = snapshot.reservationDispatch.replaceAll(
      'PostgresReservationWriteRepository::new(self.scoped.session())',
      'removed_reservation_writer()',
    );
  },
  'write dispatch incomplete',
);

expectFailure(
  'Reservation writer drops canonical Reservation insert paths',
  (snapshot) => {
    snapshot.reservationWrite = snapshot.reservationWrite.replaceAll(
      'INSERT INTO rental_reservations',
      'INSERT INTO removed_reservations',
    );
  },
  'reservation insert paths incomplete',
);

expectFailure(
  'Reservation writer loses tenant predicate',
  (snapshot) => {
    snapshot.reservationWrite = snapshot.reservationWrite.replaceAll(
      'WHERE tenant_id = $1 AND id = $2',
      'WHERE id = $2',
    );
  },
  'WHERE tenant_id = $1 AND id = $2',
);

expectFailure(
  'Reservation confirmation loses outbox',
  (snapshot) => {
    snapshot.reservationWrite = snapshot.reservationWrite.replaceAll(
      'ReservationConfirmed',
      'REMOVED_CONFIRM_EVENT',
    );
  },
  'ReservationConfirmed',
);

expectFailure(
  'Allocation completion loses outbox',
  (snapshot) => {
    snapshot.reservationWrite = snapshot.reservationWrite.replaceAll(
      'AllocationComplete',
      'REMOVED_ALLOCATION_EVENT',
    );
  },
  'AllocationComplete',
);

expectFailure(
  'Reservation writer gains SQLite coupling',
  (snapshot) => {
    snapshot.reservationWrite += '\nuse rusqlite::Connection;\n';
  },
  'must not depend on SQLite',
);

expectFailure(
  'Reservation live proof stops asserting full batch rollback',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(rollback_allocations, 0);',
      'assert!(true);',
    );
  },
  'rollback_allocations',
);

expectFailure(
  'Reservation live proof stops asserting no completion event after rollback',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(rollback_completion_events, 0);',
      'assert!(true);',
    );
  },
  'rollback_completion_events',
);

expectFailure(
  'Reservation live proof stops checking tenant isolation',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'tenant_b.reservations().get(&reservation_id)?.is_none()',
      'true',
    );
  },
  'tenant_b.reservations',
);

expectFailure(
  'Reservation live proof stops checking Preview rejection',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'REPOSITORY_PREVIEW_WRITE_DENIED',
      'REMOVED',
    );
  },
  'REPOSITORY_PREVIEW_WRITE_DENIED',
);

expectFailure(
  'Reservation production guard disappears',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
  },
  'fail-closed PostgreSQL authority guard',
);

expectFailure(
  'Reservation gate restores transitional production barrier',
  (snapshot) => {
    snapshot.main +=
      '\nconst RETIRED_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n';
  },
  'must not restore the retired transitional production barrier',
);

console.log('R4-P8 Reservation write cutover mutation tests passed.');
