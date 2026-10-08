#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  application: 'backend/src/application/refund_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/refund.rs',
  dispatch: 'backend/src/repositories/refund_dispatch.rs',
  pgRequest: 'backend/src/repositories/refund_postgres_request.rs',
  pgTransition: 'backend/src/repositories/refund_postgres_transition.rs',
  pgExecute: 'backend/src/repositories/refund_postgres_execute.rs',
  pgRead: 'backend/src/repositories/refund_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/refund_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectRefundAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateRefundAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const descriptors = compact(snapshot.descriptors)
  const factory = compact(snapshot.factory)

  for (const token of [
    'RefundCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.refunds()',
    '.request(',
    '.approve(',
    '.reject(',
    '.execute_refund(',
    '.list(',
    'FeatureRefund::new().metadata()',
    'FeatureRefund::new().commands()',
    'FeatureRefund::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Refund compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod refund;',
    'mod refund_dispatch;',
    'mod refund_postgres;',
    'mod refund_postgres_request;',
    'mod refund_postgres_transition;',
    'mod refund_postgres_execute;',
    'mod refund_postgres_read;',
    'pub use refund_dispatch::ScopedRefundRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Refund repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn refunds(&self) -> ScopedRefundRepository<'_>")) {
    errors.push('ScopedRepositories must expose Refund authority')
  }
  if (!snapshot.dispatch.includes('PostgresRefundRepository')
      || !snapshot.dispatch.includes('SqliteRefundRepository')) {
    errors.push('Refund backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM deposits',
    'FROM refunds',
    'INSERT INTO refunds',
    'INSERT INTO deposit_ledger',
    'tenant_id',
    'sqlite_refund_authority_preserves_scope_transitions_and_ledger',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Refund authority invariant missing: ' + token)
    }
  }

  for (const [name, source, tokens] of [
    ['request', snapshot.pgRequest, ['pg_write_serializable_repository', 'FROM deposits', 'FOR UPDATE', 'tenant_id=$2']],
    ['transition', snapshot.pgTransition, ['pg_write_serializable_repository', 'FROM refunds', 'FOR UPDATE', 'tenant_id=$2']],
    ['execute', snapshot.pgExecute, ['pg_write_serializable_repository', 'FROM refunds', 'FOR UPDATE', 'INSERT INTO deposit_ledger', 'tenant_id=$2']],
  ]) {
    for (const token of tokens) {
      if (!source.includes(token)) {
        errors.push('PostgreSQL Refund ' + name + ' invariant missing: ' + token)
      }
    }
  }
  if (!snapshot.pgRead.includes('QueryBuilder::<Postgres>')
      || !snapshot.pgRead.includes('r.tenant_id=')) {
    errors.push('PostgreSQL Refund list must remain tenant-scoped')
  }

  if (!factory.includes('RefundCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Refund provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureRefund)')) {
    errors.push('Registry Refund restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("refund",Business,ModuleActivation::Always,NONE,Refund)')) {
    errors.push('Refund Registry descriptor must not require SQLite')
  }

  if (!snapshot.postgresMod.includes('mod refund_qualification_tests;')) {
    errors.push('PostgreSQL Refund qualification module is not registered')
  }
  for (const token of [
    'live_pg18_refund_authority_preserves_scope_transitions_ledger_and_recomposition',
    'Err(RefundMutationError::NotFound)',
    'Err(RefundMutationError::NotApproved(status)) if status == "pending"',
    'vec!["refund"]',
    'persisted.total, 2',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Refund authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-refund-authority.test.mjs',
    'node scripts/check-r4-p8-refund-authority.mjs',
    'live_pg18_refund_authority_preserves_scope_transitions_ledger_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Refund qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateRefundAuthoritySnapshot(collectRefundAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Refund authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Refund authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
