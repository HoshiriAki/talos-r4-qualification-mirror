#!/usr/bin/env node

import assert from 'node:assert/strict'

import {
  collectAuthThrottleSource,
  validateAuthThrottleSource,
} from './check-r4-auth-throttle-dimensions.mjs'

const baseline = collectAuthThrottleSource()
assert.deepEqual(validateAuthThrottleSource(baseline), [])

const batchCallPattern = /self\s*\.\s*repository\s*\.\s*mutate_rate_states\s*\(/

const inlineFormatting = {
  ...baseline,
  policy: baseline.policy.replace(batchCallPattern, 'self.repository.mutate_rate_states('),
}
assert.notEqual(
  inlineFormatting.policy,
  baseline.policy,
  'rustfmt parity fixture must change the batch-call layout',
)
assert.deepEqual(
  validateAuthThrottleSource(inlineFormatting),
  [],
  'equivalent member-call formatting must not change the security verdict',
)

const policyMutations = [
  ['pair-only admission is rejected', 'self.check_hashes([&pair_hash, &account_hash, &source_hash])', 'self.check_hashes([&pair_hash])'],
  ['account budget removal is rejected', 'base.with_attempt_factor(LOGIN_ACCOUNT_ATTEMPT_FACTOR)', 'base'],
  ['source budget removal is rejected', 'base.with_attempt_factor(LOGIN_SOURCE_ATTEMPT_FACTOR)', 'base'],
  ['account namespace removal is rejected', 'rate_key_digest("login-account"', 'rate_key_digest("removed-account"'],
  ['source namespace removal is rejected', 'rate_key_digest("login-source"', 'rate_key_digest("removed-source"'],
  ['atomic batch call removal is rejected', batchCallPattern, 'self.repository.mutate_rate_state_removed('],
  ['atomic rollback test removal is rejected', 'login_failure_dimensions_rollback_together', 'removed_atomic_rollback_test'],
  ['distributed guessing test removal is rejected', 'distributed_sources_share_account_failure_budget', 'removed_distributed_test'],
  ['credential stuffing test removal is rejected', 'rotating_accounts_share_source_failure_budget', 'removed_source_test'],
]

for (const [name, from, to] of policyMutations) {
  const mutatedPolicy = baseline.policy.replace(from, to)
  assert.notEqual(mutatedPolicy, baseline.policy, `${name}: mutation target not found`)
  const mutated = {
    ...baseline,
    policy: mutatedPolicy,
  }
  const errors = validateAuthThrottleSource(mutated)
  assert.ok(errors.length > 0, `${name}: expected policy failure`)
}

const crossFunctionDecoyPolicy = baseline.policy
  .replace(batchCallPattern, 'self.repository.mutate_rate_state_removed(')
  .replace(
    'fn record_failure_hash_with_policy(',
    '// decoy only: self.repository.mutate_rate_states(\n    fn record_failure_hash_with_policy(',
  )
assert.notEqual(crossFunctionDecoyPolicy, baseline.policy, 'cross-function decoy fixture must mutate policy')
assert.ok(
  validateAuthThrottleSource({ ...baseline, policy: crossFunctionDecoyPolicy }).some((error) =>
    error.includes('one repository batch mutation'),
  ),
  'batch token outside record_login_failure must not satisfy the authority check',
)

function mutateRateBatch(repository, from, to) {
  const startToken = 'pub(crate) fn mutate_rate_states('
  const endToken = 'pub(crate) fn clear_rate_state('
  const start = repository.indexOf(startToken)
  const end = repository.indexOf(endToken, start + startToken.length)
  assert.ok(start >= 0 && end > start, 'mutate_rate_states function must exist in fixture')
  const block = repository.slice(start, end)
  const mutatedBlock = block.replace(from, to)
  assert.notEqual(mutatedBlock, block, `rate batch mutation target not found: ${from}`)
  return repository.slice(0, start) + mutatedBlock + repository.slice(end)
}

const repositoryMutations = [
  ['repository immediate transaction removal is rejected', 'transaction_with_behavior(TransactionBehavior::Immediate)', 'transaction()'],
  ['repository single-transaction loop removal is rejected', 'for (index, key_hash) in key_hashes.iter().enumerate()', 'for (index, key_hash) in [].iter().enumerate()'],
  ['repository commit removal is rejected', 'tx.commit()?', '/* commit removed */'],
]

for (const [name, from, to] of repositoryMutations) {
  const mutated = {
    ...baseline,
    repository: mutateRateBatch(baseline.repository, from, to),
  }
  const errors = validateAuthThrottleSource(mutated)
  assert.ok(errors.length > 0, `${name}: expected policy failure`)
}

const successPolicy = baseline.policy.replace(
  /self\.clear_hash\(&account_hash\)\n\s*}/,
  'self.clear_hash(&account_hash)?;\n        self.clear_hash(&_source_hash)\n    }',
)
assert.notEqual(successPolicy, baseline.policy, 'success-history mutation target must exist')
const successMutation = {
  ...baseline,
  policy: successPolicy,
}
assert.ok(
  validateAuthThrottleSource(successMutation).some((error) =>
    error.includes('must not erase source-IP abuse history'),
  ),
  'successful login clearing source history must fail',
)

console.log(
  `R4-P7 auth throttle fixtures passed: ${policyMutations.length + repositoryMutations.length + 2} mutations + rustfmt parity`,
)
