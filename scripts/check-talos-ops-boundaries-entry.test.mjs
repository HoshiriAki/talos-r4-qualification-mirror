#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  BASELINE_LOCAL_COMMIT_ENV,
  BASELINE_SHA_ENV,
  resolveBaselineProjection,
} from './check-talos-ops-boundaries-entry.mjs'
import { BASELINE_COMMIT } from './check-talos-ops-boundaries.mjs'

const projectedCommit = '0123456789abcdef0123456789abcdef01234567'

assert.deepEqual(
  resolveBaselineProjection({ sourceSha: undefined, localCommit: undefined }),
  {
    authorityCommit: BASELINE_COMMIT,
    comparisonCommit: BASELINE_COMMIT,
    mode: 'git-object',
  },
  'normal repositories must retain the canonical Git-object baseline path',
)

assert.deepEqual(
  resolveBaselineProjection({ sourceSha: BASELINE_COMMIT, localCommit: BASELINE_COMMIT }),
  {
    authorityCommit: BASELINE_COMMIT,
    comparisonCommit: BASELINE_COMMIT,
    mode: 'git-object',
  },
  'an already available canonical baseline object must not be mislabeled as an archive projection',
)

assert.deepEqual(
  resolveBaselineProjection({ sourceSha: BASELINE_COMMIT, localCommit: projectedCommit }),
  {
    authorityCommit: BASELINE_COMMIT,
    comparisonCommit: projectedCommit,
    mode: 'exact-archive-projection',
  },
  'an exact baseline archive may be projected through a local commit while retaining canonical authority identity',
)

assert.throws(
  () => resolveBaselineProjection({ sourceSha: BASELINE_COMMIT, localCommit: undefined }),
  new RegExp(`${BASELINE_SHA_ENV} and ${BASELINE_LOCAL_COMMIT_ENV} must be provided together`),
  'a baseline source SHA without its local projection must fail closed',
)

assert.throws(
  () => resolveBaselineProjection({ sourceSha: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', localCommit: projectedCommit }),
  new RegExp(`${BASELINE_SHA_ENV} must equal fingerprint baseline`),
  'a projection claiming a different authority SHA must fail closed',
)

assert.throws(
  () => resolveBaselineProjection({ sourceSha: BASELINE_COMMIT, localCommit: 'deadbeef' }),
  new RegExp(`${BASELINE_LOCAL_COMMIT_ENV} must be a full 40-character Git object id`),
  'an abbreviated or malformed local projection id must fail closed',
)

console.log('TALOS Operations baseline projection self-test passed: 6 cases.')
