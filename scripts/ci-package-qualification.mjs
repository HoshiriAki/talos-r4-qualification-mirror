#!/usr/bin/env node

import { readFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const scriptPath = fileURLToPath(import.meta.url)
const repositoryRoot = path.resolve(path.dirname(scriptPath), '..')
const registryPath = path.join(repositoryRoot, '.talos/ci/package-qualifications.json')

function fail(message) {
  throw new Error(message)
}

const CHECK_SCRIPT = /^scripts\/check-r4-p\d+-sp\d+-[a-z0-9-]+(?:\.test)?\.mjs$/
const QUALIFICATION_SCRIPT = /^scripts\/r4-p\d+-sp\d+-[a-z0-9-]+\.sh$/

export function validateRegistry(registry) {
  if (registry?.version !== 2 || !Array.isArray(registry.entries)) {
    fail('package qualification registry must be version 2 with entries[]')
  }

  const prefixes = new Set()
  for (const entry of registry.entries) {
    if (!/^agent\/r4-p\d+-sp\d+-/.test(entry.branch_prefix ?? '')) {
      fail('invalid branch_prefix: ' + JSON.stringify(entry.branch_prefix))
    }
    if (!Array.isArray(entry.check_scripts) || entry.check_scripts.length === 0) {
      fail('check_scripts must be a non-empty array for ' + entry.branch_prefix)
    }
    for (const checkScript of entry.check_scripts) {
      if (!CHECK_SCRIPT.test(checkScript ?? '')) {
        fail('invalid package check script: ' + JSON.stringify(checkScript))
      }
    }
    if (!QUALIFICATION_SCRIPT.test(entry.qualification_script ?? '')) {
      fail('invalid qualification script: ' + JSON.stringify(entry.qualification_script))
    }
    if (!/^[a-z0-9-]+$/.test(entry.label ?? '')) {
      fail('invalid qualification label: ' + JSON.stringify(entry.label))
    }
    if (prefixes.has(entry.branch_prefix)) fail('duplicate branch_prefix: ' + entry.branch_prefix)
    prefixes.add(entry.branch_prefix)
  }
  return registry
}

export function resolveQualification(branch, registry) {
  const isR4Sp = /^agent\/r4-p\d+-sp\d+-/.test(branch)
  const matches = registry.entries.filter(entry => branch.startsWith(entry.branch_prefix))

  if (!isR4Sp) {
    return {
      required: false,
      check_scripts: [],
      qualification_script: '',
      label: 'none',
    }
  }
  if (matches.length !== 1) {
    fail(
      'R4 SP branch must resolve to exactly one package qualification entry: ' +
        branch +
        ' (matches=' +
        matches.length +
        ')',
    )
  }

  return {
    required: true,
    check_scripts: [...matches[0].check_scripts],
    qualification_script: matches[0].qualification_script,
    label: matches[0].label,
  }
}

export async function loadRegistry() {
  return validateRegistry(JSON.parse(await readFile(registryPath, 'utf8')))
}

async function main() {
  const args = process.argv.slice(2)
  const registry = await loadRegistry()
  if (args.includes('--validate')) {
    console.log('CI package qualification registry valid: ' + registry.entries.length + ' entries')
    return
  }

  const branchIndex = args.indexOf('--branch')
  if (branchIndex < 0 || !args[branchIndex + 1]) fail('missing --branch <ref>')
  const branch = args[branchIndex + 1]
  const resolved = resolveQualification(branch, registry)

  const outputIndex = args.indexOf('--github-output')
  const outputPath = outputIndex >= 0 ? args[outputIndex + 1] : null
  const lines = ['required=' + String(resolved.required), 'label=' + resolved.label]

  if (outputPath) {
    const { appendFile } = await import('node:fs/promises')
    await appendFile(outputPath, lines.join('\n') + '\n', 'utf8')
  } else {
    console.log(lines.join('\n'))
  }
}

if (path.resolve(process.argv[1] ?? '') === scriptPath) {
  main().catch(error => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  })
}
