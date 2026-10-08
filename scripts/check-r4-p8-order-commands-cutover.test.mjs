#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8OrderCommandsSnapshot,
  validateP8OrderCommandsSnapshot,
} from './check-r4-p8-order-commands-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8OrderCommandsSnapshot());
  mutate(snapshot);
  const errors = validateP8OrderCommandsSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8OrderCommandsSnapshot(collectP8OrderCommandsSnapshot());
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Order Commands mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'Order Commands provider bypasses backend dispatch',
  (snapshot) => {
    snapshot.provider = snapshot.provider.replaceAll(
      'order_commands_dispatch::ScopedOrderCommandRepository',
      'order_commands::LegacyOrderCommandRepository',
    );
  },
  'backend-neutral dispatch',
);

expectFailure(
  'Order Commands PostgreSQL dispatch disappears',
  (snapshot) => {
    snapshot.dispatch = snapshot.dispatch.replaceAll(
      'PostgresOrderCommandRepository::new(self.scoped.session())',
      'REMOVED_ORDER_COMMAND_WRITER',
    );
  },
  'backend dispatch missing',
);

expectFailure(
  'Order Commands writer loses rollback-aware serializable ownership',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'pg_write_serializable_repository',
      'pg_write_serializable',
    );
  },
  'rollback-aware SERIALIZABLE',
);

expectFailure(
  'Order Commands writer stops checking global orderNo collision',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'SELECT tenant_id FROM orders WHERE orderno = $1 LIMIT 1 FOR SHARE',
      'SELECT tenant_id FROM orders WHERE id = $1',
    );
  },
  'SELECT tenant_id FROM orders WHERE orderno',
);

expectFailure(
  'Order Commands writer drops lifecycle row lock',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll('FOR UPDATE OF l, o', 'REMOVED_ROW_LOCK');
  },
  'FOR UPDATE OF l, o',
);

expectFailure(
  'Order Commands writer loses tenant-scoped lifecycle lookup',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'WHERE l.tenant_id = $1 AND l.order_id = $2',
      'WHERE l.order_id = $2',
    );
  },
  'WHERE l.tenant_id = $1 AND l.order_id = $2',
);

expectFailure(
  'Order Commands writer stops mapping unique orderNo collisions',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll('23505', 'REMOVED_UNIQUE_SQLSTATE');
  },
  '23505',
);

expectFailure(
  'Order Commands live proof stops checking Preview denial',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'REPOSITORY_PREVIEW_WRITE_DENIED',
      'REMOVED_PREVIEW_DENIAL',
    );
  },
  'REPOSITORY_PREVIEW_WRITE_DENIED',
);

expectFailure(
  'Order Commands live proof stops checking non-draft immutability',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(final_notes, "updated notes");',
      'assert!(true);',
    );
  },
  'final_notes',
);

expectFailure(
  'Order Commands production guard disappears',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
  },
  'fail-closed PostgreSQL authority guard',
);

expectFailure(
  'Order Commands gate restores transitional production barrier',
  (snapshot) => {
    snapshot.main +=
      '\nconst RETIRED_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n';
  },
  'must not restore the retired transitional production barrier',
);

console.log('R4-P8 Order Commands cutover mutation tests passed.');
