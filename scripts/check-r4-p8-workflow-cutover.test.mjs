#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8WorkflowSnapshot,
  validateP8WorkflowSnapshot,
} from './check-r4-p8-workflow-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8WorkflowSnapshot());
  mutate(snapshot);
  const errors = validateP8WorkflowSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8WorkflowSnapshot(collectP8WorkflowSnapshot());
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Workflow mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'Workflow provider bypasses backend dispatch',
  (snapshot) => {
    snapshot.provider = snapshot.provider.replaceAll(
      'workflow_dispatch::ScopedWorkflowRepository',
      'workflow::LegacyWorkflowRepository',
    );
  },
  'backend-neutral dispatch',
);

expectFailure(
  'Workflow PostgreSQL dispatch disappears',
  (snapshot) => {
    snapshot.dispatch = snapshot.dispatch.replaceAll(
      'PostgresWorkflowRepository::new(self.scoped.session())',
      'REMOVED_PG_WORKFLOW_ROUTE',
    );
  },
  'route all production repository entrypoints',
);

expectFailure(
  'Workflow writer loses rollback-aware serializable ownership',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'pg_write_serializable_repository',
      'pg_write_serializable',
    );
  },
  'rollback-aware SERIALIZABLE',
);

expectFailure(
  'Workflow consumer stops locking outbox work',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'FOR UPDATE SKIP LOCKED',
      'REMOVED_ROW_CLAIM',
    );
  },
  'FOR UPDATE SKIP LOCKED',
);

expectFailure(
  'Workflow inbox loses durable deduplication',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'ON CONFLICT (tenant_id,message_id) DO NOTHING',
      'ON CONFLICT DO UPDATE SET state=EXCLUDED.state',
    );
  },
  'ON CONFLICT (tenant_id,message_id) DO NOTHING',
);

expectFailure(
  'Workflow transition stops failing closed',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'illegal workflow transition',
      'REMOVED_ILLEGAL_STATE_GUARD',
    );
  },
  'illegal workflow transition',
);

expectFailure(
  'Workflow live proof drops tenant isolation',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'scoped_b.workflows().get(&first.id)?.is_none()',
      'true',
    );
  },
  'scoped_b.workflows().get',
);

expectFailure(
  'Workflow live proof drops durable event delivery evidence',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      '("delivered", "processed")',
      '("pending", "received")',
    );
  },
  '("delivered", "processed")',
);

expectFailure(
  'Workflow live proof drops Preview denial',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'REPOSITORY_PREVIEW_WRITE_DENIED',
      'REMOVED_PREVIEW_WRITE_DENIAL',
    );
  },
  'REPOSITORY_PREVIEW_WRITE_DENIED',
);

expectFailure(
  'Workflow production guard disappears',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
  },
  'fail-closed PostgreSQL authority guard',
);

expectFailure(
  'Workflow gate restores transitional production barrier',
  (snapshot) => {
    snapshot.main +=
      '\nconst RETIRED_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n';
  },
  'must not restore the retired transitional production barrier',
);

console.log('R4-P8 Workflow cutover mutation tests passed.');
