#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import process from 'node:process'

const suites = [
  'scripts/check-r4-plugin-verification-boundary.test.mjs',
  'scripts/check-r4-plugin-lifecycle-boundary.test.mjs',
  'scripts/check-r4-plugin-background-execution-boundary.test.mjs',
  'scripts/check-r4-plugin-upgrade-boundary.test.mjs',
  'scripts/check-r4-plugin-provider-binding-boundary.test.mjs',
  'scripts/check-r4-plugin-runtime-composition-boundary.test.mjs',
  'scripts/check-r4-plugin-secret-egress-boundary.test.mjs',
  'scripts/check-r4-plugin-layering-boundary.test.mjs',
  'scripts/check-r4-plugin-host-security.test.mjs',
]

for (const script of suites) {
  const result = spawnSync(process.execPath, [script], {
    cwd: process.cwd(),
    stdio: 'inherit',
  })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}

console.log('R4-P6 unified plugin mutation gate passed.')
