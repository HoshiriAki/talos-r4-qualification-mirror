#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export function collectAuthThrottleSource(root = ROOT) {
  return {
    policy: fs
      .readFileSync(path.join(root, 'backend/src/services/auth_rate_limit.rs'), 'utf8')
      .replace(/\r\n?/g, '\n'),
    repository: fs
      .readFileSync(path.join(root, 'backend/src/repositories/auth_security.rs'), 'utf8')
      .replace(/\r\n?/g, '\n'),
  }
}

function runtimeRustSource(source) {
  return source.split(/#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*mod\s+tests\s*\{/)[0]
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  if (start < 0 || end < 0) return ''
  return source.slice(start, end)
}

function compactRust(source) {
  return source.replace(/\s+/g, '')
}

export function validateAuthThrottleSource(snapshot) {
  const errors = []
  const source = snapshot.policy
  const runtimeSource = runtimeRustSource(source)
  const requireRuntime = (token, message) => {
    if (!runtimeSource.includes(token)) errors.push(message)
  }
  const requireEvidence = (token, message) => {
    if (!source.includes(token)) errors.push(message)
  }

  requireRuntime('LOGIN_ACCOUNT_ATTEMPT_FACTOR', 'account-wide login budget must remain explicit')
  requireRuntime('LOGIN_SOURCE_ATTEMPT_FACTOR', 'source-IP login budget must remain explicit')
  requireRuntime('rate_key_digest("login-pair"', 'login pair digest namespace is missing')
  requireRuntime('rate_key_digest("login-account"', 'login account digest namespace is missing')
  requireRuntime('rate_key_digest("login-source"', 'login source digest namespace is missing')

  const admissionBody = between(
    runtimeSource,
    'pub fn check_login(',
    'pub fn record_login_failure(',
  )
  if (!admissionBody) {
    errors.push('login admission function must remain explicit')
  } else if (
    !compactRust(admissionBody).includes(
      'self.check_hashes([&pair_hash,&account_hash,&source_hash])',
    )
  ) {
    errors.push('login admission must check pair, account and source blocks together')
  }

  const failureBody = between(
    runtimeSource,
    'pub fn record_login_failure(',
    'pub fn record_login_success(',
  )
  if (!failureBody) {
    errors.push('login failure recorder must remain explicit')
  } else {
    const compactFailure = compactRust(failureBody)
    if (!compactFailure.includes('base.with_attempt_factor(LOGIN_ACCOUNT_ATTEMPT_FACTOR)')) {
      errors.push('login failure must consume account-wide budget')
    }
    if (!compactFailure.includes('base.with_attempt_factor(LOGIN_SOURCE_ATTEMPT_FACTOR)')) {
      errors.push('login failure must consume source-IP budget')
    }
    if (!compactFailure.includes('self.repository.mutate_rate_states(')) {
      errors.push('login failure dimensions must be persisted through one repository batch mutation')
    }
  }

  requireEvidence('login_failure_dimensions_rollback_together', 'atomic rollback regression test is missing')
  requireEvidence('distributed_sources_share_account_failure_budget', 'distributed-source account guessing regression test is missing')
  requireEvidence('rotating_accounts_share_source_failure_budget', 'single-source credential-stuffing regression test is missing')

  const repositoryRateBatch = between(
    snapshot.repository,
    'pub(crate) fn mutate_rate_states(',
    'pub(crate) fn clear_rate_state(',
  )
  if (!repositoryRateBatch) {
    errors.push('auth repository must expose bounded batch rate-state mutation')
  } else {
    if (!repositoryRateBatch.includes('transaction_with_behavior(TransactionBehavior::Immediate)')) {
      errors.push('auth rate-state batch must remain inside an immediate database transaction')
    }
    if (!repositoryRateBatch.includes('for (index, key_hash) in key_hashes.iter().enumerate()')) {
      errors.push('auth repository must mutate all login dimensions inside the same transaction')
    }
    if (!repositoryRateBatch.includes('tx.commit()?')) {
      errors.push('auth repository batch must commit only after all dimensions succeed')
    }
  }

  const successStart = runtimeSource.indexOf('pub fn record_login_success(')
  const customStart = runtimeSource.indexOf('pub fn check_custom(', successStart)
  const successBody = successStart >= 0 && customStart > successStart
    ? runtimeSource.slice(successStart, customStart)
    : ''
  if (!successBody) {
    errors.push('login success recorder must remain explicit')
  } else {
    const compactSuccess = compactRust(successBody)
    if (!compactSuccess.includes('let(pair_hash,account_hash,_source_hash)=login_key_hashes(')) {
      errors.push('successful login must keep source history distinct from pair/account state')
    }
    if (compactSuccess.includes('self.clear_hash(&_source_hash)')) {
      errors.push('successful login must not erase source-IP abuse history')
    }
  }

  return errors
}

export function run(root = ROOT) {
  const errors = validateAuthThrottleSource(collectAuthThrottleSource(root))
  if (errors.length > 0) {
    console.error('R4-P7 multidimensional auth throttle FAILED')
    for (const error of errors) console.error(`- ${error}`)
    return false
  }
  console.log('R4-P7 multidimensional auth throttle PASS')
  return true
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!run()) process.exitCode = 1
}
