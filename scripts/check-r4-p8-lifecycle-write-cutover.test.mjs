#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8LifecycleWriteSnapshot,
  validateP8LifecycleWriteSnapshot,
} from './check-r4-p8-lifecycle-write-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8LifecycleWriteSnapshot());
  mutate(snapshot);
  const errors = validateP8LifecycleWriteSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8LifecycleWriteSnapshot(
  collectP8LifecycleWriteSnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before lifecycle write mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'Lifecycle writer loses SERIALIZABLE transaction ownership',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace(
      /self\.session\s*\.pg_write_serializable_repository\s*\(/s,
      'self.session.pg_write(',
    );
  },
  'pg_write_serializable_repository',
);

expectFailure(
  'Lifecycle writer stops locking the lifecycle row',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace('FOR UPDATE', '');
  },
  'FOR UPDATE',
);

expectFailure(
  'Lifecycle update loses tenant authority',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace(
      'WHERE tenant_id = $8 AND order_id = $9 AND version = $10',
      'WHERE order_id = $9 AND version = $10',
    );
  },
  'WHERE tenant_id = $8 AND order_id = $9 AND version = $10',
);

expectFailure(
  'Lifecycle update loses optimistic version predicate',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace(
      'WHERE tenant_id = $8 AND order_id = $9 AND version = $10',
      'WHERE tenant_id = $8 AND order_id = $9',
    );
  },
  'WHERE tenant_id = $8 AND order_id = $9 AND version = $10',
);

expectFailure(
  'Lifecycle writer drops history durability',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace(
      'INSERT INTO order_lifecycle_history',
      'INSERT INTO removed_history',
    );
  },
  'INSERT INTO order_lifecycle_history',
);

expectFailure(
  'Lifecycle confirmation drops durable outbox',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace(
      'INSERT INTO domain_outbox',
      'INSERT INTO removed_outbox',
    );
  },
  'INSERT INTO domain_outbox',
);

expectFailure(
  'Lifecycle outbox loses idempotency conflict authority',
  (snapshot) => {
    snapshot.lifecycleWrite = snapshot.lifecycleWrite.replace(
      'ON CONFLICT (tenant_id, idempotency_key) DO NOTHING',
      '',
    );
  },
  'ON CONFLICT (tenant_id, idempotency_key) DO NOTHING',
);

expectFailure(
  'Lifecycle dispatch bypasses the PostgreSQL writer',
  (snapshot) => {
    snapshot.lifecycleDispatch = snapshot.lifecycleDispatch.replace(
      'PostgresLifecycleWriteRepository::new(self.scoped.session()).apply_action(',
      'removed_lifecycle_writer(',
    );
  },
  'Lifecycle PostgreSQL write dispatch missing',
);

expectFailure(
  'Lifecycle writer gains SQLite coupling',
  (snapshot) => {
    snapshot.lifecycleWrite += '\nuse rusqlite::Connection;\n';
  },
  'must not depend on SQLite',
);

expectFailure(
  'Lifecycle live proof stops checking stale version rejection',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      '"REPOSITORY_CONTRACT_VIOLATION"',
      '"REMOVED"',
    );
  },
  'live qualification proof missing',
);

expectFailure(
  'Lifecycle live proof stops checking cross-tenant lifecycle isolation',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      'tenant_b.lifecycles().get("lifecycle-order-a")?.is_none()',
      'true',
    );
  },
  'live qualification proof missing',
);

expectFailure(
  'Lifecycle live proof stops checking cross-tenant history isolation',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      /tenant_b\s*\.lifecycles\(\)\s*\.history\(\s*"lifecycle-order-a"\s*\)\?\s*\.is_empty\(\)/s,
      'true',
    );
  },
  'tenant B history isolation assertion',
);

expectFailure(
  'Lifecycle live proof stops checking Preview rejection',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      '"REPOSITORY_PREVIEW_WRITE_DENIED"',
      '"REMOVED"',
    );
  },
  'live qualification proof missing',
);

expectFailure(
  'Lifecycle production guard disappears',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
  },
  'fail-closed PostgreSQL authority guard',
);

expectFailure(
  'Lifecycle gate restores transitional production barrier',
  (snapshot) => {
    snapshot.main +=
      '\nconst RETIRED_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n';
  },
  'must not restore the retired transitional production barrier',
);

console.log('R4-P8 lifecycle write cutover mutation tests passed.');
