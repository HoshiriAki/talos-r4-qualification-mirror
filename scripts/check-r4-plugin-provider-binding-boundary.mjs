#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-PROVIDER-BINDING'

const PATHS = Object.freeze({
  host: 'backend/src/application/plugin_host.rs',
  interconnectPlugin: 'backend/src/application/interconnect_plugin.rs',
  interconnectContract: 'backend/system/core/src/transport/interconnect.rs',
  story: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p6-plugin-permission-isolation-story-packet.md',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireTokens(failures, path, source, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) failures.push(finding(path, `missing required token: ${token}`))
  }
}

function block(source, startToken) {
  const start = source.indexOf(startToken)
  if (start < 0) return ''
  let depth = 0
  let opened = false
  for (let index = start; index < source.length; index += 1) {
    if (source[index] === '{') {
      opened = true
      depth += 1
    } else if (source[index] === '}') {
      depth -= 1
      if (opened && depth === 0) return source.slice(start, index + 1)
    }
  }
  return source.slice(start)
}

export function checkR4PluginProviderBindingBoundary({ files }) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required provider-binding evidence is missing'))
  }
  if (failures.length) return failures

  requireTokens(failures, PATHS.interconnectContract, files[PATHS.interconnectContract], [
    'pub provider_instance_id: Option<ProviderInstanceRef>',
    'pub binding_revision: Option<BindingRevisionRef>',
    'self.provider_instance_id.is_some() != self.binding_revision.is_some()',
  ])

  requireTokens(failures, PATHS.interconnectPlugin, files[PATHS.interconnectPlugin], [
    'provider_instance_id,binding_revision',
  ])
  const insertPath = block(files[PATHS.interconnectPlugin], 'if work_inserted {')
  for (const [field, evidence] of [
    ['provider_instance_id', 'insert path does not bind provider instance from durable executable'],
    ['binding_revision', 'insert path does not bind provider binding revision from durable executable'],
  ]) {
    const expression = new RegExp(`admission\\s*\\.\\s*executable\\s*\\.\\s*${field}\\s*\\.\\s*as_ref\\(\\)`, 's')
    if (!expression.test(insertPath)) failures.push(finding(PATHS.interconnectPlugin, evidence))
  }

  const host = files[PATHS.host]
  requireTokens(failures, PATHS.host, host, [
    'ProviderBindingAuthorityUnavailable',
    'if executable.provider_instance_id.is_some() {',
    'return Err(PluginHostError::ProviderBindingAuthorityUnavailable);',
    'provider_bound_executable_fails_closed_until_host_owned_binding_authority_is_wired',
  ])
  if (/if\s+false\s*\{\s*return Err\(PluginHostError::ProviderBindingAuthorityUnavailable\)/s.test(host)) {
    failures.push(finding(PATHS.host, 'provider-bound admission fail-closed condition was disabled'))
  }

  requireTokens(failures, PATHS.story, files[PATHS.story], [
    'provider-bound plugin execution',
    'host-owned',
    'Integration',
  ])

  return failures
}

export function loadRepository(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return { files }
}

function main() {
  const failures = checkR4PluginProviderBindingBoundary(loadRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin provider-binding boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin provider-binding boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
