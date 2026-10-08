#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SCRIPT_PATH = fileURLToPath(import.meta.url)
const DEFAULT_ROOT = path.resolve(path.dirname(SCRIPT_PATH), '..')

const FORBIDDEN_PREFIXES = [
  'docs/',
  '.agents/',
  '.claude/',
  '.codex/',
  '.harness/',
  '.impeccable/',
  '.mimocode/',
  '.opencode/',
  '.superpowers/',
  '.playwright-mcp/',
  'backend/.claude/',
  'backend/.harness/',
  'frontend/.claude/',
  'frontend/.harness/',
]

const FORBIDDEN_EXACT = new Set([
  '.mcp.json',
  '.npmrc',
  '.pypirc',
  '.netrc',
  'skills-lock.json',
  'CLAUDE.md',
  'backend/CLAUDE.md',
  'frontend/CLAUDE.md',
  'certs/localhost.key',
])

const FORBIDDEN_KEY_EXTENSIONS = /\.(?:key|p12|pfx|jks|keystore)$/i
const FORBIDDEN_ENV_PATH = /(^|\/)\.env(?:\.|$)/i
const FORBIDDEN_PRIVATE_KEY_NAMES = /(^|\/)(?:id_rsa|id_ed25519|id_ecdsa)(?:\.|$)/i
const LEGACY_PROJECTION_PREFIX = 'policy/qualification/legacy-evidence/docs/'
const LEGACY_PROJECTION_MARKER = 'TALOS_MACHINE_QUALIFICATION_PROJECTION v1'
const LEGACY_PROJECTION_MAX_BYTES = 32 * 1024

const CONTENT_PATTERNS = [
  ['-----BEGIN ', '(RSA |EC |DSA |OPENSSH |ENCRYPTED )?', 'PRIVATE KEY-----'].join(''),
  ['ghp', '_[A-Za-z0-9]{30,}'].join(''),
  ['github_pat', '_[A-Za-z0-9_]{20,}'].join(''),
  ['AK', 'IA[0-9A-Z]{16}'].join(''),
  ['xox', '[abprs]-[A-Za-z0-9-]{10,}'].join(''),
]

const GREP_EXCLUDES = [
  ':(exclude)scripts/check-public-surface.mjs',
  ':(exclude)scripts/check-public-surface.test.mjs',
]

function git(root, args, options = {}) {
  const result = spawnSync('git', args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  })
  if (result.error) throw result.error
  return result
}

function exactCommit(root, ref) {
  const result = git(root, ['rev-parse', '--verify', `${ref}^{commit}`])
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `unable to resolve Git ref: ${ref}`)
  }
  return result.stdout.trim()
}

export function trackedFilesAt(root = DEFAULT_ROOT, ref = 'HEAD') {
  const commit = exactCommit(root, ref)
  const result = git(root, ['ls-tree', '-r', '-z', '--name-only', commit])
  if (result.status !== 0) throw new Error(result.stderr || 'git ls-tree failed')
  return result.stdout
    .split('\0')
    .filter(Boolean)
}

function readTrackedFileAt(root, commit, file) {
  const result = git(root, ['show', `${commit}:${file}`])
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `unable to read tracked file at ${commit}: ${file}`)
  }
  return result.stdout
}

function scanContentPattern(root, commit, pattern) {
  const result = git(root, [
    'grep',
    '-I',
    '-n',
    '-E',
    '-e',
    pattern,
    commit,
    '--',
    '.',
    ...GREP_EXCLUDES,
  ])
  if (result.status === 1) return []
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `git grep failed for pattern: ${pattern}`)
  }
  return result.stdout
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(Boolean)
}

export function collectPublicSurfaceFailures({
  root = DEFAULT_ROOT,
  ref = 'HEAD',
} = {}) {
  const commit = exactCommit(root, ref)
  const files = trackedFilesAt(root, commit)
  const failures = []

  for (const file of files) {
    if (FORBIDDEN_EXACT.has(file)) {
      failures.push(`${file}: forbidden public-mirror path`)
    }
    if (FORBIDDEN_KEY_EXTENSIONS.test(file)) {
      failures.push(`${file}: key/keystore material must not be tracked in a public qualification snapshot`)
    }
    if (FORBIDDEN_ENV_PATH.test(file)) {
      failures.push(`${file}: environment-secret file must not be tracked in a public qualification snapshot`)
    }
    if (FORBIDDEN_PRIVATE_KEY_NAMES.test(file)) {
      failures.push(`${file}: private-key filename must not be tracked in a public qualification snapshot`)
    }
    if (file.startsWith(LEGACY_PROJECTION_PREFIX)) {
      const source = readTrackedFileAt(root, commit, file)
      if (!source.includes(LEGACY_PROJECTION_MARKER)) {
        failures.push(`${file}: full legacy narrative is forbidden; only marked machine qualification projections may enter the public mirror`)
      }
      if (Buffer.byteLength(source, 'utf8') > LEGACY_PROJECTION_MAX_BYTES) {
        failures.push(`${file}: machine qualification projection exceeds ${LEGACY_PROJECTION_MAX_BYTES} bytes`)
      }
    }
    for (const prefix of FORBIDDEN_PREFIXES) {
      if (file.startsWith(prefix)) {
        failures.push(`${file}: internal knowledge/retired tooling namespace is forbidden in a public qualification snapshot`)
        break
      }
    }
  }

  for (const pattern of CONTENT_PATTERNS) {
    for (const match of scanContentPattern(root, commit, pattern)) {
      failures.push(`${match}: secret/private-key signature detected in tracked content`)
    }
  }

  return [...new Set(failures)].sort()
}

export function runPublicSurfaceCheck(options = {}) {
  const failures = collectPublicSurfaceFailures(options)
  if (failures.length > 0) {
    console.error('TALOS public qualification surface FAILED')
    for (const failure of failures) console.error(`- ${failure}`)
    return false
  }
  console.log('TALOS public qualification surface PASS')
  return true
}

function parseArgs(argv) {
  let ref = 'HEAD'
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index]
    if (token === '--ref') {
      ref = argv[index + 1]
      if (!ref) throw new Error('--ref requires a Git ref')
      index += 1
      continue
    }
    throw new Error(`unknown argument: ${token}`)
  }
  return { ref }
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) {
  try {
    const { ref } = parseArgs(process.argv.slice(2))
    if (!runPublicSurfaceCheck({ ref })) process.exitCode = 1
  } catch (error) {
    console.error(`TALOS public qualification surface FAILED: ${error.message}`)
    process.exitCode = 1
  }
}
