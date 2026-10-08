#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/overdue_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/overdue.rs',
  dispatch: 'backend/src/repositories/overdue_dispatch.rs',
  pgMutation: 'backend/src/repositories/overdue_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/overdue_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/overdue_qualification_tests.rs',
  pgMigration: 'backend/src/db/migrations/postgres/079_r4_overdue_tenant_invariant.sql',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}
function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectOverdueAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateOverdueAuthoritySnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)
  const sqlite = compact(snapshot.sqlite)
  const pgMigration = compact(snapshot.pgMigration)

  for (const token of [
    'OverdueCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.overdues()',
    '.detect(',
    '.calc(',
    '.apply_delegated(',
    '.waive(',
    '.list(',
    '.get(',
    '.config_get(',
    '.config_upsert(',
    '.escalate(',
    '.escalation_history(',
    '.stats(',
    '.check_before_order(',
    'FeatureOverdue::new().metadata()',
    'FeatureOverdue::new().commands()',
    'FeatureOverdue::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Overdue compatibility authority missing: ' + token)
    }
  }
  if (!app.includes(compact('R3 settlement owns additional-charge effects'))) {
    errors.push('Overdue compatibility must preserve R3 settlement financial delegation')
  }

  for (const token of [
    'mod overdue;',
    'mod overdue_dispatch;',
    'mod overdue_postgres;',
    'mod overdue_postgres_mutation;',
    'mod overdue_postgres_read;',
    'pub use overdue_dispatch::ScopedOverdueRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Overdue repository wiring missing: ' + token)
    }
  }
  if (!snapshot.provider.includes("pub fn overdues(&self) -> ScopedOverdueRepository<'_>")) {
    errors.push('ScopedRepositories must expose Overdue authority')
  }
  if (!snapshot.dispatch.includes('PostgresOverdueRepository')
      || !snapshot.dispatch.includes('SqliteOverdueRepository')) {
    errors.push('Overdue backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'tenant_id=?1',
    'apply_delegated',
    '"financialEffectApplied":false',
    '"settlementAuthority":"r3_settlement"',
    'sqlite_overdue_authority_preserves_scope_identity_escalation_and_financial_delegation',
  ]) {
    if (!sqlite.includes(compact(token))) {
      errors.push('SQLite Overdue authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'FOR UPDATE',
    'ON CONFLICT(tenant_id,overdue_id,escalation_level) DO NOTHING',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Overdue mutation invariant missing: ' + token)
    }
  }

  for (const token of [
    'QueryBuilder::<Postgres>',
    'tenant_id=',
    'days_overdue::bigint AS days_overdue',
    'total_fee::double precision AS total_fee',
    'daily_rate::double precision AS daily_rate',
    'COALESCE(SUM(total_fee-waived_amount-paid_amount),0)::double precision',
  ]) {
    if (!snapshot.pgRead.includes(token)) {
      errors.push('PostgreSQL Overdue read invariant missing: ' + token)
    }
  }

  if (!factory.includes('OverdueCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Overdue provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureOverdue)')) {
    errors.push('Registry Overdue restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("overdue",Business,ModuleActivation::Always,NONE,Overdue)')) {
    errors.push('Overdue Registry descriptor must not require SQLite')
  }

  for (const token of [
    'ALTER TABLE overdue_records ALTER COLUMN tenant_id SET NOT NULL',
    'ALTER TABLE overdue_fee_config ALTER COLUMN tenant_id SET NOT NULL',
    'ALTER TABLE overdue_notification_log ADD COLUMN IF NOT EXISTS tenant_id TEXT',
    'UPDATE overdue_notification_log AS notification SET tenant_id = records.tenant_id FROM overdue_records AS records',
    'ALTER TABLE overdue_notification_log ALTER COLUMN tenant_id SET NOT NULL',
    'CREATE UNIQUE INDEX IF NOT EXISTS idx_overdue_fee_config_tenant_unique ON overdue_fee_config(tenant_id)',
    'CREATE UNIQUE INDEX IF NOT EXISTS idx_overdue_notification_tenant_level_unique ON overdue_notification_log(tenant_id,overdue_id,escalation_level)',
  ]) {
    if (!pgMigration.includes(compact(token))) {
      errors.push('PostgreSQL Overdue tenant invariant migration missing: ' + token)
    }
  }

  if (!snapshot.postgresMod.includes('mod overdue_qualification_tests;')) {
    errors.push('PostgreSQL Overdue qualification module is not registered')
  }
  for (const token of [
    'live_pg18_overdue_authority_preserves_scope_identity_escalation_delegation_and_recomposition',
    'detected_a["records"][0]["orderId"], "overdue-order-a"',
    'detected_b["records"][0]["orderId"], "overdue-order-b"',
    '"financialEffectApplied"], false',
    '"settlementAuthority"], "r3_settlement"',
    'FROM order_price_details AS price',
    'JOIN orders AS scoped_order ON scoped_order.id=price.orderid',
    "scoped_order.tenant_id='tenant-overdue-a'",
    'INSERT INTO identities',
    "('identity-overdue-admin','overdue-admin','hash','Overdue Admin','active','now','now')",
    '"identity-overdue-admin"',
    '079_r4_overdue_tenant_invariant',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Overdue authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-overdue-authority.test.mjs',
    'node scripts/check-r4-p8-overdue-authority.mjs',
    'live_pg18_overdue_authority_preserves_scope_identity_escalation_delegation_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Overdue qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateOverdueAuthoritySnapshot(collectOverdueAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Overdue authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Overdue authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
