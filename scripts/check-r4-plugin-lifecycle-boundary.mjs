#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-LIFECYCLE'

const PATHS = Object.freeze({
  lifecycle: 'backend/src/application/plugin_lifecycle.rs',
  sqlite: 'backend/src/application/plugin_lifecycle_sqlite.rs',
  postgres: 'backend/src/application/plugin_lifecycle_postgres.rs',
  admission: 'backend/src/application/plugin_admission.rs',
  executionAdmission: 'backend/src/application/plugin_execution_admission.rs',
  executionAdmissionService: 'backend/src/application/plugin_execution_admission_service.rs',
  executionServices: 'backend/src/application/plugin_execution_services.rs',
  admissionIsolation: 'backend/src/application/plugin_admission_isolation.rs',
  verification: 'backend/src/application/plugin_verification.rs',
  appMod: 'backend/src/application/mod.rs',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireTokens(failures, path, source, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) failures.push(finding(path, `missing required token: ${token}`))
  }
}

function productionSource(source) {
  return source.split('#[cfg(test)]', 1)[0]
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

export function checkR4PluginLifecycleBoundary({ files, rustSources }) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required lifecycle/admission evidence is missing'))
  }
  if (failures.length) return failures

  const lifecycle = productionSource(files[PATHS.lifecycle])
  requireTokens(failures, PATHS.lifecycle, lifecycle, [
    'pub struct PluginInstallationActivation',
    'pub struct PluginUpgradeAuthorization',
    'pub previous_executable: PluginExecutableRef',
    'pub review: PluginUpgradeReview',
    'pub(crate) fn validate_activation(',
    '!r4_isolation_policy(activation.isolation_profile).enabled_in_r4',
    '.any(|permission| !package.permission_request.contains(permission))',
    'PluginLifecycleMutationError::GrantOutsideManifest',
    'pub(crate) fn validate_upgrade(',
    'authorization.review.matches_transition(previous, candidate)',
    'PluginLifecycleMutationError::UpgradeReviewMismatch',
  ])

  const sqlite = productionSource(files[PATHS.sqlite])
  requireTokens(failures, PATHS.sqlite, sqlite, [
    'pub fn stage_verified_package(',
    'verified: VerifiedPluginPackage',
    'let record = verified.into_record();',
    'record.lifecycle != PluginLifecycle::Staged',
    'VerificationEvidence::PublisherVerified',
    'transaction_with_behavior(TransactionBehavior::Immediate)',
    'SELECT COUNT(*) FROM plugin_installations WHERE tenant_id=?1 AND plugin_id=?2',
    'PluginLifecycleMutationError::UpgradeReviewRequired',
    'load_upgrade_source(',
    'validate_upgrade(&previous, &candidate, authorization)?;',
    "UPDATE plugin_packages SET lifecycle_state='active'",
    "'active',?12,?12)",
    'pub fn suspend_installation(',
    'pub fn revoke_installation(',
    'executable.capability_contract_version.as_str()',
  ])

  const postgres = productionSource(files[PATHS.postgres])
  requireTokens(failures, PATHS.postgres, postgres, [
    'pub async fn stage_verified_package(',
    'verified: VerifiedPluginPackage',
    'let record = verified.into_record();',
    'pg_advisory_xact_lock(hashtextextended($1, 0))',
    'SELECT COUNT(*)::BIGINT AS count FROM plugin_installations',
    'PluginLifecycleMutationError::UpgradeReviewRequired',
    'load_upgrade_source(',
    'validate_upgrade(&previous, &candidate, authorization)?;',
    'FOR UPDATE',
    "UPDATE plugin_packages SET lifecycle_state='active'",
    "'active',$12,$12)",
    'pub async fn suspend_installation(',
    'pub async fn revoke_installation(',
    'executable.capability_contract_version.as_str()',
  ])
  const lockIndex = postgres.indexOf('pg_advisory_xact_lock(hashtextextended($1, 0))')
  const countIndex = postgres.indexOf('SELECT COUNT(*)::BIGINT AS count FROM plugin_installations')
  if (lockIndex < 0 || countIndex < 0 || lockIndex > countIndex) {
    failures.push(finding(PATHS.postgres, 'tenant+plugin activation lock must be acquired before installation-history decision'))
  }

  const admission = productionSource(files[PATHS.admission])
  requireTokens(failures, PATHS.admission, admission, [
    'pub trait PluginPrincipalAuthorityResolver: Send + Sync',
    'principal_authority_resolver: Arc<dyn PluginPrincipalAuthorityResolver>',
    'runtime_template: TrustedPluginRuntimePolicy',
    'runtime_template.principal_authority = PermissionSet::empty();',
    '.principal_authority_resolver',
    '.resolve(context)',
    'PluginAdmissionServiceError::PrincipalAuthorityResolution',
    'let runtime = self.policies.runtime_for(context)?;',
    'pub struct SqlitePluginAdmissionService',
    'resolve_sqlite_plugin_facts(',
    'pub struct PostgresPluginAdmissionService',
    'resolve_postgres_plugin_facts(',
    'ProductionPluginHost::admit(',
    '.data_scope()',
    '.tenant_id_opt()',
  ])
  if (admission.includes('pub runtime_template:') || admission.includes('pub principal_authority_resolver:')) {
    failures.push(finding(PATHS.admission, 'long-lived admission policy internals became caller-mutable'))
  }
  for (const start of ['pub fn admit(', 'pub async fn admit(']) {
    const body = block(admission, start)
    if (body && (body.includes('package: &PluginPackageRecord') || body.includes('installation: &PluginInstallationRecord'))) {
      failures.push(finding(PATHS.admission, 'persisted admission accepts caller-supplied package/installation facts'))
    }
  }

  const executionAdmission = productionSource(files[PATHS.executionAdmission])
  requireTokens(failures, PATHS.executionAdmission, executionAdmission, [
    'pub struct PluginExecutionAdmission',
    'authority: PluginAdmission',
    'context_binding: PluginExecutionContextBinding',
    'runtime_binding: PluginExecutionRuntimeBinding',
    'pub(crate) fn new(',
    'actor: context.actor().clone()',
    'tenant_scope: context.tenant_scope().clone()',
    'data_scope: context.data_scope().clone()',
    'execution_mode: context.execution_mode().clone()',
    'correlation_id: context.correlation_id().clone()',
    'idempotency_key: context.idempotency_key().map(str::to_owned)',
    '&self.actor == context.actor()',
    '&& &self.tenant_scope == context.tenant_scope()',
    '&& &self.data_scope == context.data_scope()',
    '&& &self.execution_mode == context.execution_mode()',
    '&& &self.correlation_id == context.correlation_id()',
    'self.idempotency_key.as_deref() == context.idempotency_key()',
    'pub fn require_context(',
    'PluginExecutionAdmissionError::ContextMismatch',
    'pub(crate) fn require_runtime(',
    'PluginExecutionAdmissionError::RuntimeMismatch',
    'pub(crate) fn authority(&self) -> &PluginAdmission',
  ])
  if (executionAdmission.includes('pub authority:') || executionAdmission.includes('pub context_binding:')) {
    failures.push(finding(PATHS.executionAdmission, 'execution admission internals became publicly constructible/mutable'))
  }

  const executionAdmissionService = productionSource(files[PATHS.executionAdmissionService])
  requireTokens(failures, PATHS.executionAdmissionService, executionAdmissionService, [
    'inner: super::plugin_admission::SqlitePluginAdmissionService',
    'inner: super::plugin_admission::PostgresPluginAdmissionService',
    'Result<PluginExecutionAdmission, PluginAdmissionServiceError>',
  ])
  for (const serviceImplementation of ['impl SqlitePluginAdmissionService {', 'impl PostgresPluginAdmissionService {']) {
    const serviceBody = block(executionAdmissionService, serviceImplementation)
    if (!serviceBody.includes('PluginExecutionAdmission::new(') || !serviceBody.includes('self.runtime_binding.clone(),')) {
      failures.push(finding(PATHS.executionAdmissionService, `${serviceImplementation} must mint a runtime-bound execution admission`))
    }
  }

  const executionServices = productionSource(files[PATHS.executionServices])
  requireTokens(failures, PATHS.executionServices, executionServices, [
    'pub struct PluginExecutionEgressService',
    'pub struct PluginExecutionSecretService',
    'pub struct SqlitePluginExecutionStorageService',
    'pub struct PostgresPluginExecutionStorageService',
    'admission: &PluginExecutionAdmission',
    'admission.require_context(context)?;',
    'admission.authority()',
  ])
  for (const marker of ['pub async fn send(', 'pub fn sign(', 'pub fn decrypt(', 'pub fn put(', 'pub fn get(', 'pub async fn put(', 'pub async fn get(']) {
    const body = block(executionServices, marker)
    if (body && !body.includes('admission.require_context(context)?;')) {
      failures.push(finding(PATHS.executionServices, `${marker} must reject a transferred execution admission before host adapter use`))
    }
  }

  const isolation = productionSource(files[PATHS.admissionIsolation])
  requireTokens(failures, PATHS.admissionIsolation, isolation, [
    'use super::plugin_execution_admission::PluginExecutionAdmission;',
    'impl PluginExecutionAdmission',
    'pub fn technical_isolation_enforcement(&self) -> PluginIsolationEnforcementMatrix',
    'r4_plugin_isolation_enforcement(self.isolation().profile)',
    'not proof that the operating system prevents',
  ])

  requireTokens(failures, PATHS.verification, productionSource(files[PATHS.verification]), [
    'pub struct VerifiedPluginPackage',
    'pub(crate) fn into_record(self) -> PluginPackageRecord',
  ])

  const appMod = files[PATHS.appMod]
  requireTokens(failures, PATHS.appMod, appMod, [
    'mod plugin_admission;',
    'mod plugin_admission_isolation;',
    'mod plugin_execution_admission;',
    'mod plugin_execution_admission_service;',
    'mod plugin_execution_services;',
    'mod plugin_lifecycle;',
    'mod plugin_runtime;',
    'mod plugin_secret;',
    'mod plugin_storage;',
    'mod plugin_store;',
    'PluginPrincipalAuthorityResolver',
    'PluginExecutionAdmission',
    'PluginExecutionEgressService',
    'PluginExecutionSecretService',
    'SqlitePluginLifecycleService',
    'PostgresPluginLifecycleService',
  ])
  for (const moduleName of ['plugin_runtime', 'plugin_secret', 'plugin_storage', 'plugin_store']) {
    if (appMod.includes(`pub mod ${moduleName};`)) {
      failures.push(finding(PATHS.appMod, `low-level ${moduleName} module is public and can bypass the context-bound application surface`))
    }
  }
  if (/\bProductionPluginHost\s*,/.test(appMod)) {
    failures.push(finding(PATHS.appMod, 'low-level ProductionPluginHost is publicly re-exported'))
  }
  if (/\bPluginAdmission\s*,/.test(appMod)) {
    failures.push(finding(PATHS.appMod, 'raw PluginAdmission is publicly re-exported'))
  }
  if (/pub use plugin_admission::(?:Postgres|Sqlite)PluginAdmissionService/.test(appMod)) {
    failures.push(finding(PATHS.appMod, 'raw persisted admission service is publicly re-exported instead of the context-bound wrapper'))
  }
  if (appMod.includes('PluginEgressExecutor,') || appMod.includes('PluginSecretService,')) {
    failures.push(finding(PATHS.appMod, 'raw plugin operation adapter is publicly re-exported'))
  }

  for (const [path, source] of Object.entries(rustSources)) {
    if (path === PATHS.admission || path === 'backend/src/application/plugin_runtime.rs') continue
    if (path.endsWith('_tests.rs')) continue
    const production = productionSource(source)
    if (/\bProductionPluginHost::admit\(/.test(production)) {
      failures.push(finding(path, 'production code bypasses persistence-bound PluginAdmissionService'))
    }
  }

  return failures
}

function collectRust(root, directory, output) {
  const absolute = join(root, directory)
  if (!existsSync(absolute)) return
  for (const entry of readdirSync(absolute, { withFileTypes: true })) {
    const child = join(directory, entry.name)
    if (entry.isDirectory()) collectRust(root, child, output)
    else if (entry.isFile() && entry.name.endsWith('.rs')) {
      output[child.replaceAll('\\', '/')] = readFileSync(join(root, child), 'utf8')
    }
  }
}

export function loadPluginLifecycleRepository(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  const rustSources = {}
  collectRust(root, 'backend', rustSources)
  return { files, rustSources }
}

function main() {
  const failures = checkR4PluginLifecycleBoundary(loadPluginLifecycleRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin lifecycle/admission boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin lifecycle/admission boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
