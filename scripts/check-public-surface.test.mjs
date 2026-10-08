#!/usr/bin/env node

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { collectPublicSurfaceFailures } from './check-public-surface.mjs'

const root = mkdtempSync(path.join(os.tmpdir(), 'talos-public-surface-'))

function git(...args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim()
}

function write(relative, content) {
  const absolute = path.join(root, relative)
  mkdirSync(path.dirname(absolute), { recursive: true })
  writeFileSync(absolute, content, 'utf8')
}

function commit(message) {
  git('add', '-A')
  git('commit', '-m', message)
  return git('rev-parse', 'HEAD')
}

try {
  git('init')
  git('config', 'user.name', 'TALOS Test')
  git('config', 'user.email', 'test@talos.invalid')

  write('src/main.txt', 'safe qualification source\n')
  write('.github/workflows/qualification.yml', 'name: qualification\n')
  const safe = commit('safe')
  assert.deepEqual(collectPublicSurfaceFailures({ root, ref: safe }), [])

  write('certs/dev.key', 'test fixture\n')
  const keyPath = commit('tracked key')
  assert.match(
    collectPublicSurfaceFailures({ root, ref: keyPath }).join('\n'),
    /key\/keystore material/,
  )

  git('rm', 'certs/dev.key')
  write('config/credential.txt', ['-----BEGIN ', 'PRIVATE KEY-----\n', 'fixture\n'].join(''))
  const keyContent = commit('embedded private key')
  assert.match(
    collectPublicSurfaceFailures({ root, ref: keyContent }).join('\n'),
    /secret\/private-key signature/,
  )

  git('rm', 'config/credential.txt')
  write('docs/internal-plan.md', 'internal design\n')
  const internalDocs = commit('internal docs')
  assert.match(
    collectPublicSurfaceFailures({ root, ref: internalDocs }).join('\n'),
    /internal knowledge\/retired tooling namespace/,
  )

  git('rm', 'docs/internal-plan.md')
  write('.env.production', 'PASSWORD=fixture\n')
  const envFile = commit('env')
  assert.match(
    collectPublicSurfaceFailures({ root, ref: envFile }).join('\n'),
    /environment-secret file/,
  )

  git('rm', '.env.production')
  write(
    'policy/qualification/legacy-evidence/docs/projection.md',
    '<!-- TALOS_MACHINE_QUALIFICATION_PROJECTION v1 -->\nTOKEN_A\n',
  )
  const projection = commit('marked legacy projection')
  assert.deepEqual(collectPublicSurfaceFailures({ root, ref: projection }), [])

  write(
    'policy/qualification/legacy-evidence/docs/projection.md',
    '# Full historical narrative without projection marker\n',
  )
  const fullLegacy = commit('unmarked legacy narrative')
  assert.match(
    collectPublicSurfaceFailures({ root, ref: fullLegacy }).join('\n'),
    /only marked machine qualification projections/,
  )

  console.log('TALOS public qualification surface tests PASS')
} finally {
  rmSync(root, { recursive: true, force: true })
}
