#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  application: 'backend/src/application/deposit_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/deposit.rs',
  dispatch: 'backend/src/repositories/deposit_dispatch.rs',
  postgres: 'backend/src/repositories/deposit_postgres.rs',
  pgCalculate: 'backend/src/repositories/deposit_postgres_calculate.rs',
  pgCollect: 'backend/src/repositories/deposit_postgres_collect.rs',
  pgRelease: 'backend/src/repositories/deposit_postgres_release.rs',
  pgForfeit: 'backend/src/repositories/deposit_postgres_forfeit.rs',
  pgRead: 'backend/src/repositories/deposit_postgres_read.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  legacy: 'backend/official/finance/src/deposit.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/deposit_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectDepositAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateDepositAuthoritySnapshot(snapshot) {
  const errors = []
  const application = compact(snapshot.application)
  const descriptors = compact(snapshot.descriptors)
  const factory = compact(snapshot.factory)
  const legacy = compact(snapshot.legacy)

  for (const token of [
    'DepositCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.deposits()',
    '.calculate(',
    '.collect(',
    '.release(',
    '.forfeit(',
    '.get(',
    'FeatureDeposit::new().metadata()',
    'FeatureDeposit::new().commands()',
    'FeatureDeposit::new().schema()',
  ]) {
    if (!application.includes(compact(token))) {
      errors.push('Deposit compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod deposit;',
    'mod deposit_dispatch;',
    'mod deposit_postgres;',
    'mod deposit_postgres_calculate;',
    'mod deposit_postgres_collect;',
    'mod deposit_postgres_forfeit;',
    'mod deposit_postgres_read;',
    'mod deposit_postgres_release;',
    'pub use deposit_dispatch::ScopedDepositRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Deposit repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn deposits(&self) -> ScopedDepositRepository<'_>")) {
    errors.push('ScopedRepositories must expose Deposit authority')
  }

  if (!snapshot.dispatch.includes('PostgresDepositRepository')
      || !snapshot.dispatch.includes('SqliteDepositRepository')) {
    errors.push('Deposit backend dispatch is incomplete')
  }

  for (const token of [
    'write_immediate',
    'FROM deposits',
    'JOIN devices d ON d.serialNo=od.serialNo',
    'INSERT INTO deposit_ledger',
    'INSERT INTO accounting_entries',
    'tenant_id',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Deposit authority invariant missing: ' + token)
    }
  }

  if (snapshot.sqlite.includes('d.deviceSerialNo')) {
    errors.push('SQLite Deposit device count must use devices.serialNo schema truth')
  }

  for (const [name, source] of [
    ['calculate', snapshot.pgCalculate],
    ['collect', snapshot.pgCollect],
    ['release', snapshot.pgRelease],
    ['forfeit', snapshot.pgForfeit],
  ]) {
    for (const token of [
      'pg_write_serializable_repository',
      'FOR UPDATE',
      'tenant_id=$2',
    ]) {
      if (!source.includes(token)) {
        errors.push('PostgreSQL Deposit ' + name + ' invariant missing: ' + token)
      }
    }
  }
  if (!snapshot.pgRead.includes('FROM deposits') || !snapshot.pgRead.includes('tenant_id=$2')) {
    errors.push('PostgreSQL Deposit read is not tenant scoped')
  }

  if (!factory.includes('DepositCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Deposit provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureDeposit)')) {
    errors.push('Registry Deposit restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("deposit",Business,ModuleActivation::Always,NONE,Deposit)')) {
    errors.push('Deposit Registry descriptor must not require SQLite')
  }

  if (!legacy.includes('"calculate",AccessRequirement::Authenticated,&[EffectClass::DatabaseRead,EffectClass::DatabaseWrite],SimulationSupport::Blocked')) {
    errors.push('Deposit calculate effect metadata must declare its conditional write and block simulation')
  }

  if (!snapshot.postgresMod.includes('mod deposit_qualification_tests;')) {
    errors.push('PostgreSQL Deposit qualification module is not registered')
  }

  for (const token of [
    'live_pg18_deposit_authority_preserves_scope_ledger_and_recomposition',
    'assert_eq!(calculated.device_count, Some(2))',
    'Err(DepositMutationError::OrderNotFound)',
    'vec!["collect", "forfeit", "release"]',
    'deposit record survives provider recomposition',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Deposit authority evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-deposit-authority.test.mjs',
    'node scripts/check-r4-p8-deposit-authority.mjs',
    'live_pg18_deposit_authority_preserves_scope_ledger_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Deposit qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateDepositAuthoritySnapshot(collectDepositAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Deposit authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Deposit authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
