#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const PATHS = Object.freeze({
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  providerConfig: 'backend/src/registry/provider_config.rs',
  validation: 'backend/src/registry/validation.rs',
  assembler: 'backend/src/registry/assembler.rs',
  services: 'backend/src/application/services.rs',
  workers: 'backend/src/application/workers.rs',
  state: 'backend/src/state.rs',
  main: 'backend/src/main.rs',
})

const REQUIRED_PATHS = Object.values(PATHS)
const REGISTRY_PREFIX = 'backend/src/registry/'
const APPLICATION_PREFIX = 'backend/src/application/'
function failure(path, evidence) {
  return { rule: 'TALOS-OPS-017', path, evidence, occurrences: 1 }
}

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

function isRegistryRust(path) {
  return path.startsWith(REGISTRY_PREFIX) && path.endsWith('.rs')
}

function isApplicationRust(path) {
  return path.startsWith(APPLICATION_PREFIX) && path.endsWith('.rs')
}

function maskRustStringsAndComments(source) {
  const chars = [...source]
  const masked = [...source]
  const blank = (index) => {
    if (masked[index] !== '\n') masked[index] = ' '
  }

  for (let index = 0; index < chars.length;) {
    if (chars[index] === '/' && chars[index + 1] === '/') {
      while (index < chars.length && chars[index] !== '\n') blank(index++)
      continue
    }
    if (chars[index] === '/' && chars[index + 1] === '*') {
      let depth = 0
      while (index < chars.length) {
        if (chars[index] === '/' && chars[index + 1] === '*') {
          blank(index++)
          blank(index++)
          depth += 1
        } else if (chars[index] === '*' && chars[index + 1] === '/') {
          blank(index++)
          blank(index++)
          depth -= 1
          if (depth === 0) break
        } else {
          blank(index++)
        }
      }
      continue
    }

    const rawMatch = source.slice(index).match(/^r(#+)?"/)
    if (rawMatch) {
      const hashes = rawMatch[1] ?? ''
      const terminator = `"${hashes}`
      const end = source.indexOf(terminator, index + rawMatch[0].length)
      const stop = end === -1 ? chars.length : end + terminator.length
      while (index < stop) blank(index++)
      continue
    }

    if (chars[index] === '"') {
      blank(index++)
      while (index < chars.length) {
        const escaped = chars[index] === '\\'
        const closing = chars[index] === '"'
        blank(index++)
        if (escaped && index < chars.length) blank(index++)
        else if (closing) break
      }
      continue
    }

    if (chars[index] === "'" && !/^'[A-Za-z_][A-Za-z0-9_]*/.test(source.slice(index))) {
      blank(index++)
      while (index < chars.length) {
        const escaped = chars[index] === '\\'
        const closing = chars[index] === "'"
        blank(index++)
        if (escaped && index < chars.length) blank(index++)
        else if (closing) break
      }
      continue
    }
    index += 1
  }
  return masked.join('')
}

function cfgTestItemEnd(masked, attributeEnd) {
  let index = attributeEnd
  let parentheses = 0
  let brackets = 0
  while (index < masked.length) {
    const char = masked[index]
    if (char === '(') parentheses += 1
    else if (char === ')') parentheses -= 1
    else if (char === '[') brackets += 1
    else if (char === ']') brackets -= 1
    else if (char === ';' && parentheses === 0 && brackets === 0) return index + 1
    else if (char === '{' && parentheses === 0 && brackets === 0) {
      let depth = 1
      index += 1
      while (index < masked.length && depth > 0) {
        if (masked[index] === '{') depth += 1
        else if (masked[index] === '}') depth -= 1
        index += 1
      }
      while (/\s/.test(masked[index] ?? '')) index += 1
      return masked[index] === ';' ? index + 1 : index
    }
    index += 1
  }
  return masked.length
}

function runtimeSource(source) {
  const normalized = source.replaceAll('\r\n', '\n').replaceAll('\r', '\n')
  const masked = maskRustStringsAndComments(normalized)
  const cfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g
  const ranges = []
  let match
  while ((match = cfgTest.exec(masked)) !== null) {
    const end = cfgTestItemEnd(masked, cfgTest.lastIndex)
    ranges.push([match.index, end])
    cfgTest.lastIndex = Math.max(end, cfgTest.lastIndex)
  }
  if (ranges.length === 0) return normalized

  const runtime = [...normalized]
  for (const [start, end] of ranges) {
    for (let index = start; index < end; index += 1) {
      if (runtime[index] !== '\n') runtime[index] = ' '
    }
  }
  return runtime.join('')
}

function structFields(source, name) {
  const body = source.match(new RegExp(`(?:pub\\s+)?struct\\s+${name}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1]
  if (body === undefined) return null
  return body.split('\n').flatMap((rawLine) => {
    const line = rawLine.trim()
    if (!line || line.startsWith('#[') || line.startsWith('//')) return []
    const match = line.match(/^(pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+?)(?:,\s*)?$/)
    return match ? [{ visibility: (match[1] ?? '').trim(), name: match[2], type: match[3].trim() }] : []
  })
}

function exactFields(fields, expected, visibility = '') {
  if (fields === null || fields.length !== expected.length) return false
  return expected.every(([name, type]) => {
    const field = fields.find((candidate) => candidate.name === name)
    return field?.visibility === visibility && field.type.replaceAll(/\s+/g, '') === type
  })
}

function hasGenericLocator(source) {
  return /HashMap\s*<\s*String\b[\s\S]{0,80}\bdyn\s+Any\b|\bdyn\s+Any\b|\bdowncast(?:_ref|_mut)?\s*\(|\bget_service\s*\(\s*["']/.test(source)
}

function hasProviderEnvironmentRead(source) {
  return /\b(?:std\s*::\s*env|env)\s*::\s*var(?:_os)?\s*\(\s*["'](?:SF_|WECHAT_|ALIPAY_|MINIAPP_)/i.test(source)
}

function hasConcreteFeatureImport(source) {
  return /\buse\s+[^;]*\bFeature[A-Z][A-Za-z0-9_]*\b[^;]*;/.test(source)
}

function hasPoolMutation(source) {
  return /\.\s*pool\s*\.\s*lock\s*\(|\.\s*pool\s*=|\*\s*[A-Za-z_][A-Za-z0-9_]*guard\s*=\s*Some\s*\(\s*pool\b/.test(source)
}

function hasDependencyMutation(source) {
  return /\b(?:module|feature|order|pricing|warehouse|repair|[a-z_][A-Za-z0-9_]*_module)\s*\.\s*[a-z_][A-Za-z0-9_]*\s*=/.test(source)
}

export function checkSp07ModuleFactoryWorkerBoundary(files) {
  const normalized = Object.fromEntries(
    Object.entries(files).map(([path, source]) => [normalizePath(path), runtimeSource(source)]),
  )
  const failures = []

  for (const path of REQUIRED_PATHS) {
    if (!Object.hasOwn(normalized, path)) failures.push(failure(path, 'required SP-07 boundary source is missing'))
  }
  if (failures.length > 0) return failures

  const descriptors = normalized[PATHS.descriptors]
  const factory = normalized[PATHS.factory]
  const providerConfig = normalized[PATHS.providerConfig]
  const validation = normalized[PATHS.validation]
  const assembler = normalized[PATHS.assembler]
  const services = normalized[PATHS.services]
  const workers = normalized[PATHS.workers]
  const state = normalized[PATHS.state]
  const main = normalized[PATHS.main]
  const registryRustPaths = Object.keys(normalized).filter(isRegistryRust)
  const applicationRustPaths = Object.keys(normalized).filter(isApplicationRust)
  const boundaryRustPaths = [...new Set([...registryRustPaths, ...applicationRustPaths, PATHS.state, PATHS.main])]

  const descriptorFields = structFields(descriptors, 'ModuleDescriptor')
  const expectedDescriptorFields = [
    ['registry_key', "&'staticstr"],
    ['metadata_name', "&'staticstr"],
    ['schema_name', "&'staticstr"],
    ['class', 'ModuleClass'],
    ['activation', 'ModuleActivation'],
    ['requirements', "&'static[ModuleRequirement]"],
    ['factory', 'ModuleFactoryId'],
  ]
  if (!exactFields(descriptorFields, expectedDescriptorFields, 'pub')) {
    failures.push(failure(PATHS.descriptors, 'ModuleDescriptor must use explicit registry_key, metadata_name, schema_name, and typed factory identity'))
  }
  if (/\bname\s*:\s*&'static\s+str/.test(descriptors)) {
    failures.push(failure(PATHS.descriptors, 'single ambiguous descriptor name is forbidden'))
  }

  const receiptFields = structFields(factory, 'ConstructionReceipt')
  if (receiptFields === null
      || !receiptFields.some((field) => field.name === 'factory' && /ModuleFactoryId/.test(field.type))
      || !receiptFields.some((field) => field.name === 'registry_key' && /&'static\s+str/.test(field.type))
      || !/trait\s+ConstructionIdentity[\s\S]*?const\s+FACTORY\s*:\s*ModuleFactoryId[\s\S]*?const\s+REGISTRY_KEY\s*:\s*&'static\s+str/m.test(factory)
      || !/factory\s*:\s*T\s*::\s*FACTORY/.test(factory)
      || !/registry_key\s*:\s*T\s*::\s*REGISTRY_KEY/.test(factory)
      || !/construction_receipts\s*\.\s*push\s*\(\s*constructed\s*\.\s*receipt\s*\)/.test(factory)
      || !/validate_descriptor_projection\s*\(/.test(factory)
      || !/&construction_receipts/.test(factory)) {
    failures.push(failure(PATHS.factory, 'factory selector must have concrete-type-bound construction receipts wired into projection validation'))
  }

  if (!/descriptor\s*\.\s*factory\s*!=\s*receipt\s*\.\s*factory/.test(validation)
      || !/receipt_keys\s*!=\s*active/.test(validation)) {
    failures.push(failure(PATHS.validation, 'factory selector must be proven by receipt-to-descriptor and active-set closure'))
  }

  const equalityChecks = [
    /module_keys\s*!=\s*active/,
    /build_order_keys\s*!=\s*active/,
    /receipt_keys\s*!=\s*active/,
    /metadata_name\s*!=\s*descriptor\s*\.\s*metadata_name/,
    /schema_name\s*!=\s*descriptor\s*\.\s*schema_name/,
  ]
  if (equalityChecks.some((pattern) => !pattern.test(validation))) {
    failures.push(failure(PATHS.validation, 'projection must prove registry, build, construction, metadata, and schema identity closure'))
  }
  for (const path of registryRustPaths) {
    const source = normalized[path]
    if (/descriptor\s*\.\s*registry_key\s*(?:==|!=)\s*descriptor\s*\.\s*(?:metadata_name|schema_name)|descriptor\s*\.\s*(?:metadata_name|schema_name)\s*(?:==|!=)\s*descriptor\s*\.\s*registry_key/.test(source)) {
      failures.push(failure(path, 'registry_key must not be forced equal to metadata_name or schema_name'))
    }
  }

  if (hasProviderEnvironmentRead(assembler)) {
    failures.push(failure(PATHS.assembler, 'assembler must not read deployment environment directly'))
  }
  if (/\b(?:[A-Z0-9]+_)*(?:CLIENT_SECRET|API_KEY|PRIVATE_KEY|ACCESS_TOKEN|PASSWORD)\b/i.test(assembler)) {
    failures.push(failure(PATHS.assembler, 'assembler must not know provider secret names'))
  }
  if (/\.\s*insert\s*\(/.test(assembler)) {
    failures.push(failure(PATHS.assembler, 'assembler must not perform per-module map insertion'))
  }
  if (!/ModuleFactory\s*::\s*new\s*\([\s\S]*?\)\s*\.\s*build\s*\(\s*\)/m.test(assembler)) {
    failures.push(failure(PATHS.assembler, 'assembler must delegate module construction to ModuleFactory'))
  }

  for (const path of boundaryRustPaths) {
    const source = normalized[path]
    if (hasProviderEnvironmentRead(source)) {
      failures.push(failure(path, 'provider environment reads must be confined to the Integration KeyStore boundary'))
    }
    if (isRegistryRust(path) && path !== PATHS.factory && hasConcreteFeatureImport(source)) {
      failures.push(failure(path, 'concrete Feature imports are allowed only inside ModuleFactory'))
    }
    if (path !== PATHS.factory && hasPoolMutation(source)) {
      failures.push(failure(path, 'direct pool lock or assignment is allowed only inside ModuleFactory'))
    }
    if (path !== PATHS.factory && hasDependencyMutation(source)) {
      failures.push(failure(path, 'direct concrete dependency-field mutation is allowed only inside ModuleFactory'))
    }
    if ((isRegistryRust(path) || isApplicationRust(path)) && hasGenericLocator(source)) {
      failures.push(failure(path, 'generic service-locator, Any, and downcast patterns are forbidden'))
    }
  }

  if (hasProviderEnvironmentRead(providerConfig)) {
    failures.push(failure(PATHS.providerConfig, 'legacy provider configuration must not read provider environment variables'))
  }
  if (!/impl\s+ProviderDeploymentConfig\s*\{[\s\S]*?pub\s+fn\s+is_configured\s*\([^)]*\)\s*->\s*bool\s*\{\s*false\s*\}/m.test(providerConfig)) {
    failures.push(failure(PATHS.providerConfig, 'legacy provider modules must remain disabled without Integration binding and KeyStore readiness'))
  }

  const applicationFields = structFields(services, 'ApplicationServices')
  if (!exactFields(applicationFields, [
    ['clock', 'Arc<dynClock>'],
    ['module_client', 'Arc<dynModuleClient>'],
    ['worker_runner', 'Arc<dynWorkerRunner>'],
    ['repository_provider', 'Arc<dynRepositoryProvider>'],
  ])) {
    failures.push(failure(PATHS.services, 'ApplicationServices must contain exactly four private Clock, ModuleClient, WorkerRunner, and RepositoryProvider handles'))
  }
  if (!/pub\s+fn\s+worker_runner\s*\(\s*&self\s*\)\s*->\s*Arc\s*<\s*dyn\s+WorkerRunner\s*>/.test(services)) {
    failures.push(failure(PATHS.services, 'ApplicationServices must expose the WorkerRunner through a cloned immutable handle'))
  }

  const stateFields = structFields(state, 'AppState')
  if (stateFields?.some((field) => field.name === 'worker_runner' || /WorkerRunner/.test(field.type)
      || field.name === 'repository_provider' || /RepositoryProvider|ScopedRepositories/.test(field.type))) {
    failures.push(failure(PATHS.state, 'AppState must not own a naked WorkerRunner or repository service'))
  }

  if (/\b(?:sync_order_statuses|seed_overdue_tasks|cleanup_expired_sessions)\s*\(/.test(main)) {
    failures.push(failure(PATHS.main, 'bootstrap must not call legacy maintenance functions directly'))
  }
  if (/tokio\s*::\s*time\s*::\s*interval\s*\(|\bloop\s*\{[\s\S]*?interval\s*\.\s*tick/m.test(main)) {
    failures.push(failure(PATHS.main, 'bootstrap must not own the periodic maintenance loop'))
  }
  if (!/worker_runner\s*\.\s*run_startup\s*\(\s*\)/.test(main)
      || !/worker_runner\s*\.\s*spawn_periodic\s*\(\s*\)/.test(main)) {
    failures.push(failure(PATHS.main, 'bootstrap must start maintenance only through WorkerRunner'))
  }

  if (!/pub\s+trait\s+WorkerRunner\s*:\s*Send\s*\+\s*Sync/.test(workers)
      || !/pub\s+struct\s+WorkerContextFactory\b/.test(workers)
      || !/ExecutionContext\s*::\s*new\s*\(/.test(workers)) {
    failures.push(failure(PATHS.workers, 'worker boundary must use a typed WorkerRunner and fresh ExecutionContext factory'))
  }

  return failures
}

function collectRustFiles(root, relativeDirectory, files) {
  const absoluteDirectory = join(root, relativeDirectory)
  if (!existsSync(absoluteDirectory)) return
  for (const entry of readdirSync(absoluteDirectory, { withFileTypes: true })) {
    const relativePath = `${relativeDirectory}/${entry.name}`
    if (entry.isDirectory()) collectRustFiles(root, relativePath, files)
    else if (entry.isFile() && entry.name.endsWith('.rs')) {
      files[normalizePath(relativePath)] = readFileSync(join(root, relativePath), 'utf8')
    }
  }
}

function loadRepositoryFiles(root) {
  const files = {}
  collectRustFiles(root, 'backend/src/registry', files)
  collectRustFiles(root, 'backend/src/application', files)
  for (const path of [PATHS.state, PATHS.main]) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const failures = checkSp07ModuleFactoryWorkerBoundary(loadRepositoryFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-07 module factory and worker boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-07 module factory and worker boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
