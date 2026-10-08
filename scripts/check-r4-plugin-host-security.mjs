#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-HOST-SECURITY'

const PATHS = Object.freeze({
  plugin: 'backend/system/core/src/plugin.rs',
  compatibility: 'backend/system/core/src/plugin_compatibility.rs',
  isolation: 'backend/system/core/src/plugin_isolation.rs',
  enforcement: 'backend/system/core/src/plugin_permission_enforcement.rs',
  upgrade: 'backend/system/core/src/plugin_upgrade.rs',
  secretCore: 'backend/system/core/src/plugin_secret.rs',
  appMod: 'backend/src/application/mod.rs',
  services: 'backend/src/application/services.rs',
  main: 'backend/src/main.rs',
  host: 'backend/src/application/plugin_host.rs',
  runtime: 'backend/src/application/plugin_runtime.rs',
  runtimeServices: 'backend/src/application/plugin_runtime_services.rs',
  executionAdmission: 'backend/src/application/plugin_execution_admission.rs',
  executionServices: 'backend/src/application/plugin_execution_services.rs',
  operations: 'backend/src/application/plugin_host_operations.rs',
  backgroundExecution: 'backend/src/application/plugin_background_execution.rs',
  egress: 'backend/src/application/plugin_egress.rs',
  secret: 'backend/src/application/plugin_secret.rs',
  storage: 'backend/src/application/plugin_storage.rs',
  store: 'backend/src/application/plugin_store.rs',
  conformance: 'backend/src/application/plugin_conformance_tests.rs',
  sqliteMigration: 'backend/src/db/migrations/068_r4_plugin_host_security.sql',
  postgresMigration: 'backend/src/db/migrations/postgres/068_r4_plugin_host_security.sql',
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

export function checkR4PluginHostSecurity({ files, rustSources }) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R4-P6 evidence is missing'))
  }
  if (failures.length) return failures

  const plugin = files[PATHS.plugin]
  requireTokens(failures, PATHS.plugin, plugin, [
    'pub enum PluginPermission',
    'CapabilityInvoke',
    'CapabilityProvide',
    'DataReadProjection',
    'DataWriteProjection',
    'NetworkGrant',
    'SecretPurpose',
    'EventEmit',
    'EventSubscribe',
    'ExternalEffect',
    'BackgroundJob',
    'PluginStorage',
    'UiExtension',
    'pub struct PermissionResolutionInput',
    'pub capability_contract_version: ContractVersion',
    '&& self.capability_contract_version == executable.capability_contract_version',
    'matches!(self, Self::PublisherVerified { .. })',
    'PluginIsolationProfile::SingleFileWebApp',
  ])
  const pluginPermissions = block(plugin, 'impl PluginPermission {')
  if (!/Self::BackgroundJob\(value\)[\s\S]*?subject\.is_reserved\(\)[\s\S]*?PluginContractError::ReservedBackgroundJobSubject/s.test(pluginPermissions)) {
    failures.push(finding(PATHS.plugin, 'reserved BackgroundJob subjects are not rejected during permission validation'))
  }
  const resolution = block(plugin, 'pub fn resolve_effective_permissions')
  requireTokens(failures, PATHS.plugin, resolution, [
    'input.manifest_request',
    '.intersection(&input.installation_grant)',
    '.intersection(&input.tenant_policy)',
    '.intersection(&input.principal_authority)',
    '.intersection(&input.execution_plane_policy)',
    '.intersection(&input.runtime_policy)',
  ])

  requireTokens(failures, PATHS.compatibility, files[PATHS.compatibility], [
    'pub fn host_contract_is_compatible',
    "raw.split(',')",
    'Comparator::Gte',
    'Comparator::Lt',
  ])

  const isolation = files[PATHS.isolation]
  requireTokens(failures, PATHS.isolation, isolation, [
    'IsolationMechanism::TrustedBuildBoundary',
    'IsolationMechanism::SandboxRequired',
    'IsolationMechanism::RemoteBoundaryRequired',
    'AmbientAuthorityClass::FilesystemSyscall',
    'AmbientAuthorityClass::NetworkSyscall',
    'AmbientAuthorityClass::ProcessMemory',
    'trusted_not_technically_prevented',
  ])
  const nonNative = block(isolation, 'PluginIsolationProfile::VerifiedSandboxed')
  if (!nonNative.includes('enabled_in_r4: false')) {
    failures.push(finding(PATHS.isolation, 'sandbox/remote profiles are not fail-closed in R4'))
  }

  requireTokens(failures, PATHS.enforcement, files[PATHS.enforcement], [
    'PluginPermission::CapabilityInvoke(_)',
    'PluginPermissionEnforcement::RegistryCapabilityInvoke',
    'PluginPermission::CapabilityProvide(_) => None',
    'PluginPermission::DataReadProjection(_) => None',
    'PluginPermission::DataWriteProjection(_) => None',
    'PluginPermission::NetworkGrant(_)',
    'PluginPermissionEnforcement::GovernedP5Egress',
    'PluginPermission::SecretPurpose(_)',
    'PluginPermissionEnforcement::PurposeBoundSecretOperation',
    'PluginPermission::EventEmit(_) | PluginPermission::EventSubscribe(_)',
    'PluginPermissionEnforcement::P3EventLane',
    'PluginPermission::ExternalEffect(_) => None',
    'PluginPermission::BackgroundJob(_)',
    'PluginPermissionEnforcement::P3WorkLane',
    'PluginPermission::PluginStorage(_)',
    'PluginPermissionEnforcement::TenantPluginScopedStorage',
    'PluginPermission::UiExtension(_) => None',
  ])

  requireTokens(failures, PATHS.upgrade, files[PATHS.upgrade], [
    'pub fn permission_diff',
    'pub fn review_package_transition',
    'PluginUpgradeMigrationStrategy::PreserveExactPins',
    'PluginUpgradeError::PublisherChanged',
    'PluginUpgradeError::AddedPermissionNotApproved',
    'PluginUpgradeError::ApprovalOutsideCurrentDiff',
  ])

  const host = files[PATHS.host]
  requireTokens(failures, PATHS.host, host, [
    'host_contract_is_compatible',
    'package.identity != installation.package_identity',
    '!package.identity.matches_executable(executable)',
    'ProviderBindingAuthorityUnavailable',
    'if executable.provider_instance_id.is_some() {',
    'manifest_request: package.permission_request.clone()',
    'installation_grant: installation.installation_grant.clone()',
    'tenant_policy: installation.tenant_policy.clone()',
    'principal_authority: trusted_policy.principal_authority.clone()',
    'execution_plane_policy: trusted_policy.plane_policy(context)',
    'runtime_policy: trusted_policy.runtime_policy.clone()',
  ])
  if (!host.includes('pub(crate) struct PluginHost')) {
    failures.push(finding(PATHS.host, 'low-level PluginHost became public outside the application boundary'))
  }

  const runtime = files[PATHS.runtime]
  requireTokens(failures, PATHS.runtime, runtime, [
    'pub struct TrustedNativePublisherPolicy',
    'publisher_key_id',
    'NativePublisherNotTrusted',
    '&& !native_publishers.allows_native_package(package)',
    'r4_permission_enforcement',
    'UnsupportedPermissionInProfile',
    'pub struct TrustedDangerousPluginCombinationPolicy',
    'PluginPermission::BackgroundJob',
    'PluginPermission::SecretPurpose',
    'PluginPermission::NetworkGrant',
    'DangerousPermissionCombinationDenied',
    'dangerous_combinations.allows_effective_permissions(&admission)',
  ])

  const runtimeServices = files[PATHS.runtimeServices]
  requireTokens(failures, PATHS.runtimeServices, runtimeServices, [
    'pub(crate) struct PluginRuntimeSecurityProfile',
    'pub struct PluginRuntimeServices',
    'let runtime_binding = PluginExecutionRuntimeBinding::new();',
    'PluginEgressBudgetRegistry::new()',
    'PluginHostOperations::development(',
    'PluginHostOperations::postgres(',
    'PostgresDurableDriver::new(pool.clone())',
    'PluginExecutionEgressService::new(',
    'PluginExecutionSecretService::new(',
    'SqlitePluginAdmissionService::new(',
    'SqlitePluginExecutionStorageService::new(',
    'PostgresPluginAdmissionService::new(',
    'PostgresPluginExecutionStorageService::new(',
    'PostgresPluginBackgroundExecutionGate::new_bound(',
    'runtime_binding.clone()',
    'pub async fn admit(',
    'pub async fn storage_put(',
    'pub async fn storage_get(',
  ])
  if (/\bpub\s+fn\s+(?:sqlite|postgres)\s*\(/.test(runtimeServices)) {
    failures.push(finding(PATHS.runtimeServices, 'plugin runtime composition constructor became externally public'))
  }

  const appMod = files[PATHS.appMod]
  for (const privateModule of [
    'interconnect_plugin',
    'plugin_runtime',
    'plugin_runtime_services',
    'plugin_store',
    'plugin_secret',
    'plugin_storage',
  ]) {
    if (appMod.includes(`pub mod ${privateModule};`)) {
      failures.push(finding(PATHS.appMod, `raw plugin module became public: ${privateModule}`))
    }
  }
  if (/\bPluginHost\s*,/.test(appMod) || /pub use[^;]*\bPluginAdmission\b/s.test(appMod)) {
    failures.push(finding(PATHS.appMod, 'raw plugin admission authority is publicly re-exported'))
  }
  if (appMod.includes('pub use plugin_execution_admission_service::')) {
    failures.push(finding(PATHS.appMod, 'partial admission service was publicly re-exported'))
  }

  const executionAdmission = files[PATHS.executionAdmission]
  requireTokens(failures, PATHS.executionAdmission, executionAdmission, [
    'pub struct PluginExecutionAdmission',
    'context_binding: PluginExecutionContextBinding',
    'runtime_binding: PluginExecutionRuntimeBinding',
    'PluginExecutionAdmissionError::ContextMismatch',
    'PluginExecutionAdmissionError::RuntimeMismatch',
    'pub fn require_context(',
    'pub(crate) fn require_runtime(',
    'self.runtime_binding.matches(runtime_binding)',
    'pub(crate) fn authority(&self) -> &PluginAdmission',
  ])

  const executionServices = files[PATHS.executionServices]
  requireTokens(failures, PATHS.executionServices, executionServices, [
    'PluginExecutionRuntimeBinding',
    'runtime_binding: PluginExecutionRuntimeBinding',
    'admission.require_runtime(&self.runtime_binding)?;',
    'admission.require_context(context)?;',
    'PluginExecutionEgressService',
    'grant_id: &str',
    'PluginExecutionEgressError::SecretPurposeRequired',
    'PluginExecutionEgressError::AmbiguousSecretPurpose',
    '.send(admission.authority(), grant_id, secret_purpose, request)',
    'PluginExecutionSecretService',
    'pub fn sign(',
    'SqlitePluginExecutionStorageService',
    'PostgresPluginExecutionStorageService',
  ])
  const publicEgress = block(executionServices, 'impl PluginExecutionEgressService')
  if (/\bgrant\s*:\s*EgressGrant\b/.test(publicEgress)) {
    failures.push(finding(PATHS.executionServices, 'public plugin egress can supply full EgressGrant authority'))
  }
  const publicSecret = block(executionServices, 'impl PluginExecutionSecretService')
  if (publicSecret.includes('pub fn decrypt(')) {
    failures.push(finding(PATHS.executionServices, 'public plugin secret surface can export decrypted plaintext'))
  }

  const operations = files[PATHS.operations]
  requireTokens(failures, PATHS.operations, operations, [
    'pub struct PluginHostOperations',
    'runtime_binding: PluginExecutionRuntimeBinding',
    'pub(crate) fn development(',
    'pub(crate) fn postgres(',
    'capability_invoke_permission(request)',
    "if value.contains('.') {",
    'canonical.as_str() != value',
    'PluginPermission::EventEmit(envelope.subject.clone())',
    'PluginPermission::EventSubscribe(subject.clone())',
    'let consumer = plugin_consumer_id(admission)?;',
    'PluginPermission::BackgroundJob(envelope.subject.as_str().to_owned())',
    'executable: admission.executable().clone()',
    'enqueue_postgres_plugin_work(driver, context, work).await',
  ])
  const permissionGuard = block(operations, 'fn require_permission(')
  if (!/admission\s*\.\s*require_runtime\(runtime_binding\)/s.test(permissionGuard)) {
    failures.push(finding(PATHS.operations, 'host operation guard does not bind the admission to its runtime'))
  }
  const consumerIdentity = block(operations, 'fn plugin_consumer_id(')
  for (const [field, evidence] of [
    ['provider_instance_id', 'event consumer identity omits persisted provider instance'],
    ['binding_revision', 'event consumer identity omits persisted binding revision'],
  ]) {
    const expression = new RegExp(`executable\\s*\\.\\s*${field}\\s*\\.\\s*as_ref\\(\\)`, 's')
    if (!expression.test(consumerIdentity)) failures.push(finding(PATHS.operations, evidence))
  }
  if (/pub async fn replay_events\s*\([^)]*ConsumerId/s.test(operations)) {
    failures.push(finding(PATHS.operations, 'plugin Event subscription can supply a foreign ConsumerId'))
  }
  if (/pub async fn enqueue_background_job\s*\([^)]*PluginExecutableRef/s.test(operations)) {
    failures.push(finding(PATHS.operations, 'plugin BackgroundJob can supply its own executable pin'))
  }
  if (operations.includes('pub fn plan_external_effect(')) {
    failures.push(finding(PATHS.operations, 'unpinned ExternalEffect is exposed as an executable R4 plugin operation'))
  }

  const background = files[PATHS.backgroundExecution]
  requireTokens(failures, PATHS.backgroundExecution, background, [
    'pub struct PostgresPluginBackgroundExecutionGate',
    'runtime_binding: PluginExecutionRuntimeBinding',
    'pub(crate) fn new_bound(',
    'INNER JOIN interconnect_plugin_work_pins',
    "w.status='leased'",
    'w.claim_generation=$3',
    'w.lease_owner=$4',
    'w.lease_deadline_ms=$5',
    '.bind(generation)',
    '.bind(claim.lease_owner.as_str())',
    '.bind(lease_deadline)',
    'clock_timestamp()',
    'let admission = self.admission.admit(context, &executable).await?;',
    'admission.require_runtime(&self.runtime_binding)?;',
    'PluginPermission::BackgroundJob(subject.as_str().to_owned())',
  ])
  if (/\bpub\s+fn\s+new_bound\s*\(/.test(background)) {
    failures.push(finding(PATHS.backgroundExecution, 'plugin caller can replace the host-owned background reauthorization policy'))
  }
  if (/\bpub\s+fn\s+new\s*\(pool:\s*sqlx::PgPool,\s*policies:\s*PluginAdmissionPolicies\)/s.test(background)) {
    failures.push(finding(PATHS.backgroundExecution, 'plugin caller can replace the host-owned background reauthorization policy'))
  }

  // P6 must respect the frozen SP-06/SP-08 four-handle ApplicationServices aggregate.
  const services = files[PATHS.services]
  for (const forbidden of [
    'PluginRuntimeServices',
    'PluginHostOperations',
    'plugin_runtime',
    'with_plugin_runtime',
    'plugin_host_operations',
    'with_plugin_host_operations',
  ]) {
    if (services.includes(forbidden)) {
      failures.push(finding(PATHS.services, `frozen ApplicationServices absorbed plugin runtime surface: ${forbidden}`))
    }
  }

  const main = files[PATHS.main]
  requireTokens(failures, PATHS.main, main, ['ApplicationServices::production('])
  for (const forbidden of [
    'PluginRuntimeServices::sqlite(',
    'PluginRuntimeServices::postgres(',
    '.with_plugin_runtime(',
    '.with_plugin_host_operations(',
  ]) {
    if (main.includes(forbidden)) {
      failures.push(finding(PATHS.main, `default executable mounted plugin runtime without a reviewed deployment profile: ${forbidden}`))
    }
  }

  const egress = files[PATHS.egress]
  requireTokens(failures, PATHS.egress, egress, [
    'pub struct PluginEgressGrantPolicy',
    'fn resolve(&self, grant_id: &str)',
    'UnknownNetworkGrant',
    'pub struct PluginEgressBudgetRegistry',
    'existing.executable != ExecutableAuthorityKey::from(&admission.executable)',
    'existing.grant != *grant',
    'pub struct PluginSecretNetworkPolicy',
    '.allows(admission, grant_id, purpose)',
    'PluginPermission::NetworkGrant(grant_id.to_owned())',
    'let grant = self.grant_policy.resolve(grant_id)?;',
    'PluginPermission::SecretPurpose(purpose.to_owned())',
    'dispatcher: GovernedEgressDispatcher',
    'GovernedEgressDispatcher::system()',
    'GovernedDispatchError::InvalidGrant',
    'GovernedDispatchError::Transport',
  ])
  if (/derive\([^)]*Default[^)]*\)\s*pub struct PluginEgressBudgetRegistry/.test(egress)) {
    failures.push(finding(PATHS.egress, 'shared plugin egress budget registry regained public Default reset'))
  }
  const egressSend = block(egress, 'pub async fn send(')
  const permissionIndex = egressSend.indexOf('authorize(&PluginPermission::NetworkGrant')
  const resolveIndex = egressSend.indexOf('grant_policy.resolve(grant_id)')
  if (permissionIndex < 0 || resolveIndex < 0 || permissionIndex > resolveIndex) {
    failures.push(finding(PATHS.egress, 'host grant identity is resolved before plugin NetworkGrant authority'))
  }
  const sharedPermitIndex = egressSend.indexOf('let _shared_permit = state.acquire()?;')
  const p5DispatchIndex = egressSend.indexOf('self.dispatcher')
  if (sharedPermitIndex < 0 || p5DispatchIndex < 0 || sharedPermitIndex >= p5DispatchIndex) {
    failures.push(finding(
      PATHS.egress,
      'P6 shared budget admission does not precede P5 governed dispatch',
    ))
  }

  const secret = files[PATHS.secret]
  requireTokens(failures, PATHS.secret, secret, [
    'pub trait PluginSecretBackend',
    'handle: &PluginSecretHandleRef',
    'binding.executable != admission.executable',
    'PluginPermission::SecretPurpose',
    'allowed_operations',
  ])
  for (const forbidden of ['fn get_secret(', 'fn get_key(', 'Secret<Vec<u8>>']) {
    if (secret.includes(forbidden)) failures.push(finding(PATHS.secret, `plugin secret surface exposes raw material: ${forbidden}`))
  }
  requireTokens(failures, PATHS.secretCore, files[PATHS.secretCore], [
    'pub struct PluginSecretHandleRef',
    'revision: u64',
    'pub enum PluginSecretOperationKind',
  ])

  const storage = files[PATHS.storage]
  requireTokens(failures, PATHS.storage, storage, [
    'pub struct PluginStoragePolicy',
    'pub struct SqlitePluginStorageService',
    'pub struct PostgresPluginStorageService',
    'QuotaAuthorityChanged',
    'plugin_storage_owners',
    'plugin_storage_entries',
    'sqlite_require_exact_active_installation',
    'postgres_require_exact_active_installation',
  ])
  const sqlitePut = block(storage, 'pub fn put(')
  const pgSection = storage.indexOf('pub struct PostgresPluginStorageService')
  const pgPut = pgSection >= 0 ? block(storage.slice(pgSection), 'pub async fn put(') : ''
  if (sqlitePut.includes('quota:') || pgPut.includes('quota:')) {
    failures.push(finding(PATHS.storage, 'plugin call surface can self-select storage quota'))
  }

  for (const migrationPath of [PATHS.sqliteMigration, PATHS.postgresMigration]) {
    const migration = files[migrationPath]
    requireTokens(failures, migrationPath, migration, [
      'capability_contract_version',
      'plugin_storage_owners',
      'plugin_storage_entries',
      'plugin_upgrade_transitions',
      'plugin_version',
      'package_digest_sha256',
      'manifest_digest_sha256',
    ])
    if (migration.includes('UNIQUE (tenant_id, plugin_id)')) {
      failures.push(finding(migrationPath, 'plugin installation history collapses to latest tenant/plugin row'))
    }
  }

  requireTokens(failures, PATHS.store, files[PATHS.store], [
    'capability_contract_version',
    'resolve_sqlite_plugin_facts',
    'resolve_postgres_plugin_facts',
    'package_identity.matches_executable(executable)',
  ])

  requireTokens(failures, PATHS.conformance, files[PATHS.conformance], [
    'official.sf-express.fixture',
    'official.wechat-pay.fixture',
    'DangerousPermissionCombinationDenied',
    'PluginHostError::PermissionDenied',
    'NativePublisherNotTrusted',
    'PluginHostError::TenantScopeMismatch',
  ])

  requireTokens(failures, PATHS.story, files[PATHS.story], [
    'PLUGIN_HOST_SECURITY_PASS',
    'FirstPartyNative',
    'SingleFileWebApp',
    'permission diff',
    'preserve_exact_pins',
    'provider-bound plugin execution is fail-closed',
    'runtime binding',
  ])

  for (const [path, source] of Object.entries(rustSources)) {
    if (/\bPluginHost::admit\(/.test(source) && path !== PATHS.host && path !== PATHS.runtime) {
      failures.push(finding(path, 'production code bypasses ProductionPluginHost admission'))
    }
    if (path.includes('/plugin') && source.includes('reqwest::')) {
      failures.push(finding(path, 'plugin code regained direct reqwest authority outside P5 governed egress'))
    }
    if (
      source.includes('plugin_storage_entries')
      && path !== PATHS.storage
      && !path.endsWith('plugin_storage_tests.rs')
      && !path.endsWith('plugin_storage_pg_tests.rs')
    ) {
      failures.push(finding(path, 'plugin-owned storage table accessed outside the bounded storage adapter'))
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

export function loadRepository(root) {
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
  const failures = checkR4PluginHostSecurity(loadRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin host security check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin host security check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
