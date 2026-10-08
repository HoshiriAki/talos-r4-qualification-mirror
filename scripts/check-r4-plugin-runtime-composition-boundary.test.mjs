#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginRuntimeComposition,
  loadRepository,
} from './check-r4-plugin-runtime-composition-boundary.mjs'

const base = loadRepository(process.cwd())
assert.deepEqual(
  checkR4PluginRuntimeComposition(base),
  [],
  'baseline runtime composition evidence must pass before mutations run',
)

function mutate(path, needle, replacement) {
  const files = { ...base.files }
  assert.ok(files[path].includes(needle), `mutation needle missing in ${path}: ${needle}`)
  files[path] = files[path].replace(needle, replacement)
  return { files }
}

function expectFailure(name, input, fragment) {
  const failures = checkR4PluginRuntimeComposition(input)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(fragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'postgres runtime lane downgrade',
  mutate(
    'backend/src/application/plugin_runtime_services.rs',
    'PluginHostOperations::postgres(',
    'PluginHostOperations::development(',
  ),
  'PluginHostOperations::postgres(',
)

expectFailure(
  'postgres runtime driver detached from persistence pool',
  mutate(
    'backend/src/application/plugin_runtime_services.rs',
    'PostgresDurableDriver::new(pool.clone())',
    'PostgresDurableDriver::new(unrelated_pool)',
  ),
  'PostgresDurableDriver::new(pool.clone())',
)

expectFailure(
  'sqlite runtime binding creation removed',
  mutate(
    'backend/src/application/plugin_runtime_services.rs',
    'let runtime_binding = PluginExecutionRuntimeBinding::new();',
    'let runtime_binding = unrelated_binding.clone();',
  ),
  'sqlite runtime missing required token: let runtime_binding = PluginExecutionRuntimeBinding::new();',
)

expectFailure(
  'postgres runtime binding creation removed',
  mutate(
    'backend/src/application/plugin_runtime_services.rs',
    '    pub(crate) fn postgres(\n        pool: sqlx::PgPool,\n        fabric: Arc<InterconnectFabric>,\n        profile: PluginRuntimeSecurityProfile,\n    ) -> Self {\n        let runtime_binding = PluginExecutionRuntimeBinding::new();',
    '    pub(crate) fn postgres(\n        pool: sqlx::PgPool,\n        fabric: Arc<InterconnectFabric>,\n        profile: PluginRuntimeSecurityProfile,\n    ) -> Self {\n        let runtime_binding = unrelated_binding.clone();',
  ),
  'postgres runtime missing required token: let runtime_binding = PluginExecutionRuntimeBinding::new();',
)

expectFailure(
  'postgres background gate detached from bound constructor',
  mutate(
    'backend/src/application/plugin_runtime_services.rs',
    'PostgresPluginBackgroundExecutionGate::new_bound(',
    'PostgresPluginBackgroundExecutionGate::new(',
  ),
  'PostgresPluginBackgroundExecutionGate::new_bound(',
)

expectFailure(
  'sqlite runtime accepts caller selected operations',
  mutate(
    'backend/src/application/plugin_runtime_services.rs',
    '        fabric: Arc<InterconnectFabric>,\n        profile: PluginRuntimeSecurityProfile,',
    '        operations: Arc<PluginHostOperations>,\n        profile: PluginRuntimeSecurityProfile,',
  ),
  'sqlite runtime missing required token: fabric: Arc<InterconnectFabric>',
)

expectFailure(
  'postgres admission stops stamping runtime identity',
  mutate(
    'backend/src/application/plugin_execution_admission_service.rs',
    'let authority = self.inner.admit(context, executable).await?;\n        Ok(PluginExecutionAdmission::new(\n            authority,\n            context,\n            self.runtime_binding.clone(),',
    'let authority = self.inner.admit(context, executable).await?;\n        Ok(PluginExecutionAdmission::new(\n            authority,\n            context,\n            unrelated_binding.clone(),',
  ),
  'postgres admission missing required token: self.runtime_binding.clone()',
)

expectFailure(
  'partial sqlite admission service becomes public',
  mutate(
    'backend/src/application/plugin_execution_admission_service.rs',
    'pub(crate) struct SqlitePluginAdmissionService',
    'pub struct SqlitePluginAdmissionService',
  ),
  'externally public partial admission surface',
)

expectFailure(
  'partial admission service reexported',
  mutate(
    'backend/src/application/mod.rs',
    'pub use plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionAdmissionError};',
    'pub use plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionAdmissionError};\npub use plugin_execution_admission_service::SqlitePluginAdmissionService;',
  ),
  'partial admission service was publicly re-exported',
)

expectFailure(
  'plugin runtime is absorbed into frozen ApplicationServices',
  mutate(
    'backend/src/application/services.rs',
    '    repository_provider: Arc<dyn RepositoryProvider>,\n}',
    '    repository_provider: Arc<dyn RepositoryProvider>,\n    plugin_runtime: Option<Arc<PluginRuntimeServices>>,\n}',
  ),
  'frozen ApplicationServices absorbed plugin runtime surface: PluginRuntimeServices',
)

expectFailure(
  'partial plugin operations injection is added to frozen ApplicationServices',
  mutate(
    'backend/src/application/services.rs',
    '    pub fn order_queries(&self) -> OrderQueryService {',
    '    pub(crate) fn with_plugin_host_operations(self) -> Self { self }\n\n    pub fn order_queries(&self) -> OrderQueryService {',
  ),
  'frozen ApplicationServices absorbed plugin runtime surface: with_plugin_host_operations',
)

console.log('R4-P6 plugin runtime composition mutation suite passed.')
