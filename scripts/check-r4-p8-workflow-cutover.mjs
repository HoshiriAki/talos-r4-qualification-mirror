#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  dispatch: 'backend/src/repositories/workflow_dispatch.rs',
  writer: 'backend/src/repositories/workflow_postgres.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/workflow_qualification_tests.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8WorkflowSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8WorkflowSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'mod workflow_dispatch;',
    'mod workflow_postgres;',
    'pub use workflow_dispatch::ScopedWorkflowRepository;',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`Workflow PostgreSQL module wiring missing: ${token}`);
    }
  }

  if (!snapshot.provider.includes(
    'use crate::repositories::workflow_dispatch::ScopedWorkflowRepository;',
  )) {
    errors.push('Workflow provider must use backend-neutral dispatch');
  }

  const dispatchCount =
    snapshot.dispatch.match(/PostgresWorkflowRepository::new\(self\.scoped\.session\(\)\)/g)
      ?.length ?? 0;
  if (dispatchCount < 11) {
    errors.push(
      `Workflow backend dispatch must route all production repository entrypoints (found ${dispatchCount})`,
    );
  }
  if (!snapshot.dispatch.includes('SqliteWorkflowRepository::new(self.scoped)')) {
    errors.push('Workflow dispatch must preserve the SQLite local implementation path');
  }

  for (const token of [
    'pub(in crate::repositories) struct PostgresWorkflowRepository',
    'INSERT INTO domain_inbox',
    'ON CONFLICT (tenant_id,message_id) DO NOTHING',
    'consume_domain_events',
    'FOR UPDATE SKIP LOCKED',
    'claim_due',
    'illegal workflow transition',
    'workflow_blockers',
    'manual_decision_tasks',
    "state='delivered'",
    "state='processed'",
  ]) {
    if (!snapshot.writer.includes(token)) {
      errors.push(`Workflow PostgreSQL invariant missing: ${token}`);
    }
  }

  if (!/self\.session\s*\.pg_write_serializable_repository\s*\(/s.test(snapshot.writer)) {
    errors.push(
      'Workflow PostgreSQL writer must use rollback-aware SERIALIZABLE repository transactions',
    );
  }

  if (!/WHERE tenant_id=\$1/.test(snapshot.writer)) {
    errors.push('Workflow PostgreSQL writer must retain tenant-scoped mutation predicates');
  }

  if (/\brusqlite\b|SqliteRepositorySession|SqliteConnectionManager/.test(snapshot.writer)) {
    errors.push('Workflow PostgreSQL writer must not depend on SQLite runtime types');
  }

  if (!snapshot.pgMod.includes('mod workflow_qualification_tests;')) {
    errors.push('Workflow PostgreSQL live qualification wiring missing');
  }

  for (const token of [
    'live_pg18_workflow_preserves_durable_identity_claims_resolution_and_scope',
    'assert_eq!(first.id, duplicate.id);',
    'scoped_b.workflows().get(&first.id)?.is_none()',
    'WorkflowStepState::RetryScheduled',
    'illegal workflow transition',
    'resolve_blocker',
    'resolve_manual_task',
    '("delivered", "processed")',
    '"REPOSITORY_PREVIEW_WRITE_DENIED"',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Workflow PostgreSQL live qualification proof missing: ${token}`);
    }
  }

  if (
    !/assert_eq!\(\s*scoped_a\.workflows\(\)\.steps\(&first\.id\)\?\.len\(\),\s*RENTAL_STEPS\.len\(\)\s*,?\s*\);/s.test(
      snapshot.liveQualification,
    )
  ) {
    errors.push('Workflow PostgreSQL live qualification proof missing: durable step count');
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push(
      'Workflow production cutover must retain the fail-closed PostgreSQL authority guard',
    );
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push(
      'Workflow cutover must not restore the retired transitional production barrier',
    );
  }

  return errors;
}

function main() {
  const errors = validateP8WorkflowSnapshot(collectP8WorkflowSnapshot());
  if (errors.length > 0) {
    console.error('R4-P8 Workflow cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 Workflow cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
