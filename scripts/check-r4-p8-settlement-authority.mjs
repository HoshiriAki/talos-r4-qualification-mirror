#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/settlement_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/settlement.rs',
  dispatch: 'backend/src/repositories/settlement_dispatch.rs',
  pgMutation: 'backend/src/repositories/settlement_postgres_mutation.rs',
  pgRead: 'backend/src/repositories/settlement_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/settlement_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectSettlementAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateSettlementAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'SettlementCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.settlements()',
    '.generate(',
    '.confirm(',
    '.get_by_period(',
    '.get_by_id(',
    '.list(',
    '.revenue_details(',
    'FeatureSettlement::new().metadata()',
    'FeatureSettlement::new().commands()',
    'FeatureSettlement::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Settlement compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod settlement;',
    'mod settlement_dispatch;',
    'mod settlement_postgres;',
    'mod settlement_postgres_mutation;',
    'mod settlement_postgres_read;',
    'pub use settlement_dispatch::ScopedSettlementRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Settlement repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn settlements(&self) -> ScopedSettlementRepository<'_>")) {
    errors.push('ScopedRepositories must expose Settlement authority')
  }
  if (!snapshot.dispatch.includes('PostgresSettlementRepository')
      || !snapshot.dispatch.includes('SqliteSettlementRepository')) {
    errors.push('Settlement backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM revenue_records',
    'FROM deposit_ledger',
    'tenant_id=?1',
    'confirmed=0',
    'sqlite_settlement_authority_preserves_scope_reaggregation_and_confirmation_freeze',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Settlement authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'FROM settlements',
    'FOR UPDATE',
    'FROM revenue_records',
    'FROM deposit_ledger',
    'tenant_id=$1',
    'COALESCE(SUM(amount),0)::double precision',
    'confirmed=0',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Settlement mutation invariant missing: ' + token)
    }
  }

  if (!snapshot.pgRead.includes('QueryBuilder::<Postgres>')
      || !snapshot.pgRead.includes('tenant_id=')
      || !snapshot.pgRead.includes('total_revenue::double precision')
      || !snapshot.pgRead.includes('total_deposits::double precision')
      || !snapshot.pgRead.includes('total_refunds::double precision')
      || !snapshot.pgRead.includes('(confirmed <> 0) AS confirmed')) {
    errors.push('PostgreSQL Settlement reads must remain tenant-scoped and type-normalized')
  }

  if (!factory.includes('SettlementCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Settlement provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureSettlement)')) {
    errors.push('Registry Settlement restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("settlement",Business,ModuleActivation::Always,NONE,Settlement)')) {
    errors.push('Settlement Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod settlement_qualification_tests;')) {
    errors.push('PostgreSQL Settlement qualification module is not registered')
  }
  for (const token of [
    'live_pg18_settlement_authority_preserves_scope_reaggregation_confirmation_and_recomposition',
    'first.total_revenue, Some(100.0)',
    'other.total_revenue, Some(900.0)',
    'Err(SettlementMutationError::NotFound)',
    'Err(SettlementMutationError::AlreadyConfirmed)',
    'regenerated.total_revenue, Some(140.0)',
    'frozen.total_revenue, None',
    'persisted.total_revenue, 140.0',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Settlement authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-settlement-authority.test.mjs',
    'node scripts/check-r4-p8-settlement-authority.mjs',
    'live_pg18_settlement_authority_preserves_scope_reaggregation_confirmation_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Settlement qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateSettlementAuthoritySnapshot(collectSettlementAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Settlement authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Settlement authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
