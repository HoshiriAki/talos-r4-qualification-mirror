#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-LAYERING-BOUNDARY'

const PATHS = Object.freeze({
  interconnect: 'backend/system/core/src/transport/interconnect.rs',
  sqlite067: 'backend/src/db/migrations/067_r4_interconnect_fabric.sql',
  postgres067: 'backend/src/db/migrations/postgres/067_r4_interconnect_fabric.sql',
  egress: 'backend/src/application/plugin_egress.rs',
  executionServices: 'backend/src/application/plugin_execution_services.rs',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
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

function requireTokens(failures, path, source, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) failures.push(finding(path, `missing required token: ${token}`))
  }
}

export function checkR4PluginLayeringBoundary({ files }) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required layering evidence is missing'))
  }
  if (failures.length) return failures

  const executable = block(files[PATHS.interconnect], 'pub struct PluginExecutableRef')
  requireTokens(failures, PATHS.interconnect, executable, [
    'pub plugin_id: PluginId',
    'pub version: ContractVersion',
    'pub capability_contract_version: ContractVersion',
    'pub provider_instance_id: Option<ProviderInstanceRef>',
    'pub binding_revision: Option<BindingRevisionRef>',
  ])
  if (/\bPublisherId\b|\bpublisher_id\b/.test(executable)) {
    failures.push(finding(
      PATHS.interconnect,
      'P3 PluginExecutableRef must not absorb P6 publisher provenance',
    ))
  }

  for (const migrationPath of [PATHS.sqlite067, PATHS.postgres067]) {
    if (/\bpublisher_id\b/.test(files[migrationPath])) {
      failures.push(finding(
        migrationPath,
        'frozen R4-P3 migration 067 must not be rewritten with P6 publisher provenance',
      ))
    }
  }

  const egress = files[PATHS.egress]
  const grantPolicy = block(egress, 'pub struct PluginEgressGrantPolicy')
  requireTokens(failures, PATHS.egress, grantPolicy, ['grants: BTreeMap<String, EgressGrant>'])
  if (egress.includes('HostOwnedPluginEgressRequest')) {
    failures.push(finding(
      PATHS.egress,
      'P6 must own EgressGrant authority, not freeze the complete per-call ExternalRequest',
    ))
  }
  const executor = block(egress, 'impl PluginEgressExecutor')
  requireTokens(failures, PATHS.egress, executor, [
    'request: ExternalRequest',
    'let grant = self.grant_policy.resolve(grant_id)?;',
    'dispatcher: GovernedEgressDispatcher',
    'let _shared_permit = state.acquire()?;',
    'self.dispatcher',
  ])
  const sharedBudget = executor.indexOf('let _shared_permit = state.acquire()?;')
  const p5Dispatch = executor.indexOf('self.dispatcher')
  if (sharedBudget < 0 || p5Dispatch < 0 || sharedBudget >= p5Dispatch) {
    failures.push(finding(
      PATHS.egress,
      'P6 shared budget must be acquired before handing dynamic request data to P5 governed dispatch',
    ))
  }

  const publicEgress = block(files[PATHS.executionServices], 'impl PluginExecutionEgressService')
  requireTokens(failures, PATHS.executionServices, publicEgress, [
    'grant_id: &str',
    'mut request: ExternalRequest',
    'admission.require_context(context)?;',
    'request.correlation_id = context.correlation_id().as_str().to_owned();',
    '.send(admission.authority(), grant_id, secret_purpose, request)',
  ])
  if (/\bgrant\s*:\s*EgressGrant\b/.test(publicEgress)) {
    failures.push(finding(
      PATHS.executionServices,
      'public plugin egress must reference host-owned grant authority by grant_id only',
    ))
  }
  const contextCheck = publicEgress.indexOf('admission.require_context(context)?;')
  const correlationRewrite = publicEgress.indexOf(
    'request.correlation_id = context.correlation_id().as_str().to_owned();',
  )
  const dispatch = publicEgress.indexOf(
    '.send(admission.authority(), grant_id, secret_purpose, request)',
  )
  if (!(contextCheck >= 0 && correlationRewrite > contextCheck && dispatch > correlationRewrite)) {
    failures.push(finding(
      PATHS.executionServices,
      'trusted egress correlation must be derived from ExecutionContext before governed dispatch',
    ))
  }

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
  const failures = checkR4PluginLayeringBoundary(loadRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin layering boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin layering boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
