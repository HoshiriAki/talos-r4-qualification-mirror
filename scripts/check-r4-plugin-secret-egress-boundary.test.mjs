#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginSecretEgress,
  loadSource,
} from './check-r4-plugin-secret-egress-boundary.mjs'

const base = loadSource(process.cwd())
assert.equal(typeof base, 'string', 'secret-egress implementation must exist')
assert.deepEqual(
  checkR4PluginSecretEgress(base),
  [],
  'baseline secret-egress evidence must pass before mutations run',
)

function expectFailure(name, needle, replacement, fragment) {
  assert.ok(base.includes(needle), `mutation needle missing: ${needle}`)
  const failures = checkR4PluginSecretEgress(base.replace(needle, replacement))
  assert.ok(
    failures.some((failure) => failure.evidence.includes(fragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'cross-runtime egress guard removed',
  'admission.require_runtime(&self.runtime_binding)?;',
  'let _ = &self.runtime_binding;',
  'runtime/context/secret-purpose guards are not ordered before governed dispatch',
)

expectFailure(
  'multi-purpose secret egress ambiguity guard disabled',
  'if secret_purpose_count > 1 {',
  'if false {',
  'multi-purpose secret egress ambiguity guard was disabled',
)

expectFailure(
  'secret capable admission can relabel egress as secretless',
  'if secret_purpose_count == 1 && secret_purpose.is_none() {',
  'if false {',
  'secret-capable egress purpose guard was disabled',
)

expectFailure(
  'secret authority count removed',
  '.filter(|permission| matches!(permission, PluginPermission::SecretPurpose(_)))',
  '.filter(|_| false)',
  'PluginPermission::SecretPurpose',
)

console.log('R4-P6 plugin secret-egress mutation suite passed.')
