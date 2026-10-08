#!/usr/bin/env node

import assert from 'node:assert/strict'
import { resolveQualification, validateRegistry } from './ci-package-qualification.mjs'

const registry = validateRegistry({
  version: 2,
  entries: [
    {
      branch_prefix: 'agent/r4-p10-sp01-',
      check_scripts: [
        'scripts/check-r4-p10-sp01-backup-artifact-contract.test.mjs',
        'scripts/check-r4-p10-sp01-backup-artifact-contract.mjs',
      ],
      qualification_script: 'scripts/r4-p10-sp01-backup-artifact-contract.sh',
      label: 'p10-sp01-backup-artifact',
    },
    {
      branch_prefix: 'agent/r4-p11-sp02-',
      check_scripts: ['scripts/check-r4-p11-sp02-example.mjs'],
      qualification_script: 'scripts/r4-p11-sp02-example.sh',
      label: 'p11-sp02-example',
    },
  ],
})

assert.deepEqual(resolveQualification('agent/ci-tiered-qualification-v2', registry), {
  required: false,
  check_scripts: [],
  qualification_script: '',
  label: 'none',
})

assert.deepEqual(resolveQualification('agent/r4-p10-sp01-backup-artifact-contract', registry), {
  required: true,
  check_scripts: [
    'scripts/check-r4-p10-sp01-backup-artifact-contract.test.mjs',
    'scripts/check-r4-p10-sp01-backup-artifact-contract.mjs',
  ],
  qualification_script: 'scripts/r4-p10-sp01-backup-artifact-contract.sh',
  label: 'p10-sp01-backup-artifact',
})

assert.throws(
  () => resolveQualification('agent/r4-p10-sp02-restore-rehearsal', registry),
  /exactly one package qualification entry/,
)

assert.throws(
  () =>
    validateRegistry({
      version: 2,
      entries: [
        {
          branch_prefix: 'agent/r4-p10-sp01-',
          check_scripts: ['../unsafe.mjs'],
          qualification_script: 'scripts/r4-p10-sp01-backup-artifact-contract.sh',
          label: 'unsafe',
        },
      ],
    }),
  /invalid package check script/,
)

assert.throws(
  () =>
    validateRegistry({
      version: 2,
      entries: [
        {
          branch_prefix: 'agent/r4-p10-sp01-',
          check_scripts: ['scripts/check-r4-p10-sp01-backup-artifact-contract.mjs'],
          qualification_script: '../unsafe.sh',
          label: 'unsafe',
        },
      ],
    }),
  /invalid qualification script/,
)

console.log('CI package qualification resolver fixtures passed')
