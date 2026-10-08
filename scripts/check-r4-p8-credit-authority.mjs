#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/credit_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/credit.rs',
  dispatch: 'backend/src/repositories/credit_dispatch.rs',
  pgMutation: 'backend/src/repositories/credit_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/credit_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/credit_qualification_tests.rs',
  pgMigration: 'backend/src/db/migrations/postgres/078_r4_credit_score_tenant_invariant.sql',
  workflow: '.github/workflows/exact-head-qualification.yml',
}
function read(relative) { return readFileSync(path.join(ROOT, relative), 'utf8') }
function compact(source) { return source.replace(/\s+/g, '') }

export function collectCreditAuthoritySnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]))
}

export function validateCreditAuthoritySnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'CreditCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.credits()',
    '.blacklist_add(',
    '.blacklist_remove(',
    '.violation_record(',
    '.violation_appeal(',
    '.violation_review(',
    '.credit_get(',
    '.credit_recalculate(',
    '.check_before_order(',
    'FeatureCredit::new().metadata()',
    'FeatureCredit::new().commands()',
    'FeatureCredit::new().schema()',
  ]) {
    if (!app.includes(compact(token))) errors.push('Credit compatibility authority missing: ' + token)
  }
  if (snapshot.application.includes('.parse::<i64>()')) {
    errors.push('Credit compatibility must preserve string identity IDs instead of restoring integer actor parsing')
  }

  for (const token of [
    'mod credit;',
    'mod credit_dispatch;',
    'mod credit_postgres;',
    'mod credit_postgres_mutation;',
    'mod credit_postgres_read;',
    'pub use credit_dispatch::ScopedCreditRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) errors.push('Credit repository wiring missing: ' + token)
  }
  if (!snapshot.provider.includes("pub fn credits(&self) -> ScopedCreditRepository<'_>")) {
    errors.push('ScopedRepositories must expose Credit authority')
  }
  if (!snapshot.dispatch.includes('PostgresCreditRepository') || !snapshot.dispatch.includes('SqliteCreditRepository')) {
    errors.push('Credit backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM blacklist',
    'INSERT INTO violations',
    'UPDATE credit_scores',
    'tenant_id=?1',
    'sqlite_credit_authority_preserves_scope_atomic_score_and_state_machine',
  ]) {
    if (!snapshot.sqlite.includes(token)) errors.push('SQLite Credit authority invariant missing: ' + token)
  }

  for (const token of [
    'pg_write_serializable_repository',
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'FOR UPDATE',
    'ON CONFLICT (tenant_id,customer_phone) DO NOTHING',
    'GREATEST(0,score-$1)',
  ]) {
    if (!snapshot.pgMutation.includes(token)) errors.push('PostgreSQL Credit mutation invariant missing: ' + token)
  }
  for (const token of [
    'financial_penalty::double precision AS financial_penalty',
    'score::bigint',
    'on_time_returns::bigint',
    'damage_incidents::bigint',
    'QueryBuilder::<Postgres>',
    'tenant_id=',
  ]) {
    if (!snapshot.pgRead.includes(token)) errors.push('PostgreSQL Credit read invariant missing: ' + token)
  }

  if (!factory.includes('CreditCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Credit provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureCredit)')) {
    errors.push('Registry Credit restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("credit",Business,ModuleActivation::Always,NONE,Credit)')) {
    errors.push('Credit Registry descriptor must not require SQLite')
  }

  const pgMigration = compact(snapshot.pgMigration)
  if (!pgMigration.includes('DROPINDEXIFEXISTSidx_credit_scores_phone')
      || !pgMigration.includes('CREATEUNIQUEINDEXIFNOTEXISTSidx_credit_scores_tenant_phone')
      || !pgMigration.includes('ONcredit_scores(tenant_id,customer_phone)')) {
    errors.push('PostgreSQL Credit tenant identity migration invariant is incomplete')
  }

  if (!snapshot.postgresMod.includes('mod credit_qualification_tests;')) {
    errors.push('PostgreSQL Credit qualification module is not registered')
  }
  const pgTest = compact(snapshot.pgTest)
  if (!pgTest.includes('(id,username,password_hash,display_name,status,created_at,updated_at)')
      || !pgTest.includes("VALUES($1,$1,'!non-interactive',$1,'active','now','now')")) {
    errors.push('PG18 Credit identity fixture must satisfy canonical required identity fields')
  }

  for (const token of [
    'live_pg18_credit_authority_preserves_scope_atomic_score_identity_and_recomposition',
    'Err(CreditMutationError::DuplicateBlacklist)',
    'Err(CreditMutationError::ViolationNotFound)',
    'profile_a["score"], 80',
    'profile_b["score"], 100',
    'violation["reportedBy"], "credit-reporter-a"',
    'row.try_get::<String, _>("created_by")?, "credit-admin-a"',
  ]) {
    if (!snapshot.pgTest.includes(token)) errors.push('PG18 Credit authority evidence missing: ' + token)
  }

  for (const token of [
    'node scripts/check-r4-p8-credit-authority.test.mjs',
    'node scripts/check-r4-p8-credit-authority.mjs',
    'live_pg18_credit_authority_preserves_scope_atomic_score_identity_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push('Exact-head Credit qualification missing: ' + token)
  }
  return errors
}

function main() {
  const errors = validateCreditAuthoritySnapshot(collectCreditAuthoritySnapshot())
  if (errors.length) {
    console.error('R4-P8 Credit authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Credit authority gate passed.')
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
