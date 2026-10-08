#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginProviderBindingBoundary,
  loadRepository,
} from './check-r4-plugin-provider-binding-boundary.mjs'

const base = loadRepository(process.cwd())
assert.deepEqual(
  checkR4PluginProviderBindingBoundary(base),
  [],
  'baseline provider-binding authority evidence must pass before mutations run',
)

function mutate(path, needle, replacement) {
  const files = { ...base.files }
  assert.ok(files[path].includes(needle), `mutation needle missing in ${path}: ${needle}`)
  files[path] = files[path].replace(needle, replacement)
  return { files }
}

function expectFailure(name, input, fragment) {
  const failures = checkR4PluginProviderBindingBoundary(input)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(fragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'provider-bound plugin admission accidentally enabled without authority resolver',
  mutate(
    'backend/src/application/plugin_host.rs',
    'if executable.provider_instance_id.is_some() {',
    'if false {',
  ),
  'provider-bound admission fail-closed condition was disabled',
)

expectFailure(
  'provider binding revision removed from durable P3 executable identity',
  mutate(
    'backend/system/core/src/transport/interconnect.rs',
    'pub binding_revision: Option<BindingRevisionRef>',
    'pub binding_revision_removed: Option<BindingRevisionRef>',
  ),
  'pub binding_revision: Option<BindingRevisionRef>',
)

expectFailure(
  'provider binding revision stopped being persisted with plugin work',
  mutate(
    'backend/src/application/interconnect_plugin.rs',
    '            admission\n                .executable\n                .binding_revision\n                .as_ref()\n                .map(|value| value.as_str()),',
    '            None::<String>,',
  ),
  'insert path does not bind provider binding revision',
)

console.log('R4-P6 plugin provider-binding mutation suite passed.')
