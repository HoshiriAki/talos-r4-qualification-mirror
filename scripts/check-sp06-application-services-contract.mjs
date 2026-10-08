#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

const MODULE_CLIENT_PATH = 'backend/src/application/module_client.rs'
const APPLICATION_SERVICES_PATH = 'backend/src/application/services.rs'
const APP_STATE_PATH = 'backend/src/state.rs'

function failure(path, evidence) {
  return { rule: 'TALOS-OPS-016', path, evidence, occurrences: 1 }
}

function runtimeSource(source) {
  const normalized = source.replaceAll('\r\n', '\n').replaceAll('\r', '\n')
  const testBoundary = normalized.search(/#\[cfg\s*\(\s*test\s*\)\]/)
  return testBoundary === -1 ? normalized : normalized.slice(0, testBoundary)
}

function structBody(source, name) {
  return source.match(new RegExp(`(?:pub\\s+)?struct\\s+${name}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1] ?? null
}

function structFields(source, name) {
  const body = structBody(source, name)
  if (body === null) return null

  const fields = []
  for (const rawLine of body.split('\n')) {
    const line = rawLine.trim()
    if (!line || line.startsWith('#[') || line.startsWith('//')) continue
    const match = line.match(/^(pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+?)(?:,\s*)?$/)
    if (!match) continue
    fields.push({
      visibility: (match[1] ?? '').trim(),
      name: match[2],
      type: match[3].trim(),
    })
  }
  return fields
}

function fieldNames(fields) {
  return fields.map((field) => field.name).sort()
}

function sameNames(actual, expected) {
  return JSON.stringify(fieldNames(actual)) === JSON.stringify([...expected].sort())
}

const MODULE_CLIENT_TRAIT_SIGNATURE = /pub\s+trait\s+ModuleClient\s*:\s*Send\s*\+\s*Sync\s*\{\s*fn\s+execute\s*\(\s*&self\s*,\s*module_name\s*:\s*&str\s*,\s*command\s*:\s*&str\s*,\s*payload\s*:\s*Value\s*,\s*ctx\s*:\s*&ExecutionContext\s*,?\s*\)\s*->\s*Result\s*<\s*Value\s*,\s*String\s*>\s*;\s*\}/m
const REGISTRY_CLIENT_IMPL_SIGNATURE = /impl\s+ModuleClient\s+for\s+RegistryModuleClient\s*\{[\s\S]*?fn\s+execute\s*\(\s*&self\s*,\s*module_name\s*:\s*&str\s*,\s*command\s*:\s*&str\s*,\s*payload\s*:\s*Value\s*,\s*ctx\s*:\s*&ExecutionContext\s*,?\s*\)\s*->\s*Result\s*<\s*Value\s*,\s*String\s*>\s*\{[\s\S]*?self\s*\.\s*registry\s*\.\s*execute\s*\(\s*module_name\s*,\s*command\s*,\s*payload\s*,\s*ctx\s*\)[\s\S]*?\}\s*\}/m

export function checkSp06ApplicationServicesContract(files) {
  const failures = []
  const moduleClient = runtimeSource(files[MODULE_CLIENT_PATH] ?? '')
  const services = runtimeSource(files[APPLICATION_SERVICES_PATH] ?? '')
  const state = runtimeSource(files[APP_STATE_PATH] ?? '')

  if (!MODULE_CLIENT_TRAIT_SIGNATURE.test(moduleClient)) {
    failures.push(failure(
      MODULE_CLIENT_PATH,
      'ModuleClient::execute ABI must remain (&self, module_name: &str, command: &str, payload: Value, ctx: &ExecutionContext) -> Result<Value, String>',
    ))
  }

  const registryFields = structFields(moduleClient, 'RegistryModuleClient')
  if (registryFields === null
      || !sameNames(registryFields, ['registry'])
      || registryFields.some((field) => field.visibility !== '')
      || registryFields[0]?.type.replaceAll(/\s+/g, '') !== 'Arc<ModuleRegistry>') {
    failures.push(failure(
      MODULE_CLIENT_PATH,
      'RegistryModuleClient must contain exactly one private Arc<ModuleRegistry> field',
    ))
  }

  if (!REGISTRY_CLIENT_IMPL_SIGNATURE.test(moduleClient)
      || /(?:self\s*\.\s*)?registry\s*\.\s*get\s*\(/.test(moduleClient)
      || /SystemModule\s*::\s*execute\s*\(/.test(moduleClient)) {
    failures.push(failure(
      MODULE_CLIENT_PATH,
      'RegistryModuleClient must preserve the complete ABI and delegate only to ModuleRegistry::execute',
    ))
  }

  const serviceFields = structFields(services, 'ApplicationServices')
  if (serviceFields === null
      || !sameNames(serviceFields, ['clock', 'module_client', 'worker_runner', 'repository_provider'])
      || serviceFields.some((field) => field.visibility !== '')
      || serviceFields.find((field) => field.name === 'clock')?.type.replaceAll(/\s+/g, '') !== 'Arc<dynClock>'
      || serviceFields.find((field) => field.name === 'module_client')?.type.replaceAll(/\s+/g, '') !== 'Arc<dynModuleClient>'
      || serviceFields.find((field) => field.name === 'worker_runner')?.type.replaceAll(/\s+/g, '') !== 'Arc<dynWorkerRunner>'
      || serviceFields.find((field) => field.name === 'repository_provider')?.type.replaceAll(/\s+/g, '') !== 'Arc<dynRepositoryProvider>') {
    failures.push(failure(
      APPLICATION_SERVICES_PATH,
      'ApplicationServices must contain exactly four private Clock, ModuleClient, WorkerRunner, and RepositoryProvider handles',
    ))
  }

  if (!/pub\s+fn\s+worker_runner\s*\(\s*&self\s*\)\s*->\s*Arc\s*<\s*dyn\s+WorkerRunner\s*>\s*\{\s*self\s*\.\s*worker_runner\s*\.\s*clone\s*\(\s*\)\s*\}/m.test(services)) {
    failures.push(failure(
      APPLICATION_SERVICES_PATH,
      'ApplicationServices must expose only a cloned Arc<dyn WorkerRunner> accessor',
    ))
  }

  if (!/pub\s+fn\s+repository_provider\s*\(\s*&self\s*\)\s*->\s*Arc\s*<\s*dyn\s+RepositoryProvider\s*>\s*\{\s*self\s*\.\s*repository_provider\s*\.\s*clone\s*\(\s*\)\s*\}/m.test(services)) {
    failures.push(failure(
      APPLICATION_SERVICES_PATH,
      'ApplicationServices must expose only a cloned Arc<dyn RepositoryProvider> accessor',
    ))
  }

  const stateFields = structFields(state, 'AppState')
  const allowedStateFields = [
    'config',
    'registry',
    'http_client',
    'integration_webhook_ingress',
    'integration_webhook_rate_limiter',
    'application_services',
    'audit_compatibility_repository',
    'auth_security_repository',
    'identity_authority_repository',
    'machine_authority_repository',
    'platform_membership_repository',
    'platform_tenant_repository',
    'tenant_resolution_repository',
    'pg_pool',
  ]
  if (stateFields === null || !sameNames(stateFields, allowedStateFields)) {
    failures.push(failure(
      APP_STATE_PATH,
      `AppState field set must remain exactly: ${allowedStateFields.join(', ')}`,
    ))
  } else {
    const applicationServicesField = stateFields.find((field) => field.name === 'application_services')
    if (applicationServicesField?.visibility !== ''
        || applicationServicesField?.type.replaceAll(/\s+/g, '') !== 'Arc<ApplicationServices>') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState application_services must remain a private Arc<ApplicationServices>',
      ))
    }

    const webhookIngressField = stateFields.find((field) => field.name === 'integration_webhook_ingress')
    if (webhookIngressField?.visibility !== 'pub'
        || webhookIngressField?.type.replaceAll(/\s+/g, '') !== 'Arc<FixtureWebhookIngress>') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState integration_webhook_ingress must remain a public Arc<FixtureWebhookIngress> boundary handle',
      ))
    }

    const webhookRateLimiterField = stateFields.find((field) => field.name === 'integration_webhook_rate_limiter')
    if (webhookRateLimiterField?.visibility !== 'pub'
        || webhookRateLimiterField?.type.replaceAll(/\s+/g, '') !== 'Arc<FixtureWebhookRateLimiter>') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState integration_webhook_rate_limiter must remain a public Arc<FixtureWebhookRateLimiter> boundary handle',
      ))
    }

    const auditCompatibilityField = stateFields.find((field) => field.name === 'audit_compatibility_repository')
    if (auditCompatibilityField?.visibility !== ''
        || auditCompatibilityField?.type.replaceAll(/\s+/g, '') !== 'AuditCompatibilityRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState audit_compatibility_repository must remain a private AuditCompatibilityRepository authority handle',
      ))
    }

    const authSecurityField = stateFields.find((field) => field.name === 'auth_security_repository')
    if (authSecurityField?.visibility !== ''
        || authSecurityField?.type.replaceAll(/\s+/g, '') !== 'AuthSecurityRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState auth_security_repository must remain a private AuthSecurityRepository authority handle',
      ))
    }

    const identityAuthorityField = stateFields.find((field) => field.name === 'identity_authority_repository')
    if (identityAuthorityField?.visibility !== ''
        || identityAuthorityField?.type.replaceAll(/\s+/g, '') !== 'IdentityAuthorityRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState identity_authority_repository must remain a private IdentityAuthorityRepository authority handle',
      ))
    }

    const machineAuthorityField = stateFields.find((field) => field.name === 'machine_authority_repository')
    if (machineAuthorityField?.visibility !== ''
        || machineAuthorityField?.type.replaceAll(/\s+/g, '') !== 'MachineAuthorityRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState machine_authority_repository must remain a private MachineAuthorityRepository authority handle',
      ))
    }

    const platformMembershipField = stateFields.find((field) => field.name === 'platform_membership_repository')
    if (platformMembershipField?.visibility !== ''
        || platformMembershipField?.type.replaceAll(/\s+/g, '') !== 'PlatformMembershipRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState platform_membership_repository must remain a private PlatformMembershipRepository authority handle',
      ))
    }

    const platformTenantField = stateFields.find((field) => field.name === 'platform_tenant_repository')
    if (platformTenantField?.visibility !== ''
        || platformTenantField?.type.replaceAll(/\s+/g, '') !== 'PlatformTenantRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState platform_tenant_repository must remain a private PlatformTenantRepository authority handle',
      ))
    }

    const tenantResolutionField = stateFields.find((field) => field.name === 'tenant_resolution_repository')
    if (tenantResolutionField?.visibility !== ''
        || tenantResolutionField?.type.replaceAll(/\s+/g, '') !== 'TenantResolutionRepository') {
      failures.push(failure(
        APP_STATE_PATH,
        'AppState tenant_resolution_repository must remain a private TenantResolutionRepository authority handle',
      ))
    }
  }

  return failures
}

function loadRepositoryFiles(root) {
  const paths = [MODULE_CLIENT_PATH, APPLICATION_SERVICES_PATH, APP_STATE_PATH]
  const files = {}
  for (const path of paths) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

function main() {
  const failures = checkSp06ApplicationServicesContract(loadRepositoryFiles(process.cwd()))
  if (failures.length > 0) {
    console.error('SP-06 ApplicationServices contract check failed:')
    for (const item of failures) {
      console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    }
    process.exitCode = 1
    return
  }
  console.log('SP-06 ApplicationServices contract check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
