#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginHostSecurity,
  loadRepository,
} from './check-r4-plugin-host-security.mjs'

const base = loadRepository(process.cwd())
assert.deepEqual(
  checkR4PluginHostSecurity(base),
  [],
  'baseline R4-P6 structural evidence must pass before mutation tests run',
)

function cloneInput() {
  return {
    files: { ...base.files },
    rustSources: { ...base.rustSources },
  }
}

function mutateFile(path, needle, replacement) {
  const input = cloneInput()
  const source = input.files[path]
  assert.equal(typeof source, 'string', `missing fixture path ${path}`)
  assert.ok(source.includes(needle), `mutation needle missing in ${path}: ${needle}`)
  input.files[path] = source.replace(needle, replacement)
  if (Object.hasOwn(input.rustSources, path)) {
    input.rustSources[path] = input.rustSources[path].replace(needle, replacement)
  }
  return input
}

function expectFailure(name, input, evidenceFragment) {
  const failures = checkR4PluginHostSecurity(input)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(evidenceFragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'capability contract pin removal',
  mutateFile(
    'backend/system/core/src/plugin.rs',
    '&& self.capability_contract_version == executable.capability_contract_version',
    '&& true',
  ),
  'capability_contract_version',
)

expectFailure(
  'six-way runtime policy bypass',
  mutateFile(
    'backend/system/core/src/plugin.rs',
    '.intersection(&input.runtime_policy);',
    ';',
  ),
  '.intersection(&input.runtime_policy)',
)

expectFailure(
  'publisher verification downgrade',
  mutateFile(
    'backend/system/core/src/plugin.rs',
    'matches!(self, Self::PublisherVerified { .. })',
    '!matches!(self, Self::Unverified)',
  ),
  'PublisherVerified',
)

expectFailure(
  'reserved background work namespace removal',
  mutateFile(
    'backend/system/core/src/plugin.rs',
    'PluginContractError::ReservedBackgroundJobSubject',
    'PluginContractError::InvalidToken',
  ),
  'reserved BackgroundJob subjects are not rejected',
)

expectFailure(
  'sandbox profile accidental enablement',
  mutateFile(
    'backend/system/core/src/plugin_isolation.rs',
    'enabled_in_r4: false,',
    'enabled_in_r4: true,',
  ),
  'sandbox/remote profiles',
)

for (const [name, needle, replacement] of [
  [
    'capability provide accidentally executable',
    'PluginPermission::CapabilityProvide(_) => None',
    'PluginPermission::CapabilityProvide(_) => Some(PluginPermissionEnforcement::RegistryCapabilityInvoke)',
  ],
  [
    'generic read projection accidentally executable',
    'PluginPermission::DataReadProjection(_) => None',
    'PluginPermission::DataReadProjection(_) => Some(PluginPermissionEnforcement::RegistryCapabilityInvoke)',
  ],
  [
    'generic write projection accidentally executable',
    'PluginPermission::DataWriteProjection(_) => None',
    'PluginPermission::DataWriteProjection(_) => Some(PluginPermissionEnforcement::RegistryCapabilityInvoke)',
  ],
  [
    'unpinned external effect accidentally executable',
    'PluginPermission::ExternalEffect(_) => None',
    'PluginPermission::ExternalEffect(_) => Some(PluginPermissionEnforcement::RegistryCapabilityInvoke)',
  ],
]) {
  expectFailure(
    name,
    mutateFile('backend/system/core/src/plugin_permission_enforcement.rs', needle, replacement),
    needle,
  )
}

expectFailure(
  'native publisher trust removal',
  mutateFile(
    'backend/src/application/plugin_runtime.rs',
    '&& !native_publishers.allows_native_package(package)',
    '&& false',
  ),
  'native_publishers.allows_native_package',
)

expectFailure(
  'background secret network approval removal',
  mutateFile(
    'backend/src/application/plugin_runtime.rs',
    '!dangerous_combinations.allows_effective_permissions(&admission)',
    'false',
  ),
  'dangerous_combinations.allows_effective_permissions',
)

expectFailure(
  'capability delimiter collision protection removal',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    "if value.contains('.') {",
    'if false {',
  ),
  "if value.contains('.')",
)

