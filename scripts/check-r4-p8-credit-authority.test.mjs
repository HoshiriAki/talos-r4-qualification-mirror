#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectCreditAuthoritySnapshot,
  validateCreditAuthoritySnapshot,
} from './check-r4-p8-credit-authority.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}
function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectCreditAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateCreditAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(errors.some(error => error.includes(needle)), name + ': ' + JSON.stringify(errors))
}

const baseline = validateCreditAuthoritySnapshot(collectCreditAuthoritySnapshot())
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure('Credit factory restores SQLite', snapshot => {
  snapshot.factory = replaceRequired(
    snapshot.factory,
    'CreditCompatibilityModule::new(repository_provider.clone())',
    'with_pool!(FeatureCredit)',
    'Credit factory restores SQLite',
  )
}, 'direct SQLite construction')

expectFailure('Credit descriptor restores SQLite', snapshot => {
  snapshot.descriptors = replaceRequired(
    snapshot.descriptors,
    'descriptor!("credit", Business, ModuleActivation::Always, NONE, Credit)',
    'descriptor!("credit", Business, ModuleActivation::Always, SQLITE, Credit)',
    'Credit descriptor restores SQLite',
  )
}, 'descriptor must not require SQLite')

expectFailure('Credit actor parsing regresses to integer', snapshot => {
  snapshot.application += '\nlet _legacy = ctx.user_id().and_then(|id| id.parse::<i64>().ok());\n'
}, 'string identity IDs')

expectFailure('Credit blacklist loses advisory lock', snapshot => {
  snapshot.pgMutation = replaceRequired(
    snapshot.pgMutation,
    'pg_advisory_xact_lock(hashtextextended($1,0))',
    'credit_lock_removed($1)',
    'Credit blacklist loses advisory lock',
  )
}, 'mutation invariant missing')

expectFailure('Credit mutations lose serialization', snapshot => {
  snapshot.pgMutation = snapshot.pgMutation.replaceAll(
    'pg_write_serializable_repository',
    'pg_write',
  )
}, 'mutation invariant missing')

expectFailure('Credit reads lose REAL normalization', snapshot => {
  assert.ok(
    snapshot.pgRead.includes('financial_penalty::double precision AS financial_penalty'),
    'Credit reads lose REAL normalization: mutation anchor missing',
  )
  snapshot.pgRead = snapshot.pgRead.replaceAll(
    'financial_penalty::double precision AS financial_penalty',
    'financial_penalty',
  )
}, 'read invariant missing')

expectFailure('Credit reads lose integer normalization', snapshot => {
  snapshot.pgRead = snapshot.pgRead.replaceAll('score::bigint', 'score')
}, 'read invariant missing')

expectFailure('Credit reads lose tenant scope', snapshot => {
  snapshot.pgRead = snapshot.pgRead.replaceAll('tenant_id=', 'tenant_scope_removed=')
}, 'read invariant missing')

expectFailure('Credit tenant identity migration restores global phone uniqueness', snapshot => {
  snapshot.pgMigration = replaceRequired(
    snapshot.pgMigration,
    'ON credit_scores(tenant_id, customer_phone)',
    'ON credit_scores(customer_phone)',
    'Credit tenant identity migration restores global phone uniqueness',
  )
}, 'tenant identity migration invariant')

expectFailure('Credit PG18 fixture loses canonical identity fields', snapshot => {
  snapshot.pgTest = replaceRequired(
    snapshot.pgTest,
    '(id,username,password_hash,display_name,status,created_at,updated_at)',
    '(id,password_hash)',
    'Credit PG18 fixture loses canonical identity fields',
  )
}, 'canonical required identity fields')

expectFailure('Credit live proof loses string actor identity', snapshot => {
  snapshot.pgTest = replaceRequired(
    snapshot.pgTest,
    'violation["reportedBy"], "credit-reporter-a"',
    'violation["reportedBy"], 1',
    'Credit live proof loses string actor identity',
  )
}, 'credit-reporter-a')

console.log('R4-P8 Credit authority mutation tests passed.')
