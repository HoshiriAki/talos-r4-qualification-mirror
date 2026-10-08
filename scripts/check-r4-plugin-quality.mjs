#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import process from 'node:process'

const checks = [
  'scripts/check-r4-plugin-verification-boundary.mjs',
  'scripts/check-r4-plugin-lifecycle-boundary.mjs',
  'scripts/check-r4-plugin-upgrade-boundary.mjs',
  'scripts/check-r4-plugin-background-execution-boundary.mjs',
  'scripts/check-r4-plugin-provider-binding-boundary.mjs',
  'scripts/check-r4-plugin-runtime-composition-boundary.mjs',
  'scripts/check-r4-plugin-secret-egress-boundary.mjs',
  'scripts/check-r4-plugin-layering-boundary.mjs',
  'scripts/check-r4-plugin-host-security.mjs',
]

for (const script of checks) {
  const result = spawnSync(process.execPath, [script], {
    cwd: process.cwd(),
    stdio: 'inherit',
  })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}

console.log('R4-P6 unified plugin quality gate passed.')