expectFailure(
  'capability canonicalization protection removal',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    'if canonical.as_str() != value {',
    'if false {',
  ),
  'canonical.as_str() != value',
)

expectFailure(
  'event subscription provider-instance identity removal',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    '        executable\n            .provider_instance_id\n            .as_ref()\n            .map(|value| value.as_str()),',
    '        None::<&str>,',
  ),
  'event consumer identity omits persisted provider instance',
)

expectFailure(
  'host operation runtime binding guard removal',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    '        .require_runtime(runtime_binding)',
    '        .require_runtime(unrelated_runtime)',
  ),
  'host operation guard does not bind the admission to its runtime',
)

expectFailure(
  'event subscription caller-selected consumer identity',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    '        subject: Subject,\n        after_sequence: u64,',
    '        subject: Subject,\n        caller_consumer: ConsumerId,\n        after_sequence: u64,',
  ),
  'foreign ConsumerId',
)

expectFailure(
  'background job executable retargeting',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    'executable: admission.executable().clone()',
    'executable: caller_executable.clone()',
  ),
  'executable: admission.executable().clone()',
)

expectFailure(
  'unpinned external effect facade reintroduced',
  mutateFile(
    'backend/src/application/plugin_host_operations.rs',
    '    pub fn invoke(',
    '    pub fn plan_external_effect(&self) {}\n\n    pub fn invoke(',
  ),
  'unpinned ExternalEffect is exposed',
)

expectFailure(
  'background execution accepts stale claim generation',
  mutateFile(
    'backend/src/application/plugin_background_execution.rs',
    '.bind(generation)',
    '.bind(0_i64)',
  ),
  '.bind(generation)',
)

expectFailure(
  'background execution ignores lease owner fencing',
  mutateFile(
    'backend/src/application/plugin_background_execution.rs',
    '.bind(claim.lease_owner.as_str())',
    '.bind("foreign-worker")',
  ),
  '.bind(claim.lease_owner.as_str())',
)

expectFailure(
  'background execution skips persisted re-admission',
  mutateFile(
    'backend/src/application/plugin_background_execution.rs',
    'let admission = self.admission.admit(context, &executable).await?;',
    'let admission = cached_admission.clone();',
  ),
  'self.admission.admit(context, &executable)',
)

expectFailure(
  'background execution skips exact subject permission',
  mutateFile(
    'backend/src/application/plugin_background_execution.rs',
    'PluginPermission::BackgroundJob(subject.as_str().to_owned())',
    'PluginPermission::BackgroundJob("unscoped".into())',
  ),
  'PluginPermission::BackgroundJob(subject.as_str().to_owned())',
)

expectFailure(
  'production background execution policy constructor becomes caller-owned',
  mutateFile(
    'backend/src/application/plugin_background_execution.rs',
    '    pub(crate) fn new_bound(\n',
    '    pub fn new_bound(\n',
  ),
  'caller can replace the host-owned background reauthorization policy',
)

expectFailure(
  'plugin runtime is absorbed into frozen ApplicationServices',
  mutateFile(
    'backend/src/application/services.rs',
    '    repository_provider: Arc<dyn RepositoryProvider>,\n}',
    '    repository_provider: Arc<dyn RepositoryProvider>,\n    plugin_runtime: Option<Arc<PluginRuntimeServices>>,\n}',
  ),
  'frozen ApplicationServices absorbed plugin runtime surface',
)

