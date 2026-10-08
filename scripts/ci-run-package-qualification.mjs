#!/usr/bin/env node

import { access, mkdir } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { loadRegistry, resolveQualification } from './ci-package-qualification.mjs'

const scriptPath = fileURLToPath(import.meta.url)
const repositoryRoot = path.resolve(path.dirname(scriptPath), '..')

function fail(message) {
  throw new Error(message)
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    stdio: 'inherit',
    env: process.env,
  })
  if (result.error) throw result.error
  if (result.status !== 0) {
    fail(command + ' ' + args.join(' ') + ' exited with ' + String(result.status))
  }
}

async function main() {
  const args = process.argv.slice(2)
  const branchIndex = args.indexOf('--branch')
  if (branchIndex < 0 || !args[branchIndex + 1]) fail('missing --branch <ref>')

  const resolved = resolveQualification(args[branchIndex + 1], await loadRegistry())
  if (!resolved.required) {
    console.log('No current-package qualification is required for this branch.')
    return
  }

  for (const checkScript of resolved.check_scripts) {
    await access(path.join(repositoryRoot, checkScript))
    run(process.execPath, [checkScript])
  }

  await access(path.join(repositoryRoot, resolved.qualification_script))
  await mkdir(path.join(repositoryRoot, '.talos-evidence'), { recursive: true })
  const logPath = '.talos-evidence/' + resolved.label + '-workflow.log'
  run('bash', [
    '-lc',
    'set -euo pipefail; bash "$1" 2>&1 | tee "$2"',
    'talos-package-qualification',
    resolved.qualification_script,
    logPath,
  ])
}

if (path.resolve(process.argv[1] ?? '') === scriptPath) {
  main().catch(error => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  })
}
