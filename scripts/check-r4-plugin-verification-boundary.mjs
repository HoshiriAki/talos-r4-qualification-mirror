#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P6-PLUGIN-VERIFICATION'

const PATHS = Object.freeze({
  verification: 'backend/src/application/plugin_verification.rs',
  appMod: 'backend/src/application/mod.rs',
  plugin: 'backend/system/core/src/plugin.rs',
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

function isTestOnlyRustSource(path) {
  return path.includes('/tests/') || path.endsWith('_tests.rs')
}

export function checkR4PluginVerificationBoundary({ files, rustSources }) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R4-P6 verification evidence is missing'))
  }
  if (failures.length) return failures

  const verification = productionSource(files[PATHS.verification])
  requireTokens(failures, PATHS.verification, verification, [
    'pub trait PluginPublisherSignatureVerifier',
    'pub struct VerifiedPluginPackage',
    'record: PluginPackageRecord',
    'pub fn record(&self) -> &PluginPackageRecord',
    'pub(crate) fn into_record(self) -> PluginPackageRecord',
    'pub fn verify_publisher_package(',
    'candidate.lifecycle != PluginLifecycle::Staged',
    'VerificationEvidence::Unverified | VerificationEvidence::DigestVerified',
    'PluginPackageVerificationError::CandidateClaimsVerifiedEvidence',
    'let package_digest = sha256_hex(package_bytes);',
    'package_digest != candidate.identity.package_digest_sha256.as_str()',
    'let manifest_digest = sha256_hex(manifest_bytes);',
    'manifest_digest != candidate.identity.manifest_digest_sha256.as_str()',
    'publisher_verification_statement(candidate)?',
    'SIGNED_STATEMENT_DOMAIN',
    'candidate.identity.capability_contract_version.as_str()',
    'candidate.compatibility_range.as_str()',
    'b"declared_capabilities"',
    'b"permission_request"',
    'fn canonical_permission(',
    'permissions.sort_unstable()',
    'record.verification = VerificationEvidence::PublisherVerified',
    'signature_digest_sha256: Sha256Digest::new(sha256_hex(signature))',
    'Ok(VerifiedPluginPackage { record })',
  ])
  if (verification.includes('serde_json::to_vec(permission)')) {
    failures.push(finding(PATHS.verification, 'signed permission statement depends on JSON serializer details'))
  }
  if (!/matches!\(\s*candidate\.verification,\s*VerificationEvidence::Unverified\s*\|\s*VerificationEvidence::DigestVerified\s*\)/s.test(verification)) {
    failures.push(finding(PATHS.verification, 'candidate verification allowlist admits evidence other than Unverified or DigestVerified'))
  }
  if (/pub\s+fn\s+new\s*\([^)]*PluginPackageRecord/.test(verification)) {
    failures.push(finding(PATHS.verification, 'opaque verification proof gained a caller-constructible public constructor'))
  }

  const appMod = files[PATHS.appMod]
  requireTokens(failures, PATHS.appMod, appMod, [
    'mod plugin_verification;',
    'verify_publisher_package',
    'PluginPackageVerificationError',
    'PluginPublisherSignatureVerifier',
    'VerifiedPluginPackage',
  ])

  requireTokens(failures, PATHS.plugin, files[PATHS.plugin], [
    'pub enum VerificationEvidence',
    'PublisherVerified',
    'signature_digest_sha256',
  ])

  for (const [path, source] of Object.entries(rustSources)) {
    if (path === PATHS.verification || isTestOnlyRustSource(path)) continue
    const production = productionSource(source)
    if (/\.verification\s*=\s*VerificationEvidence::PublisherVerified\b/.test(production)) {
      failures.push(finding(path, 'production code can self-promote plugin provenance outside trusted verifier'))
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

export function loadVerificationRepository(root) {
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
  const failures = checkR4PluginVerificationBoundary(loadVerificationRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P6 plugin verification boundary check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P6 plugin verification boundary check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
