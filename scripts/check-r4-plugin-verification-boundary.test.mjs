#!/usr/bin/env node

import assert from 'node:assert/strict'
import process from 'node:process'

import {
  checkR4PluginVerificationBoundary,
  loadVerificationRepository,
} from './check-r4-plugin-verification-boundary.mjs'

const base = loadVerificationRepository(process.cwd())
assert.deepEqual(
  checkR4PluginVerificationBoundary(base),
  [],
  'baseline R4-P6 package verification evidence must pass before mutations run',
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
  const failures = checkR4PluginVerificationBoundary(input)
  assert.ok(
    failures.some((failure) => failure.evidence.includes(evidenceFragment)),
    `${name} was not detected; failures=${JSON.stringify(failures)}`,
  )
}

expectFailure(
  'package digest recomputation removal',
  mutateFile(
    'backend/src/application/plugin_verification.rs',
    'package_digest != candidate.identity.package_digest_sha256.as_str()',
    'false',
  ),
  'package_digest != candidate.identity.package_digest_sha256.as_str()',
)

expectFailure(
  'manifest digest recomputation removal',
  mutateFile(
    'backend/src/application/plugin_verification.rs',
    'manifest_digest != candidate.identity.manifest_digest_sha256.as_str()',
    'false',
  ),
  'manifest_digest != candidate.identity.manifest_digest_sha256.as_str()',
)

expectFailure(
  'self asserted publisher verification acceptance',
  mutateFile(
    'backend/src/application/plugin_verification.rs',
    'VerificationEvidence::Unverified | VerificationEvidence::DigestVerified',
    'VerificationEvidence::Unverified | VerificationEvidence::DigestVerified | VerificationEvidence::PublisherVerified { .. }',
  ),
  'candidate verification allowlist',
)

expectFailure(
  'capability contract omitted from signed statement',
  mutateFile(
    'backend/src/application/plugin_verification.rs',
    '        candidate.identity.capability_contract_version.as_str(),\n',
    '',
  ),
  'candidate.identity.capability_contract_version.as_str()',
)

expectFailure(
  'permission request omitted from signed statement',
  mutateFile(
    'backend/src/application/plugin_verification.rs',
    '    append_field(&mut output, b"permission_request")?;\n',
    '',
  ),
  'b"permission_request"',
)

expectFailure(
  'serializer dependent permission statement regression',
  mutateFile(
    'backend/src/application/plugin_verification.rs',
    '        append_field(&mut output, dimension.as_bytes())?;\n        append_field(&mut output, value.as_bytes())?;',
    '        append_field(&mut output, &serde_json::to_vec(permission).unwrap())?;',
  ),
  'JSON serializer',
)

{
  const input = cloneInput()
  input.rustSources['backend/src/application/plugin_fixture_tests.rs'] = `
fn active_for_review(record: &mut PluginPackageRecord) {
  record.verification = VerificationEvidence::PublisherVerified {
    publisher_key_id: "fixture".into(),
    signature_digest_sha256: digest,
  };
}
`
  const failures = checkR4PluginVerificationBoundary(input)
  assert.ok(
    !failures.some((failure) => failure.evidence.includes('self-promote plugin provenance')),
    `test-only fixture was treated as production; failures=${JSON.stringify(failures)}`,
  )
}

{
  const input = cloneInput()
  input.rustSources['backend/src/application/plugin_fake_promoter.rs'] = `
fn promote(record: &mut PluginPackageRecord) {
  record.verification = VerificationEvidence::PublisherVerified {
    publisher_key_id: "forged".into(),
    signature_digest_sha256: digest,
  };
}
`
  expectFailure('production provenance self-promotion', input, 'self-promote plugin provenance')
}

console.log('R4-P6 plugin verification mutation suite passed.')
