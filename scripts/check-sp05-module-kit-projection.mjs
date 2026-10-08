#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-OPS-015'
export const POLICY_MARKER = 'orchestration=EXPERIMENTAL_FEATURE; transport=KEEP_CORE_CANONICAL'

const CORE_ROOT = 'backend/system/core/src'
const CORE_CARGO = 'backend/system/core/Cargo.toml'
const CORE_LIB = 'backend/system/core/src/lib.rs'
const EXPERIMENTAL_MOD = 'backend/system/core/src/experimental/mod.rs'
const EXPERIMENTAL_ORCHESTRATION = 'backend/system/core/src/experimental/orchestration.rs'
const INIT = 'module-kit/INIT.md'
const MODULE_TEMPLATE = 'module-kit/MODULE_TEMPLATE.md'

const ORCHESTRATION_SYMBOLS = ['ModuleOp', 'SagaStep', 'OrchestrationError', 'CompensationLog']
const TRANSPORT_SYMBOLS = [
  'DataTransport',
  'TransportMetadata',
  'TransportPriority',
  'TransportOptions',
  'TransportMessage',
  'TransportHealth',
]

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

function finding(path, evidence) {
  return { rule: RULE, path: normalizePath(path), evidence, occurrences: 1 }
}

function publicDefinition(source, symbol) {
  return new RegExp(`\\bpub\\s+(?:struct|enum|trait)\\s+${symbol}\\b`).test(source)
}

function publicUseStatements(source) {
  return [...source.matchAll(/\bpub\s+use\b[\s\S]*?;/g)].map((match) => match[0])
}

function walk(directory) {
  if (!existsSync(directory)) return []
  const files = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) files.push(...walk(path))
    else if (entry.isFile() && entry.name.endsWith('.rs')) files.push(path)
  }
  return files
}

function readRepositoryFiles(root) {
  const paths = [CORE_CARGO, CORE_LIB, EXPERIMENTAL_MOD, EXPERIMENTAL_ORCHESTRATION, INIT, MODULE_TEMPLATE]
  const files = {}
  for (const path of paths) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  for (const absolute of walk(join(root, CORE_ROOT))) {
    files[normalizePath(relative(root, absolute))] = readFileSync(absolute, 'utf8')
  }
  return files
}

