#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  integrationMod: 'backend/src/integration/mod.rs',
  contract: 'backend/src/integration/deposit_refund_persistence_contract.rs',
  sqliteAdapter: 'backend/src/integration/deposit_refund_persistence_sqlite.rs',
  postgresAdapter: 'backend/src/integration/deposit_refund_persistence_postgres.rs',
  integrationModule: 'backend/src/integration/module.rs',
  liveQualification: 'backend/src/integration/deposit_refund_persistence_postgres_tests.rs',
  fabricMigration: 'backend/src/db/migrations/postgres/059_integration_fabric.sql',
  runtimeMigration: 'backend/src/db/migrations/postgres/060_integration_runtime.sql',
  main: 'backend/src/main.rs',
}

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8')
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  return start >= 0 && end > start ? source.slice(start, end) : ''
}

export function collectP8DepositRefundSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8DepositRefundSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'pub(crate) mod deposit_refund_persistence_contract;',
    'pub(crate) mod deposit_refund_persistence_postgres;',
    'pub(crate) mod deposit_refund_persistence_sqlite;',
    'mod deposit_refund_persistence_postgres_tests;',
  ]) {
    if (!snapshot.integrationMod.includes(token)) {
      errors.push(`Deposit/Refund module wiring missing: ${token}`)
    }
  }

  for (const token of [
    'pub(crate) trait DepositRefundPersistence: Send + Sync',
    'fn create_deposit(',
    'fn record_deposit_received(',
    'fn hold_deposit(',
    'fn deduct_deposit(',
    'fn release_deposit(',
    'fn request_refund(',
    'fn link_refund_operation(',
    'fn reconcile_refund(',
  ]) {
    if (!snapshot.contract.includes(token)) {
      errors.push(`Deposit/Refund persistence contract missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::|sqlx::/.test(snapshot.contract)) {
    errors.push('Deposit/Refund persistence contract must remain backend-neutral')
  }

  if (!snapshot.sqliteAdapter.includes(
    'impl DepositRefundPersistence for IntegrationStore',
  )) {
    errors.push('SQLite Deposit/Refund compatibility adapter missing')
  }

  for (const token of [
    'finance: Arc<dyn DepositRefundPersistence>',
    'new_with_persistence(',
    'new_with_postgres_persistence(',
    'PostgresWebhookPersistence::new(pool.clone())',
    'PostgresDepositRefundPersistence::new_with_metrics',
    'self.finance.create_deposit(',
    'self.finance.request_refund(',
    'self.finance.record_deposit_received(',
    '.link_refund_operation(',
    'self.finance.reconcile_refund(',
  ]) {
    if (!snapshot.integrationModule.includes(token)) {
      errors.push(`IntegrationModule finance dependency inversion missing: ${token}`)
    }
  }
  for (const method of [
    'create_deposit',
    'request_refund',
    'record_deposit_received',
    'link_refund_operation',
    'reconcile_refund',
  ]) {
    const directStoreCall = new RegExp(
      `self\\s*\\.\\s*store\\s*\\.\\s*${method}\\s*\\(`,
    )
    if (directStoreCall.test(snapshot.integrationModule)) {
      errors.push(`IntegrationModule finance authority bypasses persistence port: ${method}`)
    }
  }

  for (const token of [
    'impl DepositRefundPersistence for PostgresDepositRefundPersistence',
    'new_with_metrics(',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'FROM integration_deposits',
    'FOR UPDATE',
    'INSERT INTO integration_deposit_ledger',
    "WHEN 'received' THEN amount_minor",
    "WHEN 'manual_adjustment' THEN amount_minor",
    "WHEN 'deducted' THEN -amount_minor",
    "WHEN 'refund_completed' THEN -amount_minor",
    'FROM refund_intents',
    "state IN ('requested','approved','dispatching','unknown_outcome')",
    'RefundAmountExceedsAvailable',
    "operation_type='refund'",
    "state='requested'",
    'integration_deposit_reconciliations',
    'DepositLedgerEntryKind::RefundCompleted',
    'DepositState::ManualResolutionRequired',
    'FinancialEvent::DepositRecorded',
    'FinancialEvent::DepositReceivedLedger',
    'FinancialEvent::DepositDeductionAdmitted',
    'FinancialEvent::RefundIntentAdmitted',
    'if admitted {',
    'FinancialEvent::RefundOperationPlanned',
    'if linked {',
    'FinancialEvent::RefundReconciliation',
  ]) {
    if (!snapshot.postgresAdapter.includes(token)) {
      errors.push(`PostgreSQL Deposit/Refund invariant missing: ${token}`)
    }
  }
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresAdapter)) {
    errors.push('PostgreSQL Deposit/Refund persistence must not depend on SQLite runtime types')
  }
  for (const forbidden of [
    'webhook_inbox',
    'integration_scheduler_cursor',
    'integration_startup_recovery_snapshot',
    'provider_manifests',
    'provider_instances',
    'provider_bindings',
  ]) {
    if (snapshot.postgresAdapter.includes(forbidden)) {
      errors.push(`PostgreSQL Deposit/Refund crossed persistence boundary: ${forbidden}`)
    }
  }

  for (const token of [
    'CREATE TABLE integration_deposits',
    'CREATE TABLE integration_deposit_ledger',
    'integration_deposit_ledger_immutable_update',
    'integration_deposit_ledger_immutable_delete',
    'CREATE TABLE refund_intents',
    'UNIQUE (tenant_id, deposit_id, idempotency_key)',
    'integration_refund_operation_atomic_link',
    'refund operation has no matching requested refund intent',
  ]) {
    if (!snapshot.fabricMigration.includes(token)) {
      errors.push(`Deposit/Refund migration authority missing: ${token}`)
    }
  }
  for (const token of [
    'CREATE TABLE integration_deposit_reconciliations',
    'FOREIGN KEY (tenant_id, refund_id) REFERENCES refund_intents',
  ]) {
    if (!snapshot.runtimeMigration.includes(token)) {
      errors.push(`Deposit reconciliation migration authority missing: ${token}`)
    }
  }

  for (const token of [
    'live_pg18_deposit_refund_preserves_ledger_idempotency_reconciliation_and_metrics',
    'PostgresDepositRefundPersistence::new_with_metrics',
    'CurrencyMismatch',
    'partially_deducted',
    'RefundAmountExceedsAvailable',
    'refund-key-a',
    'cross-tenant',
    'refund_operation',
    'link_refund_operation',
    '"approved"',
    'DepositReconciliationOutcome::Matched',
    '"completed"',
    'refund_completed',
    'matched_reconciliation_count, 1',
    'DepositReconciliationOutcome::ManualRequired',
    'manual_resolution_required',
    '"released"',
    "SET audit_ref='tampered'",
    'talos_financial_foundation_events_total',
    '!output.contains(\n            "talos_financial_foundation_events_total{event=\\\"refund_operation_planned\\\"}"\n        )',
    'refund_reconciliation',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Deposit/Refund PostgreSQL live proof missing: ${token}`)
    }
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Deposit/Refund production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Deposit/Refund cutover must not restore the retired transitional production barrier')
  }

  return errors
}

function main() {
  const errors = validateP8DepositRefundSnapshot(
    collectP8DepositRefundSnapshot(),
  )
  if (errors.length > 0) {
    console.error('R4-P8 Deposit/Refund cutover gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Deposit/Refund PostgreSQL parity gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
