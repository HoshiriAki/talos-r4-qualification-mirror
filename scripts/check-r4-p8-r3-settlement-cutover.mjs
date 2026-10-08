#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  repositoryMod: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  dispatch: 'backend/src/repositories/r3_settlement_dispatch.rs',
  writer: 'backend/src/repositories/r3_settlement_postgres.rs',
  admission: 'backend/src/integration/settlement_admission.rs',
  lifecyclePg: 'backend/src/repositories/lifecycle_postgres_write.rs',
  triggerRepair:
    'backend/src/db/migrations/postgres/071_r4_r3_terminal_blocker_trigger_fix.sql',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  liveQualification:
    'backend/src/repositories/postgres/r3_settlement_qualification_tests.rs',
  main: 'backend/src/main.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8R3SettlementSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8R3SettlementSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'mod r3_settlement_dispatch;',
    'mod r3_settlement_postgres;',
    'pub use r3_settlement_dispatch::ScopedR3SettlementRepository;',
  ]) {
    if (!snapshot.repositoryMod.includes(token)) {
      errors.push(`R3 settlement module wiring missing: ${token}`);
    }
  }

  if (
    !snapshot.provider.includes(
      'r3_settlement_dispatch::ScopedR3SettlementRepository',
    )
  ) {
    errors.push('R3 settlement provider must use backend-neutral dispatch');
  }

  const routes =
    snapshot.dispatch.match(
      /PostgresR3SettlementRepository::new\(self\.scoped\)/g,
    )?.length ?? 0;
  if (routes < 16) {
    errors.push(
      `R3 settlement dispatch must route all 16 repository entrypoints (found ${routes})`,
    );
  }

  for (const token of [
    'pg_write_serializable_repository',
    'admit_settlement_charge_pg',
    'apply_action_in_pg_transaction',
    'SettlementComplete',
    'FinancialEffectState',
    'assert_financial_reservation_pg',
    'assert_order_settlement_unsealed_pg',
    'financial_balance_fully_accounted',
  ]) {
    if (!snapshot.writer.includes(token)) {
      errors.push(`R3 settlement PostgreSQL invariant missing: ${token}`);
    }
  }

  if (
    /\brusqlite\b|SqliteRepositorySession|SqliteConnectionManager/.test(
      snapshot.writer,
    )
  ) {
    errors.push('R3 settlement PostgreSQL writer must not depend on SQLite runtime types');
  }

  if (
    !snapshot.admission.includes('pub(crate) async fn admit_settlement_charge_pg')
  ) {
    errors.push('R2 Integration Fabric must expose a PostgreSQL settlement admission seam');
  }
  for (const token of [
    'EffectIntent',
    'SETTLEMENT_CHARGE_CAPABILITY',
    'assert_required_secret_references',
    'external_operations',
    'OperationState::Ready',
  ]) {
    if (!snapshot.admission.includes(token)) {
      errors.push(`R2 PostgreSQL settlement admission invariant missing: ${token}`);
    }
  }

  if (
    !snapshot.lifecyclePg.includes(
      'pub(in crate::repositories) async fn apply_action_in_pg_transaction',
    )
  ) {
    errors.push('Lifecycle PostgreSQL writer must expose caller-owned transaction seam');
  }

  for (const token of [
    'CREATE OR REPLACE FUNCTION r3_prevent_post_terminal_blocker()',
    'new_row JSONB := to_jsonb(NEW);',
    "new_row ->> 'settlement_case_id'",
    "new_row ->> 'order_id'",
  ]) {
    if (!snapshot.triggerRepair.includes(token)) {
      errors.push(`R3 terminal-blocker trigger repair invariant missing: ${token}`);
    }
  }
  if (/NEW\.settlement_case_id|NEW\.order_id/.test(snapshot.triggerRepair)) {
    errors.push('R3 terminal-blocker trigger repair must not access heterogeneous NEW fields directly');
  }

  if (!snapshot.pgMod.includes('mod r3_settlement_qualification_tests;')) {
    errors.push('R3 settlement PostgreSQL live qualification wiring missing');
  }

  for (const token of [
    'live_pg18_r3_settlement_preserves_r2_authority_terminal_seal_and_atomic_close',
    'REPOSITORY_PREVIEW_WRITE_DENIED',
    'payment.charge',
    'external_operation_state.as_deref(), Some("ready")',
    "SET state='succeeded'",
    'sealed_update.is_err()',
    'can_close_order',
    'deduction_count',
    'operation_count',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`R3 settlement PostgreSQL live proof missing: ${token}`);
    }
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('R3 Settlement production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('R3 Settlement cutover must not restore the retired transitional production barrier')
  }

  return errors;
}

function main() {
  const errors = validateP8R3SettlementSnapshot(collectP8R3SettlementSnapshot());
  if (errors.length > 0) {
    console.error('R4-P8 R3 settlement cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 R3 settlement cutover gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