export function checkSp05Projection(files) {
  const normalized = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), source]),
  )
  const failures = []
  const cargo = normalized[CORE_CARGO] ?? ''
  const lib = normalized[CORE_LIB] ?? ''
  const experimentalMod = normalized[EXPERIMENTAL_MOD] ?? ''
  const orchestration = normalized[EXPERIMENTAL_ORCHESTRATION] ?? ''
  const init = normalized[INIT] ?? ''
  const template = normalized[MODULE_TEMPLATE] ?? ''

  if (!/^default\s*=\s*\[\s*\]\s*$/m.test(cargo)) {
    failures.push(finding(CORE_CARGO, 'default features must remain empty'))
  }
  if (!/^experimental-orchestration\s*=\s*\[\s*\]\s*$/m.test(cargo)) {
    failures.push(finding(CORE_CARGO, 'missing non-default experimental-orchestration feature'))
  }
  if (!/#\[cfg\(feature\s*=\s*"experimental-orchestration"\)\]\s*pub mod experimental;/m.test(lib)) {
    failures.push(finding(CORE_LIB, 'experimental namespace must be feature-gated at the crate root'))
  }

  for (const symbol of TRANSPORT_SYMBOLS) {
    if (!publicDefinition(lib, symbol)) {
      failures.push(finding(CORE_LIB, `canonical root ${symbol} definition is missing`))
    }
  }

  for (const symbol of ORCHESTRATION_SYMBOLS) {
    if (!publicDefinition(orchestration, symbol)) {
      failures.push(finding(EXPERIMENTAL_ORCHESTRATION, `experimental ${symbol} definition is missing`))
    }
  }
  if (!/pub\s+mod\s+orchestration\s*;/.test(experimentalMod)
      || !ORCHESTRATION_SYMBOLS.every((symbol) => experimentalMod.includes(symbol))) {
    failures.push(finding(EXPERIMENTAL_MOD, 'experimental orchestration exports are incomplete'))
  }

  for (const [path, source] of Object.entries(normalized)) {
    if (!path.startsWith(`${CORE_ROOT}/`) || !path.endsWith('.rs')) continue
    const isExperimental = path.startsWith(`${CORE_ROOT}/experimental/`)

    if (!isExperimental) {
      for (const symbol of ORCHESTRATION_SYMBOLS) {
        if (publicDefinition(source, symbol)) {
          failures.push(finding(path, `moved ${symbol} must not be defined outside experimental`))
        }
      }
      if (/\bpub\s+mod\s+orchestration\s*;/.test(source)) {
        failures.push(finding(path, 'default Core must not expose an orchestration module'))
      }
      for (const statement of publicUseStatements(source)) {
        if (ORCHESTRATION_SYMBOLS.some((symbol) => new RegExp(`\\b${symbol}\\b`).test(statement))) {
          failures.push(finding(path, `default Core re-export is forbidden: ${statement}`))
        }
      }
    } else if (path !== EXPERIMENTAL_ORCHESTRATION) {
      for (const symbol of TRANSPORT_SYMBOLS) {
        if (publicDefinition(source, symbol)) {
          failures.push(finding(path, `canonical transport ${symbol} must not be duplicated under experimental`))
        }
      }
    }
  }

  if (!init.includes(POLICY_MARKER)) {
    failures.push(finding(INIT, 'active projection is missing the SP-05 disposition marker'))
  }
  for (const required of [
    'backend/system/core',
    'backend/system/core-derive',
    'implementation_commit',
    'experimental-orchestration',
    'system_core::experimental',
    'DataTransport',
  ]) {
    if (!init.includes(required)) failures.push(finding(INIT, `bootstrap projection is missing ${required}`))
  }
  if (!/不得从 Markdown 重新手写|不得从本文件复制/.test(init)) {
    failures.push(finding(INIT, 'bootstrap projection must forbid hand-written Core reconstruction'))
  }
  for (const symbol of [...ORCHESTRATION_SYMBOLS, ...TRANSPORT_SYMBOLS]) {
    if (publicDefinition(init, symbol)) {
      failures.push(finding(INIT, `active bootstrap must not duplicate the ${symbol} definition`))
    }
  }

  if (!template.includes(POLICY_MARKER)) {
    failures.push(finding(MODULE_TEMPLATE, 'module template is missing the SP-05 disposition marker'))
  }
  if (!template.includes('experimental-orchestration') || !template.includes('system_core::experimental')) {
    failures.push(finding(MODULE_TEMPLATE, 'module template must use the explicit feature and experimental path'))
  }
  const legacyCandidateImport = new RegExp(
    `system_core::orchestration(?:::\\{[^}]*\\b(?:${ORCHESTRATION_SYMBOLS.join('|')})\\b|::(?:${ORCHESTRATION_SYMBOLS.join('|')})\\b)`,
  )
  if (/\bSaga::new\s*\(/.test(template) || legacyCandidateImport.test(template)) {
    failures.push(finding(MODULE_TEMPLATE, 'module template presents SP-05 orchestration candidates as a default operational runtime'))
  }
  for (const symbol of ORCHESTRATION_SYMBOLS) {
    if (publicDefinition(template, symbol)) {
      failures.push(finding(MODULE_TEMPLATE, `module template must not duplicate ${symbol}`))
    }
  }

  return failures
}

export function runRepositoryCheck(root = process.cwd()) {
  return checkSp05Projection(readRepositoryFiles(root))
}

function main() {
  const failures = runRepositoryCheck()
  if (failures.length > 0) {
    console.error(`${RULE} SP-05 projection check failed:`)
    for (const failure of failures) {
      console.error(`- ${failure.path}: ${failure.evidence}`)
    }
    process.exitCode = 1
    return
  }
  console.log(`${RULE} SP-05 module-kit/Core projection check passed.`)
}

const invoked = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invoked === import.meta.url) main()
