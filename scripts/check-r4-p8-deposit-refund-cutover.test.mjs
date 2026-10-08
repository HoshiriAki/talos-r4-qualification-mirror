#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8DepositRefundSnapshot,
  validateP8DepositRefundSnapshot,
} from './check-r4-p8-deposit-refund-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8DepositRefundSnapshot())
  mutate(snapshot)
  const errors = validateP8DepositRefundSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8DepositRefundSnapshot(
  collectP8DepositRefundSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Deposit/Refund mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Deposit Refund persistence port disappears',
  snapshot => {
    snapshot.contract = snapshot.contract.replaceAll(
      'pub(crate) trait DepositRefundPersistence: Send + Sync',
      'pub(crate) trait RemovedFinancePersistence: Send + Sync',
    )
  },
  'DepositRefundPersistence',
)

expectFailure(
  'IntegrationModule finance field regains concrete Store authority',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'finance: Arc<dyn DepositRefundPersistence>',
      'finance_store: IntegrationStore',
    )
  },
  'finance: Arc<dyn DepositRefundPersistence>',
)

expectFailure(
  'IntegrationModule create deposit bypasses finance port',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'self.finance.create_deposit(',
      'self.store.create_deposit(',
    )
  },
  'bypasses persistence port',
)

expectFailure(
  'Combined PostgreSQL persistence seam regresses Webhook to SQLite',
  snapshot => {
    snapshot.integrationModule = snapshot.integrationModule.replaceAll(
      'PostgresWebhookPersistence::new(pool.clone())',
      'IntegrationStore::new_REMOVED',
    )
  },
  'PostgresWebhookPersistence::new(pool.clone())',
)

expectFailure(
  'PostgreSQL finance loses SERIALIZABLE isolation',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL finance loses deposit row locking',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'FOR UPDATE',
      'NO_ROW_LOCK',
    )
  },
  'FOR UPDATE',
)

expectFailure(
  'Deposit ledger append disappears',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'INSERT INTO integration_deposit_ledger',
      'INSERT INTO removed_financial_ledger',
    )
  },
  'integration_deposit_ledger',
)

expectFailure(
  'Refund reservation stops counting unknown outcomes',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "state IN ('requested','approved','dispatching','unknown_outcome')",
      "state IN ('requested','approved','dispatching')",
    )
  },
  'unknown_outcome',
)

expectFailure(
  'Refund linkage accepts non-refund operations',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      "operation_type='refund'",
      "operation_type='charge'",
    )
  },
  "operation_type='refund'",
)

expectFailure(
  'Matched reconciliation stops appending refund ledger effect',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'DepositLedgerEntryKind::RefundCompleted',
      'DepositLedgerEntryKind::Released',
    )
  },
  'RefundCompleted',
)

expectFailure(
  'Manual reconciliation stops fencing deposit state',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'DepositState::ManualResolutionRequired',
      'DepositState::Released',
    )
  },
  'ManualResolutionRequired',
)

expectFailure(
  'Duplicate refund request starts emitting admission metrics',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'if admitted {',
      'if true {',
    )
  },
  'if admitted',
)

expectFailure(
  'Idempotent refund linkage starts emitting planning metrics',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'if linked {',
      'if true {',
    )
  },
  'if linked',
)

expectFailure(
  'Live proof stops checking idempotent linkage metric suppression',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      '!output.contains(\n            "talos_financial_foundation_events_total{event=\\\"refund_operation_planned\\\"}"\n        )',
      'output.contains(\n            "talos_financial_foundation_events_total{event=\\\"refund_operation_planned\\\"}"\n        )',
    )
  },
  'refund_operation_planned',
)

expectFailure(
  'Finance metrics lose refund reconciliation event',
  snapshot => {
    snapshot.postgresAdapter = snapshot.postgresAdapter.replaceAll(
      'FinancialEvent::RefundReconciliation',
      'FinancialEvent::DepositRecorded',
    )
  },
  'RefundReconciliation',
)

expectFailure(
  'Ledger immutable update trigger disappears',
  snapshot => {
    snapshot.fabricMigration = snapshot.fabricMigration.replaceAll(
      'integration_deposit_ledger_immutable_update',
      'removed_ledger_update_guard',
    )
  },
  'integration_deposit_ledger_immutable_update',
)

expectFailure(
  'Refund operation atomic linkage trigger disappears',
  snapshot => {
    snapshot.fabricMigration = snapshot.fabricMigration.replaceAll(
      'integration_refund_operation_atomic_link',
      'removed_refund_link_guard',
    )
  },
  'integration_refund_operation_atomic_link',
)

expectFailure(
  'Live proof stops checking currency mismatch',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'CurrencyMismatch',
      'Persistence',
    )
  },
  'CurrencyMismatch',
)

expectFailure(
  'Live proof stops checking reserved balance rejection',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'RefundAmountExceedsAvailable',
      'InvalidOperationTransition',
    )
  },
  'RefundAmountExceedsAvailable',
)

expectFailure(
  'Live proof stops checking ledger immutability',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      "SET audit_ref='tampered'",
      "SELECT audit_ref FROM integration_deposit_ledger",
    )
  },
  'tampered',
)

expectFailure(
  'Live proof stops checking finance metrics',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'talos_financial_foundation_events_total',
      'removed_financial_metric',
    )
  },
  'talos_financial_foundation_events_total',
)

expectFailure(
  'Deposit/Refund production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Deposit/Refund gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Deposit/Refund cutover mutation tests passed.')
