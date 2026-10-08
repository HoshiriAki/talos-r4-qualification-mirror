#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-SECRET-EGRESS'
export const PATH = 'backend/src/application/plugin_execution_services.rs'

function finding(evidence) {
  return { rule: RULE, path: PATH, evidence, occurrences: 1 }
}

export function checkR4PluginSecretEgress(source) {
  const failures = []
  for (const token of [
    'runtime_binding: PluginExecutionRuntimeBinding',
    'admission.require_runtime(&self.runtime_binding)?;',
    'admission.require_context(context)?;',
    'PluginExecutionEgressError::SecretPurposeRequired',
    'PluginExecutionEgressError::AmbiguousSecretPurpose',
    '.filter(|permission| matches!(permission, PluginPermission::SecretPurpose(_)))',
    '.count()',
    'if secret_purpose_count > 1 {',
    'if secret_purpose_count == 1 && secret_purpose.is_none() {',
    '.send(admission.authority(), grant_id, secret_purpose, request)',
  ]) {
    if (!source.includes(token)) failures.push(finding(`missing required token: ${token}`))
  }

  const sendStart = source.indexOf('pub async fn send(')
  const sendEnd = sendStart >= 0 ? source.indexOf('\n    }\n}', sendStart) : -1
  const send = sendStart >= 0 && sendEnd >= 0 ? source.slice(sendStart, sendEnd) : ''
  const runtimeIndex = send.indexOf('admission.require_runtime(&self.runtime_binding)?;')
  const contextIndex = send.indexOf('admission.require_context(context)?;')
  const countIndex = send.indexOf('let secret_purpose_count =')
  const ambiguityIndex = send.indexOf('if secret_purpose_count > 1')
  const requiredIndex = send.indexOf('if secret_purpose_count == 1 && secret_purpose.is_none()')
  const dispatchIndex = send.indexOf('.send(admission.authority(), grant_id, secret_purpose, request)')
  const order = [runtimeIndex, contextIndex, countIndex, ambiguityIndex, requiredIndex, dispatchIndex]
  if (order.some((index) => index < 0) || order.some((index, offset) => offset > 0 && index < order[offset - 1])) {
    failures.push(finding('runtime/context/secret-purpose guards are not ordered before governed dispatch'))
  }

  if (/if\s+false\s*\{\s*return Err\(PluginExecutionEgressError::AmbiguousSecretPurpose\)/s.test(source)) {
    failures.push(finding('multi-purpose secret egress ambiguity guard was disabled'))
  }
  if (/if\s+false\s*\{\s*return Err\(PluginExecutionEgressError::SecretPurposeRequired\)/s.test(source)) {
    failures.push(finding('secret-capable egress purpose guard was disabled'))
  }

  return failures
}

export function loadSource(root) {
  const absolute = join(root, PATH)
  if (!existsSync(absolute)) return null
  return readFileSync(absolute, 'utf8')
}

function main() {
  const source = loadSource(process.cwd())
  const failures = source === null
    ? [finding('required R4-P6 execution service implementation is missing')]
    : checkR4PluginSecretEgress(source)
  if (failures.length) {
    console.error('R4-P6 plugin secret-egress boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin secret-egress boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
