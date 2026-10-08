#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginLifecycleBoundary,
  loadPluginLifecycleRepository,
} from './check-r4-plugin-lifecycle-boundary.mjs'

const base = loadPluginLifecycleRepository(process.cwd())
assert.deepEqual(
  checkR4PluginLifecycleBoundary(base),
  [],
  'baseline R4-P6 lifecycle/admission evidence must pass before mutations run',
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
  const failures = checkR4PluginLifecycleBoundary(input)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(evidenceFragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'verification proof bypass',
  mutateFile(
    'backend/src/application/plugin_lifecycle_sqlite.rs',
    'verified: VerifiedPluginPackage',
    'verified: PluginPackageRecord',
  ),
  'VerifiedPluginPackage',
)

expectFailure(
  'manifest grant widening',
  mutateFile(
    'backend/src/application/plugin_lifecycle.rs',
    '.any(|permission| !package.permission_request.contains(permission))',
    '.any(|_| false)',
  ),
  'package.permission_request.contains',
)

expectFailure(
  'upgrade review bypass',
  mutateFile(
    'backend/src/application/plugin_lifecycle.rs',
    'authorization.review.matches_transition(previous, candidate)',
    'true',
  ),
  'review.matches_transition',
)

expectFailure(
  'sqlite upgrade validation removal',
  mutateFile(
    'backend/src/application/plugin_lifecycle_sqlite.rs',
    'validate_upgrade(&previous, &candidate, authorization)?;',
    'let _ = (previous, authorization);',
  ),
  'validate_upgrade(&previous, &candidate, authorization)?;',
)

expectFailure(
  'postgres upgrade validation removal',
  mutateFile(
    'backend/src/application/plugin_lifecycle_postgres.rs',
    'validate_upgrade(&previous, &candidate, authorization)?;',
    'let _ = (previous, authorization);',
  ),
  'validate_upgrade(&previous, &candidate, authorization)?;',
)

expectFailure(
  'postgres activation serialization removal',
  mutateFile(
    'backend/src/application/plugin_lifecycle_postgres.rs',
    'pg_advisory_xact_lock(hashtextextended($1, 0))',
    'pg_advisory_unlock(hashtextextended($1, 0))',
  ),
  'pg_advisory_xact_lock',
)

expectFailure(
  'persisted sqlite admission removal',
  mutateFile(
    'backend/src/application/plugin_admission.rs',
    'resolve_sqlite_plugin_facts(',
    'resolve_untrusted_plugin_facts(',
  ),
  'resolve_sqlite_plugin_facts',
)

expectFailure(
  'persisted postgres admission removal',
  mutateFile(
    'backend/src/application/plugin_admission.rs',
    'resolve_postgres_plugin_facts(',
    'resolve_untrusted_plugin_facts(',
  ),
  'resolve_postgres_plugin_facts',
)

expectFailure(
  'principal authority cached in runtime template',
  mutateFile(
    'backend/src/application/plugin_admission.rs',
    'runtime_template.principal_authority = PermissionSet::empty();',
    'let _ = &runtime_template.principal_authority;',
  ),
  'runtime_template.principal_authority = PermissionSet::empty();',
)

expectFailure(
  'per-admission principal resolver bypass',
  mutateFile(
    'backend/src/application/plugin_admission.rs',
    '.resolve(context)',
    '.resolve(&context.clone()) /* mutated */',
  ),
  '.resolve(context)',
)

expectFailure(
  'long-lived admission internals made caller mutable',
  mutateFile(
    'backend/src/application/plugin_admission.rs',
    '    runtime_template: TrustedPluginRuntimePolicy,',
    '    pub runtime_template: TrustedPluginRuntimePolicy,',
  ),
  'caller-mutable',
)

expectFailure(
  'execution context binding removed',
  mutateFile(
    'backend/src/application/plugin_execution_admission.rs',
    '            && &self.correlation_id == context.correlation_id()',
    '            && true',
  ),
  '&& &self.correlation_id == context.correlation_id()',
)

expectFailure(
  'execution admission authority made public',
  mutateFile(
    'backend/src/application/plugin_execution_admission.rs',
    '    authority: PluginAdmission,',
    '    pub authority: PluginAdmission,',
  ),
  'publicly constructible/mutable',
)

expectFailure(
  'context-bound wrapper stopped wrapping raw admission',
  mutateFile(
    'backend/src/application/plugin_execution_admission_service.rs',
    '        Ok(PluginExecutionAdmission::new(\n            authority,\n            context,\n            self.runtime_binding.clone(),\n        ))',
    'unreachable!("raw authority leaked")',
  ),
  'must mint a runtime-bound execution admission',
)

expectFailure(
  'operation facade context check removed',
  mutateFile(
    'backend/src/application/plugin_execution_services.rs',
    '        admission.require_context(context)?;',
    '        let _ = context;',
  ),
  'must reject a transferred execution admission',
)

expectFailure(
  'runtime module made public',
  mutateFile(
    'backend/src/application/mod.rs',
    'mod plugin_runtime;',
    'pub mod plugin_runtime;',
  ),
  'low-level plugin_runtime module is public',
)

expectFailure(
  'plugin store module reopened',
  mutateFile(
    'backend/src/application/mod.rs',
    'mod plugin_store;',
    'pub mod plugin_store;',
  ),
  'low-level plugin_store module is public',
)

expectFailure(
  'raw admission re-exported',
  mutateFile(
    'backend/src/application/mod.rs',
    '    PluginHostError, PluginInstallationLifecycle, PluginInstallationRecord,\n    TrustedPluginRuntimePolicy,',
    '    PluginAdmission, PluginHostError, PluginInstallationLifecycle, PluginInstallationRecord,\n    TrustedPluginRuntimePolicy,',
  ),
  'raw PluginAdmission is publicly re-exported',
)

expectFailure(
  'raw sqlite admission service re-exported',
  mutateFile(
    'backend/src/application/mod.rs',
    'pub use plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionAdmissionError};',
    'pub use plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionAdmissionError};\n#[cfg(feature = "sqlite")]\npub use plugin_admission::SqlitePluginAdmissionService;',
  ),
  'raw persisted admission service is publicly re-exported',
)

expectFailure(
  'low-level production host re-export',
  mutateFile(
    'backend/src/application/mod.rs',
    'DangerousPluginCombinationApproval, ProductionPluginHostError,',
    'DangerousPluginCombinationApproval, ProductionPluginHost, ProductionPluginHostError,',
  ),
  'ProductionPluginHost is publicly re-exported',
)

expectFailure(
  'technical isolation visibility removal',
  mutateFile(
    'backend/src/application/plugin_admission_isolation.rs',
    'r4_plugin_isolation_enforcement(self.isolation().profile)',
    'panic!("no technical isolation evidence")',
  ),
  'r4_plugin_isolation_enforcement',
)

{
  const input = cloneInput()
  input.rustSources['backend/src/application/plugin_direct_host_escape.rs'] = `
fn bypass() {
    let _ = ProductionPluginHost::admit(context, executable, package, installation, runtime, publishers, dangerous);
}
`
  expectFailure('direct production host bypass', input, 'bypasses persistence-bound PluginAdmissionService')
}

console.log('R4-P6 plugin lifecycle/admission mutation suite passed.')
