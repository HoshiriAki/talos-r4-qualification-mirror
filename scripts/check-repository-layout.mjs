#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import process from 'node:process'

const result = spawnSync('git', ['ls-files'], { encoding: 'utf8' })
if (result.status !== 0) {
  console.error(result.stderr || 'git ls-files failed')
  process.exit(result.status || 1)
}

const trackedFiles = result.stdout
  .split(/\r?\n/)
  .map((entry) => entry.trim())
  .filter(Boolean)

const rootFiles = trackedFiles.filter((entry) => !entry.includes('/'))

const allowedRootMarkdown = new Set([
  'README.md',
  'AGENTS.md',
  'DESIGN.md',
  'PRODUCT.md',
])

const allowedRootYaml = new Set([
  'docker-compose.yml',
  'pnpm-lock.yaml',
  'pnpm-workspace.yaml',
])

const forbiddenExact = new Set([
  'package-lock.json',
  'check-startup.cmd',
  'migrate-json-to-sqlite.js',
  'test-multi-tenant.js',
  'response.body',
  'users.json',
  'devices.json',
  'rental.db.backup',
  'eslint.config.js',
  'eslint.config.mjs',
])

const forbiddenTrackedPrefixes = [
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

const forbiddenTrackedExact = new Set([
  '.mcp.json',
  'skills-lock.json',
  'CLAUDE.md',
  'backend/CLAUDE.md',
  'frontend/CLAUDE.md',
  'frontend/package-lock.json',
  'scripts/check-doc-links.mjs',
  'scripts/feature-status.mjs',
])

const failures = []

for (const file of trackedFiles) {
  if (forbiddenTrackedExact.has(file)) {
    failures.push(`${file}: retired Agent/tooling artifact must remain untracked`)
  }
  for (const prefix of forbiddenTrackedPrefixes) {
    if (file.startsWith(prefix)) {
      failures.push(`${file}: retired Agent/tooling namespace must not contain tracked files`)
      break
    }
  }
}

function workflowUsesGenericSelfHostedRunner(source) {
  const lines = source.replace(/\r\n?/g, '\n').split('\n')

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    const inline = line.match(/^(\s*)runs-on:\s*(.+?)\s*$/i)
    if (inline) {
      if (/\bself-hosted\b/i.test(inline[2])) return true
      continue
    }

    const block = line.match(/^(\s*)runs-on:\s*$/i)
    if (!block) continue

    const indent = block[1].length
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const candidate = lines[cursor]
      if (candidate.trim() === '' || candidate.trimStart().startsWith('#')) continue

      const candidateIndent = candidate.match(/^\s*/)?.[0].length ?? 0
      if (candidateIndent <= indent) break
      if (/^\s*-\s*self-hosted\s*(?:#.*)?$/i.test(candidate)) return true
    }
  }

  return false
}

for (const file of rootFiles) {
  const lower = file.toLowerCase()

  if (forbiddenExact.has(file)) failures.push(`${file}: forbidden legacy/generated root file`)
  if (file.endsWith('.md') && !allowedRootMarkdown.has(file)) failures.push(`${file}: root Markdown is not an approved repository-root document`)
  if (/\.html?$/i.test(file)) failures.push(`${file}: HTML artifacts must remain untracked or live under an approved product asset path`)
  if (/\.(?:body|txt)$/i.test(file)) failures.push(`${file}: response/log text must remain untracked`)
  if (/\.ya?ml$/i.test(file) && !allowedRootYaml.has(file)) failures.push(`${file}: root YAML is not an approved tool entry`)
  if (/^(?:login|after-login|dashboard).*\.ya?ml$/i.test(file)) failures.push(`${file}: browser snapshot must remain untracked`)
  if (/^estimate\d*\.json$/i.test(file)) failures.push(`${file}: generated estimate data must remain untracked`)
  if (/^p3.*\.txt$/i.test(file)) failures.push(`${file}: local test state must remain untracked`)
  if (/^__.*\.(?:js|py)$/i.test(file)) failures.push(`${file}: one-off helper must live under scripts/ or remain local`)
  if (/\.cmd$/i.test(file) && lower !== 'start.cmd') failures.push(`${file}: only start.cmd is allowed at repository root`)
  if (/\.ps1$/i.test(file) && lower !== 'start.ps1') failures.push(`${file}: only start.ps1 is allowed at repository root`)
}

for (const workflow of trackedFiles.filter((file) => /^\.github\/workflows\/[^/]+\.ya?ml$/i.test(file))) {
  const source = readFileSync(workflow, 'utf8')
  if (workflowUsesGenericSelfHostedRunner(source)) {
    failures.push(`${workflow}: generic self-hosted runner routing is forbidden; use bounded custom runner labels`)
  }
}

if (failures.length > 0) {
  console.error('Repository layout validation failed:')
  for (const failure of [...new Set(failures)]) console.error(`- ${failure}`)
  process.exit(1)
}

console.log(`Repository root layout valid (${rootFiles.length} tracked root files).`)