expectFailure(
  'partial plugin operations injection is added to frozen ApplicationServices',
  mutateFile(
    'backend/src/application/services.rs',
    '    pub fn order_queries(&self) -> OrderQueryService {',
    '    pub(crate) fn with_plugin_host_operations(self) -> Self { self }\n\n    pub fn order_queries(&self) -> OrderQueryService {',
  ),
  'frozen ApplicationServices absorbed plugin runtime surface',
)

expectFailure(
  'plugin runtime constructor made externally public',
  mutateFile(
    'backend/src/application/plugin_runtime_services.rs',
    'pub(crate) fn postgres(',
    'pub fn postgres(',
  ),
  'plugin runtime composition constructor became externally public',
)

expectFailure(
  'host grant resolution bypass',
  mutateFile(
    'backend/src/application/plugin_egress.rs',
    'let grant = self.grant_policy.resolve(grant_id)?;',
    'let grant = caller_grant.clone();',
  ),
  'grant_policy.resolve(grant_id)',
)

expectFailure(
  'secret-network exact pairing removal',
  mutateFile(
    'backend/src/application/plugin_egress.rs',
    '.allows(admission, grant_id, purpose)',
    '.allows(admission, "unscoped", purpose)',
  ),
  '.allows(admission, grant_id, purpose)',
)

expectFailure(
  'caller-selected full egress grant reintroduction',
  mutateFile(
    'backend/src/application/plugin_execution_services.rs',
    '        grant_id: &str,',
    '        grant: EgressGrant,',
  ),
  'public plugin egress can supply full EgressGrant authority',
)

expectFailure(
  'plaintext decrypt reintroduced on public plugin secret surface',
  mutateFile(
    'backend/src/application/plugin_execution_services.rs',
    '    pub fn sign(',
    '    pub fn decrypt(&self) -> Vec<u8> { vec![] }\n\n    pub fn sign(',
  ),
  'public plugin secret surface can export decrypted plaintext',
)

expectFailure(
  'secret executable pin removal',
  mutateFile(
    'backend/src/application/plugin_secret.rs',
    'binding.executable != admission.executable',
    'false',
  ),
  'binding.executable != admission.executable',
)

expectFailure(
  'caller-selected storage quota reintroduction',
  mutateFile(
    'backend/src/application/plugin_storage.rs',
    '        value: &[u8],\n    ) -> Result<PluginStorageEntry, PluginStorageError> {',
    '        value: &[u8],\n        quota: PluginStorageQuota,\n    ) -> Result<PluginStorageEntry, PluginStorageError> {',
  ),
  'self-select storage quota',
)

expectFailure(
  'latest-only plugin installation schema regression',
  mutateFile(
    'backend/src/db/migrations/068_r4_plugin_host_security.sql',
    '    UNIQUE (\n        tenant_id,\n        plugin_id,\n        plugin_version,\n        package_digest_sha256,\n        manifest_digest_sha256\n    ),',
    '    UNIQUE (tenant_id, plugin_id),',
  ),
  'installation history collapses',
)

{
  const input = cloneInput()
  input.files['backend/src/application/mod.rs'] = input.files['backend/src/application/mod.rs'].replace(
    'mod interconnect_plugin;',
    'pub mod interconnect_plugin;',
  )
  input.rustSources['backend/src/application/mod.rs'] = input.files['backend/src/application/mod.rs']
  expectFailure('raw P3 plugin work module reopened', input, 'raw plugin module became public')
}

{
  const input = cloneInput()
  input.rustSources['backend/src/application/plugin_bypass_fixture.rs'] =
    'fn bypass() { let _ = PluginHost::admit(context, executable, package, installation, policy); }'
  expectFailure('low-level host bypass', input, 'bypasses ProductionPluginHost')
}

{
  const input = cloneInput()
  input.rustSources['backend/src/application/plugin_network_escape_fixture.rs'] =
    'fn escape() { let _ = reqwest::Client::new(); }'
  expectFailure('direct plugin reqwest escape', input, 'direct reqwest authority')
}

console.log('R4-P6 plugin host security mutation suite passed.')
