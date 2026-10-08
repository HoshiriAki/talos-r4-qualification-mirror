#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const REPOSITORY_PREFIX = 'backend/src/repositories/'
const APPLICATION_PREFIX = 'backend/src/application/'
const ROUTES_PREFIX = 'backend/src/routes/'
const SERVICES_PATH = 'backend/src/application/services.rs'
const STATE_PATH = 'backend/src/state.rs'
const MAIN_PATH = 'backend/src/main.rs'
const PROVIDER_CONTRACT_PATH = 'backend/src/repositories/contracts/provider.rs'
const SQLITE_MOD_PATH = 'backend/src/repositories/sqlite/mod.rs'
const SQLITE_SESSION_PATH = 'backend/src/repositories/sqlite/session.rs'
const CHANGED_PATH_DISCOVERY_FAILURE = 'GIT_CHANGED_PATH_DISCOVERY_FAILED:'

function failure(path, evidence) {
  return { rule: 'TALOS-OPS-018', path, evidence, occurrences: 1 }
}

function normalizePath(path) {
  return path.replaceAll('\\', '/')
}

function maskRustStringsAndComments(source) {
  const chars = [...source]
  const masked = [...source]
  const blank = (index) => { if (masked[index] !== '\n') masked[index] = ' ' }
  for (let index = 0; index < chars.length;) {
    if (chars[index] === '/' && chars[index + 1] === '/') {
      while (index < chars.length && chars[index] !== '\n') blank(index++)
      continue
    }
    if (chars[index] === '/' && chars[index + 1] === '*') {
      let depth = 0
      while (index < chars.length) {
        if (chars[index] === '/' && chars[index + 1] === '*') { blank(index++); blank(index++); depth += 1 }
        else if (chars[index] === '*' && chars[index + 1] === '/') {
          blank(index++); blank(index++); depth -= 1
          if (depth === 0) break
        } else blank(index++)
      }
      continue
    }
    const raw = source.slice(index).match(/^r(#+)?"/)
    if (raw) {
      const terminator = `"${raw[1] ?? ''}`
      const end = source.indexOf(terminator, index + raw[0].length)
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

export function runtimeSource(source) {
  const normalized = source.replaceAll('\r\n', '\n').replaceAll('\r', '\n')
  const masked = maskRustStringsAndComments(normalized)
  const ranges = []
  const pattern = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g
  let match
  while ((match = pattern.exec(masked)) !== null) {
    const end = cfgTestItemEnd(masked, pattern.lastIndex)
    ranges.push([match.index, end])
    pattern.lastIndex = Math.max(end, pattern.lastIndex)
  }
  const output = [...normalized]
  for (const [start, end] of ranges) {
    for (let index = start; index < end; index += 1) if (output[index] !== '\n') output[index] = ' '
  }
  return output.join('')
}

function structFields(source, name) {
  const body = source.match(new RegExp(`(?:pub\\s+)?struct\\s+${name}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1]
  if (body === undefined) return null
  return body.split('\n').flatMap((rawLine) => {
    const match = rawLine.trim().match(/^(pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+?)(?:,\s*)?$/)
    return match ? [{ visibility: (match[1] ?? '').trim(), name: match[2], type: match[3].trim().replaceAll(/\s+/g, '') }] : []
  })
}

function exactApplicationServices(fields) {
  const expected = new Map([
    ['clock', 'Arc<dynClock>'],
    ['module_client', 'Arc<dynModuleClient>'],
    ['worker_runner', 'Arc<dynWorkerRunner>'],
    ['repository_provider', 'Arc<dynRepositoryProvider>'],
  ])
  return fields?.length === expected.size
    && fields.every((field) => field.visibility === '' && expected.get(field.name) === field.type)
}

function normalizedVisibility(visibility) {
  return visibility.replaceAll(/\s+/g, '')
}

function isRepositoryInternalVisibility(visibility) {
  return normalizedVisibility(visibility) === 'pub(incrate::repositories)'
}

function containsRawStorageHandle(type) {
  return /\b(?:Pool|Connection|Transaction)\b/.test(type)
}

function checkRawStorageVisibility(repositoryEntries, failures) {
  for (const [path, source] of repositoryEntries) {
    const fieldPattern = /(pub(?:\s*\([^)]*\))?\s+)([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([^,}\n]+)/g
    let field
    while ((field = fieldPattern.exec(source)) !== null) {
      const visibility = field[1].trim()
      const type = field[3]
      if (containsRawStorageHandle(type) && !isRepositoryInternalVisibility(visibility)) {
        failures.push(failure(path, 'repository storage handles must be private or restricted to crate::repositories'))
      }
    }

    const getterPattern = /^\s*(pub(?:\s*\([^)]*\))?)\s+fn\s+[A-Za-z_][A-Za-z0-9_]*(?:\s*<[^>{}]*>)?\s*\([^)]*\)\s*->\s*([^{;\n]+)/gm
    let getter
    while ((getter = getterPattern.exec(source)) !== null) {
      const visibility = getter[1].trim()
      const returnType = getter[2]
      if (containsRawStorageHandle(returnType) && !isRepositoryInternalVisibility(visibility)) {
        failures.push(failure(path, 'repositories must not expose Pool, Connection, or Transaction getters outside crate::repositories'))
      }
    }

    const executorPattern = /^\s*(pub(?:\s*\([^)]*\))?)\s+fn\s+(?:read|write|execute|query|raw_sql)\b([^{;]*)/gm
    let executor
    while ((executor = executorPattern.exec(source)) !== null) {
      const visibility = executor[1].trim()
      const signature = executor[2]
      if ((/[<]|Connection|Transaction|&\s*str/.test(signature))
          && !isRepositoryInternalVisibility(visibility)) {
        failures.push(failure(path, 'generic SQL executors must remain restricted to crate::repositories'))
      }
    }
  }
}

export function checkSp08ScopedRepositoryBoundary(files, changedPaths = []) {
  const runtimeFiles = Object.fromEntries(Object.entries(files).map(([path, source]) => [normalizePath(path), runtimeSource(source)]))
  const failures = []
  const repositoryEntries = Object.entries(runtimeFiles).filter(([path]) => path.startsWith(REPOSITORY_PREFIX) && path.endsWith('.rs'))
  const allRuntime = Object.entries(runtimeFiles).filter(([path]) => (
    path.startsWith(REPOSITORY_PREFIX) || path.startsWith(APPLICATION_PREFIX) || path.startsWith(ROUTES_PREFIX)
      || path === MAIN_PATH || path === STATE_PATH
  ))
  const repositoryText = repositoryEntries.map(([, source]) => source).join('\n')
  const services = runtimeFiles[SERVICES_PATH] ?? ''
  const state = runtimeFiles[STATE_PATH] ?? ''
  const providerContract = runtimeFiles[PROVIDER_CONTRACT_PATH] ?? ''
  const sqliteMod = runtimeFiles[SQLITE_MOD_PATH] ?? ''
  const sqliteSession = runtimeFiles[SQLITE_SESSION_PATH] ?? ''

  const providerTrait = repositoryText.match(/pub\s+trait\s+RepositoryProvider\s*:\s*Send\s*\+\s*Sync\s*\{([\s\S]*?)\n\}/)?.[1] ?? ''
  if (!/fn\s+bind\s*\(\s*&self\s*,\s*(?:ctx|context)\s*:\s*&ExecutionContext\s*\)\s*->\s*Result\s*<\s*ScopedRepositories\s*,\s*RepositoryError\s*>/.test(providerTrait)) {
    failures.push(failure(REPOSITORY_PREFIX, 'RepositoryProvider::bind must be object-safe and accept only &ExecutionContext'))
  }
  if (!/pub\s+fn\s+from_execution\s*\(\s*(?:ctx|context)\s*:\s*&ExecutionContext\s*\)\s*->\s*Result\s*<\s*Self\s*,\s*RepositoryError\s*>/.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'RepositoryBinding production construction must derive only from &ExecutionContext'))
  }
  if (/fn\s+bind\s*\([^)]*\b(?:TenantId|DataScope|TenantScope|String|&\s*str)\b/.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'RepositoryProvider::bind must not accept raw tenant or scope input'))
  }

  checkRawStorageVisibility(repositoryEntries, failures)

  if (!/pub\s+struct\s+ScopedRepositories\b/.test(repositoryText)
      || !/pub\s+fn\s+binding\s*\(\s*&self\s*\)\s*->\s*&RepositoryBinding/.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'ScopedRepositories must expose its immutable RepositoryBinding'))
  }
  if (!/pub\s*\(\s*in\s+crate\s*::\s*repositories\s*\)\s+fn\s+sqlite\b/.test(providerContract)
      || !/pub\s*\(\s*in\s+crate\s*::\s*repositories\s*\)\s+fn\s+session\b/.test(providerContract)) {
    failures.push(failure(PROVIDER_CONTRACT_PATH, 'ScopedRepositories construction and session access must be restricted to crate::repositories'))
  }
  if (!/pub\s*\(\s*in\s+crate\s*::\s*repositories\s*\)\s+struct\s+SqliteRepositorySession\b/.test(sqliteSession)
      || !/pub\s*\(\s*in\s+crate\s*::\s*repositories\s*\)\s+use\s+session\s*::\s*SqliteRepositorySession/.test(sqliteMod)) {
    failures.push(failure(SQLITE_SESSION_PATH, 'SQLite repository session visibility must not escape crate::repositories'))
  }
  if (!/struct\s+QueryOnlyGuard\b/.test(sqliteSession)
      || !/impl\s+Drop\s+for\s+QueryOnlyGuard/.test(sqliteSession)
      || !/pragma_query_value\s*\(/.test(sqliteSession)
      || (sqliteSession.match(/pragma_update\s*\(/g)?.length ?? 0) < 2
      || !/fn\s+restore\s*\(/.test(sqliteSession)) {
    failures.push(failure(SQLITE_SESSION_PATH, 'repository read capability must enforce and restore SQLite query_only with an RAII guard'))
  }
  if (!/RepositoryAccess\s*::\s*ReadOnly/.test(repositoryText)
      || !/RepositoryError\s*::\s*PreviewWriteDenied/.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'Preview must map to ReadOnly and writes must fail closed'))
  }
  if (/ReadOnlyPreview[\s\S]{0,160}RepositoryAccess\s*::\s*ReadWrite/.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'Preview must never map to ReadWrite'))
  }
  if (/ExecutionMode\s*::\s*Simulation[\s\S]{0,180}Namespace\s*::\s*Production/.test(repositoryText)
      || /Simulation[\s\S]{0,180}(?:fallback|default)[\s\S]{0,180}Production/i.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'Simulation must not fall back to Production storage'))
  }
  if (!/SimulationUnsupported/.test(repositoryText) || !/TenantScopeRequired/.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'Simulation and platform tenant-repository binds must fail closed'))
  }
  if (/default[_\s-]*tenant|fallback[_\s-]*tenant|TenantId\s*::\s*new\s*\(\s*"(?:default|fallback)/i.test(repositoryText)) {
    failures.push(failure(REPOSITORY_PREFIX, 'default or fallback tenant selection is forbidden'))
  }

  if (!exactApplicationServices(structFields(services, 'ApplicationServices'))) {
    failures.push(failure(SERVICES_PATH, 'ApplicationServices must contain exactly four private approved service handles'))
  }
  if (/\b(?:repository_provider|scoped_repositories)\s*:|\b(?:RepositoryProvider|ScopedRepositories)\b/.test(structFields(state, 'AppState')?.map((field) => `${field.name}:${field.type}`).join('\n') ?? '')) {
    failures.push(failure(STATE_PATH, 'AppState must not own RepositoryProvider or ScopedRepositories'))
  }

  for (const [path, source] of allRuntime) {
    if (/HashMap\s*<\s*String\s*,\s*(?:Arc\s*<\s*dyn\b|[^>]*(?:Repository|Service)\b)|\bdyn\s+Any\b|\bdowncast(?:_ref|_mut)?\s*\(|\bget_service\s*\(\s*["']/.test(source)) {
      failures.push(failure(path, 'generic service locator, Any, and downcast patterns are forbidden'))
    }
    if (path.startsWith(ROUTES_PREFIX) && /(?:use\s+crate\s*::\s*repositories|crate\s*::\s*repositories\s*::)/.test(source)) {
      failures.push(failure(path, 'routes must not import repository internals in SP-08'))
    }
    if (path.startsWith(ROUTES_PREFIX) && /\.\s*repository_provider\s*\(/.test(source)) {
      failures.push(failure(path, 'routes must not call repository_provider() in SP-08'))
    }
  }

  for (const path of repositoryEntries.map(([path]) => path)) {
    if (/(?:^|\/)(?:customer|quote|order|reservation|allocation|workflow|integration)\.rs$/.test(path)) {
      failures.push(failure(path, 'production domain repositories are deferred beyond SP-08'))
    }
  }
  for (const rawPath of changedPaths) {
    const path = normalizePath(rawPath)
    if (path.startsWith(CHANGED_PATH_DISCOVERY_FAILURE)) {
      failures.push(failure('git', 'changed-path discovery failed closed'))
    }
  }
  return failures
}

function collectRust(root, relativePath, files) {
  const directory = join(root, relativePath)
  if (!existsSync(directory)) return
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const child = join(relativePath, entry.name)
    if (entry.isDirectory()) collectRust(root, child, files)
    else if (entry.isFile() && entry.name.endsWith('.rs')) files[normalizePath(child)] = readFileSync(join(root, child), 'utf8')
  }
}

function loadRepositoryFiles(root) {
  const files = {}
  for (const path of ['backend/src/repositories', 'backend/src/application', 'backend/src/routes']) collectRust(root, path, files)
  for (const path of [MAIN_PATH, STATE_PATH]) if (existsSync(join(root, path))) files[path] = readFileSync(join(root, path), 'utf8')
  return files
}

export function discoverChangedPaths(root, execute = execFileSync) {
  try {
    const options = { cwd: root, encoding: 'utf8' }
    const committed = execute('git', ['diff', '--name-only', 'origin/prototype...HEAD'], options)
    const unstaged = execute('git', ['diff', '--name-only'], options)
    const staged = execute('git', ['diff', '--cached', '--name-only'], options)
    const untracked = execute('git', ['ls-files', '--others', '--exclude-standard'], options)
    return [...new Set(`${committed}\n${unstaged}\n${staged}\n${untracked}`.split(/\r?\n/).filter(Boolean))]
  } catch (error) {
    return [`${CHANGED_PATH_DISCOVERY_FAILURE}${error.message}`]
  }
}

function main() {
  const root = process.cwd()
  const failures = checkSp08ScopedRepositoryBoundary(loadRepositoryFiles(root), discoverChangedPaths(root))
  if (failures.length > 0) {
    console.error('SP-08 scoped repository boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('SP-08 scoped repository boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
