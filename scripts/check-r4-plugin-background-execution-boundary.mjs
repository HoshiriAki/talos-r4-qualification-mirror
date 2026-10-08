#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-BACKGROUND-EXECUTION'
export const PATH = 'backend/src/application/plugin_background_execution.rs'

function finding(evidence) {
  return { rule: RULE, path: PATH, evidence, occurrences: 1 }
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

export function checkR4PluginBackgroundExecution(source) {
  const failures = []
  for (const token of [
    'pub struct PostgresPluginBackgroundExecutionGate',
    'runtime_binding: PluginExecutionRuntimeBinding',
    'pub(crate) fn new_bound(',
    'pub async fn reauthorize_claim(',
    'INNER JOIN interconnect_plugin_work_pins',
    "w.status='leased'",
    'w.claim_generation=$3',
    'w.lease_owner=$4',
    'w.lease_deadline_ms=$5',
    'clock_timestamp()',
    '.bind(generation)',
    '.bind(claim.lease_owner.as_str())',
    '.bind(lease_deadline)',
    'let admission = self.admission.admit(context, &executable).await?;',
    'admission.require_runtime(&self.runtime_binding)?;',
    'PluginPermission::BackgroundJob(subject.as_str().to_owned())',
  ]) {
    if (!source.includes(token)) failures.push(finding(`missing required token: ${token}`))
  }

  const constructor = block(source, 'pub(crate) fn new_bound(')
  if (!/\bruntime_binding\s*:\s*PluginExecutionRuntimeBinding\b/.test(constructor)) {
    failures.push(finding('bound background constructor missing runtime binding parameter'))
  }
  for (const token of [
    'PostgresPluginAdmissionService::new(',
    'runtime_binding.clone()',
    'runtime_binding,',
  ]) {
    if (!constructor.includes(token)) {
      failures.push(finding(`bound background constructor missing required token: ${token}`))
    }
  }

  const signatureStart = source.indexOf('pub async fn reauthorize_claim(')
  const signatureEnd = signatureStart >= 0 ? source.indexOf(') -> Result<', signatureStart) : -1
  const signature = signatureStart >= 0 && signatureEnd >= 0
    ? source.slice(signatureStart, signatureEnd)
    : ''
  if (signature.includes('now_ms')) {
    failures.push(finding('background lease freshness accepts caller-selected clock authority'))
  }

  if (/\bpub\s+fn\s+new_bound\s*\(/.test(source)) {
    failures.push(finding('plugin caller can replace the host-owned bound background reauthorization policy'))
  }

  const testOnlyNew = source.indexOf('#[cfg(test)]')
  const publicNew = source.indexOf('pub fn new(')
  if (publicNew >= 0 && (testOnlyNew < 0 || publicNew < testOnlyNew)) {
    failures.push(finding('non-test plugin caller can construct an unbound background reauthorization gate'))
  }

  const queryStart = source.indexOf('let row = sqlx::query(')
  const queryEnd = queryStart >= 0 ? source.indexOf('.fetch_optional(&self.pool)', queryStart) : -1
  const query = queryStart >= 0 && queryEnd >= 0 ? source.slice(queryStart, queryEnd) : ''
  const requiredQueryTokens = [
    "w.status='leased'",
    'w.claim_generation=$3',
    'w.lease_owner=$4',
    'w.lease_deadline_ms=$5',
    'clock_timestamp()',
  ]
  if (requiredQueryTokens.some((token) => query.indexOf(token) < 0)) {
    failures.push(finding('background claim fencing query is incomplete'))
  }

  const admitIndex = source.indexOf('let admission = self.admission.admit(context, &executable).await?;')
  const runtimeIndex = source.indexOf('admission.require_runtime(&self.runtime_binding)?;', admitIndex)
  const subjectIndex = source.indexOf('PluginPermission::BackgroundJob(subject.as_str().to_owned())', runtimeIndex)
  if (admitIndex < 0 || runtimeIndex < admitIndex || subjectIndex < runtimeIndex) {
    failures.push(finding('background execution does not enforce fresh admission -> runtime binding -> exact subject ordering'))
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
    ? [finding('required R4-P6 background execution implementation is missing')]
    : checkR4PluginBackgroundExecution(source)
  if (failures.length) {
    console.error('R4-P6 plugin background execution boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin background execution boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
