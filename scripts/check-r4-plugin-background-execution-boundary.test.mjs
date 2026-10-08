#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginBackgroundExecution,
  loadSource,
} from './check-r4-plugin-background-execution-boundary.mjs'

const base = loadSource(process.cwd())
assert.equal(typeof base, 'string', 'background execution fixture must exist')
assert.deepEqual(
  checkR4PluginBackgroundExecution(base),
  [],
  'baseline R4-P6 background execution evidence must pass before mutations',
)

function expectFailure(name, needle, replacement, evidence) {
  assert.ok(base.includes(needle), `mutation needle missing: ${needle}`)
  const failures = checkR4PluginBackgroundExecution(base.replace(needle, replacement))
  assert.ok(
    failures.some((failure) => failure.evidence.includes(evidence)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure('generation fencing removal', 'w.claim_generation=$3', 'TRUE', 'w.claim_generation=$3')
expectFailure('lease-owner fencing removal', 'w.lease_owner=$4', 'TRUE', 'w.lease_owner=$4')
expectFailure('exact lease deadline binding removal', 'w.lease_deadline_ms=$5', 'TRUE', 'w.lease_deadline_ms=$5')
expectFailure('trusted postgres clock removal', 'clock_timestamp()', 'to_timestamp(0)', 'clock_timestamp()')
expectFailure(
  'caller-selected clock reintroduced',
  '        claim: &WorkClaim,\n    ) -> Result<PluginExecutionAdmission, PluginBackgroundExecutionError> {',
  '        claim: &WorkClaim,\n        now_ms: u64,\n    ) -> Result<PluginExecutionAdmission, PluginBackgroundExecutionError> {',
  'caller-selected clock authority',
)
expectFailure('lease-deadline claim binding removal', '.bind(lease_deadline)', '.bind(0_i64)', '.bind(lease_deadline)')
expectFailure(
  'fresh persisted admission removal',
  'let admission = self.admission.admit(context, &executable).await?;',
  'let admission = cached_admission.clone();',
  'self.admission.admit(context, &executable)',
)
expectFailure(
  'runtime binding check removal',
  'admission.require_runtime(&self.runtime_binding)?;',
  'let _ = &self.runtime_binding;',
  'admission.require_runtime(&self.runtime_binding)?;',
)
expectFailure(
  'exact background subject authorization removal',
  'PluginPermission::BackgroundJob(subject.as_str().to_owned())',
  'PluginPermission::BackgroundJob("unscoped".into())',
  'PluginPermission::BackgroundJob(subject.as_str().to_owned())',
)
expectFailure(
  'bound production constructor becomes public',
  'pub(crate) fn new_bound(',
  'pub fn new_bound(',
  'host-owned bound background reauthorization policy',
)
expectFailure(
  'runtime binding dropped from production constructor',
  '        runtime_binding: PluginExecutionRuntimeBinding,',
  '        _runtime_binding: PluginExecutionRuntimeBinding,',
  'bound background constructor missing runtime binding parameter',
)

console.log('R4-P6 plugin background execution mutation suite passed.')
