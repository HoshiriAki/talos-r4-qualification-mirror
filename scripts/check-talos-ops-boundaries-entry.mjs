#!/usr/bin/env node

import { pathToFileURL } from 'node:url'
import process from 'node:process'
import {
  BASELINE_COMMIT,
  runRepositoryCheck,
} from './check-talos-ops-boundaries.mjs'

export const BASELINE_SHA_ENV = 'TALOS_OPS_BASELINE_SHA'
export const BASELINE_LOCAL_COMMIT_ENV = 'TALOS_OPS_BASELINE_LOCAL_COMMIT'
export const STRICT_ZERO_DEBT_ENV = 'TALOS_OPS_STRICT_ZERO_DEBT'

function present(value) {
  return typeof value === 'string' && value.trim().length > 0
}

export function resolveBaselineProjection(options) {
  const sourceSha = options === undefined
    ? process.env[BASELINE_SHA_ENV]
    : options.sourceSha
  const localCommit = options === undefined
    ? process.env[BASELINE_LOCAL_COMMIT_ENV]
    : options.localCommit
  const strictZeroDebt = options === undefined
    ? process.env[STRICT_ZERO_DEBT_ENV] === '1'
    : options.strictZeroDebt === true
  const hasSourceSha = present(sourceSha)
  const hasLocalCommit = present(localCommit)

  if (strictZeroDebt) {
    if (hasSourceSha || hasLocalCommit) {
      throw new Error(`${STRICT_ZERO_DEBT_ENV} cannot be combined with ${BASELINE_SHA_ENV} or ${BASELINE_LOCAL_COMMIT_ENV}`)
    }
    return {
      authorityCommit: BASELINE_COMMIT,
      comparisonCommit: null,
      mode: 'strict-zero-debt',
    }
  }

  if (hasSourceSha !== hasLocalCommit) {
    throw new Error(`${BASELINE_SHA_ENV} and ${BASELINE_LOCAL_COMMIT_ENV} must be provided together`)
  }

  if (!hasSourceSha) {
    return {
      authorityCommit: BASELINE_COMMIT,
      comparisonCommit: BASELINE_COMMIT,
      mode: 'git-object',
    }
  }

  if (sourceSha !== BASELINE_COMMIT) {
    throw new Error(`${BASELINE_SHA_ENV} must equal fingerprint baseline ${BASELINE_COMMIT}`)
  }

  if (!/^[0-9a-f]{40}$/i.test(localCommit)) {
    throw new Error(`${BASELINE_LOCAL_COMMIT_ENV} must be a full 40-character Git object id`)
  }

  const comparisonCommit = localCommit.toLowerCase()
  if (comparisonCommit === BASELINE_COMMIT.toLowerCase()) {
    return {
      authorityCommit: BASELINE_COMMIT,
      comparisonCommit: BASELINE_COMMIT,
      mode: 'git-object',
    }
  }

  return {
    authorityCommit: BASELINE_COMMIT,
    comparisonCommit,
    mode: 'exact-archive-projection',
  }
}

export function runProjectedRepositoryCheck(options) {
  const projection = resolveBaselineProjection(options)
  const result = runRepositoryCheck({
    baselineCommit: projection.comparisonCommit ?? BASELINE_COMMIT,
    strictZeroDebt: projection.mode === 'strict-zero-debt',
  })
  return {
    ...result,
    authorityBaselineCommit: projection.authorityCommit,
    comparisonBaselineCommit: projection.comparisonCommit,
    baselineMode: projection.mode,
  }
}

function main() {
  let result
  try {
    result = runProjectedRepositoryCheck()
  } catch (error) {
    console.error(`TALOS Operations baseline projection failed closed: ${error instanceof Error ? error.message : String(error)}`)
    process.exitCode = 1
    return
  }

  if (process.argv.includes('--print-baseline')) {
    console.log(JSON.stringify(result.baselineFindings, null, 2))
    return
  }

  const projectionSuffix = result.baselineMode === 'git-object'
    ? ''
    : result.baselineMode === 'strict-zero-debt'
      ? ' via strict zero-debt projection'
      : ` via local projection ${result.comparisonBaselineCommit}`

  if (result.failures.length > 0) {
    console.error(`TALOS Operations boundary check failed against ${result.authorityBaselineCommit}${projectionSuffix}:`)
    for (const failure of result.failures) console.error(`- ${failure.rule} ${failure.path}: ${failure.evidence} (+${failure.occurrences})`)
    process.exitCode = 1
    return
  }

  console.log(`TALOS Operations boundary check passed against fingerprint baseline ${result.authorityBaselineCommit}${projectionSuffix}.`)
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
