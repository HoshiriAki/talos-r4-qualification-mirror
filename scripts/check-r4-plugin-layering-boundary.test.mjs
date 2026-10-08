#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginLayeringBoundary,
  loadRepository,
} from './check-r4-plugin-layering-boundary.mjs'

const base = loadRepository(process.cwd())
assert.deepEqual(
  checkR4PluginLayeringBoundary(base),
  [],
  'baseline plugin layering evidence must pass before mutations run',
)

function mutate(path, needle, replacement) {
  const files = { ...base.files }
  assert.ok(files[path].includes(needle), `mutation needle missing in ${path}: ${needle}`)
  files[path] = files[path].replace(needle, replacement)
  return { files }
}

function expectFailure(name, input, fragment) {
  const failures = checkR4PluginLayeringBoundary(input)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(fragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'publisher provenance was pushed down into the frozen P3 executable identity',
  mutate(
    'backend/system/core/src/transport/interconnect.rs',
    'pub plugin_id: PluginId,',
    'pub plugin_id: PluginId,\n    pub publisher_id: PublisherId,',
  ),
  'must not absorb P6 publisher provenance',
)

expectFailure(
  'historical SQLite P3 migration was rewritten for P6 publisher provenance',
  mutate(
    'backend/src/db/migrations/067_r4_interconnect_fabric.sql',
    'plugin_id TEXT NOT NULL,',
    'plugin_id TEXT NOT NULL,\n    publisher_id TEXT NOT NULL,',
  ),
  'frozen R4-P3 migration 067 must not be rewritten',
)

expectFailure(
  'historical PostgreSQL P3 migration was rewritten for P6 publisher provenance',
  mutate(
    'backend/src/db/migrations/postgres/067_r4_interconnect_fabric.sql',
    'plugin_id TEXT NOT NULL,',
    'plugin_id TEXT NOT NULL,\n    publisher_id TEXT NOT NULL,',
  ),
  'frozen R4-P3 migration 067 must not be rewritten',
)

expectFailure(
  'host registry started freezing complete per-call requests',
  mutate(
    'backend/src/application/plugin_egress.rs',
    'pub struct PluginEgressGrantPolicy {',
    'pub struct HostOwnedPluginEgressRequest;\n\npub struct PluginEgressGrantPolicy {',
  ),
  'must own EgressGrant authority, not freeze the complete per-call ExternalRequest',
)

expectFailure(
  'P6 shared budget admission disappeared before P5 governed dispatch',
  mutate(
    'backend/src/application/plugin_egress.rs',
    'let _shared_permit = state.acquire()?;',
    'let _shared_permit_after_dispatch = state.acquire()?;',
  ),
  'P6 shared budget must be acquired before handing dynamic request data to P5 governed dispatch',
)

expectFailure(
  'public plugin egress stopped accepting dynamic request data',
  mutate(
    'backend/src/application/plugin_execution_services.rs',
    'mut request: ExternalRequest,',
    'request_removed: ExternalResponse,',
  ),
  'mut request: ExternalRequest',
)

expectFailure(
  'public plugin egress regained caller-selected full grant authority',
  mutate(
    'backend/src/application/plugin_execution_services.rs',
    'grant_id: &str,',
    'grant_id: &str,\n        grant: EgressGrant,',
  ),
  'must reference host-owned grant authority by grant_id only',
)

expectFailure(
  'plugin-supplied correlation escaped trusted ExecutionContext binding',
  mutate(
    'backend/src/application/plugin_execution_services.rs',
    'request.correlation_id = context.correlation_id().as_str().to_owned();',
    '// correlation rewrite removed',
  ),
  'request.correlation_id = context.correlation_id().as_str().to_owned();',
)

console.log('R4-P6 plugin layering mutation suite passed.')
