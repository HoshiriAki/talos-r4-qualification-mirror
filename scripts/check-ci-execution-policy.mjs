#!/usr/bin/env node

import { readFile, readdir } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const scriptPath = fileURLToPath(import.meta.url)
const repositoryRoot = path.resolve(path.dirname(scriptPath), '..')

function requireText(errors, source, token, label) {
  if (!source.includes(token)) errors.push(label + ': missing ' + JSON.stringify(token))
}

function forbidText(errors, source, token, label) {
  if (source.includes(token)) errors.push(label + ': forbidden ' + JSON.stringify(token))
}

function normalizeLineEndings(source) {
  return source.replace(/\r\n?/g, '\n')
}

export async function loadPolicySources(root = repositoryRoot) {
  const workflowDir = path.join(root, '.github/workflows')
  const [
    preflightSource,
    fullSource,
    exactHeadSource,
    milestoneSource,
    maintenanceSource,
    packageSource,
    workflowFiles,
  ] = await Promise.all([
    readFile(path.join(workflowDir, 'repository-preflight.yml'), 'utf8'),
    readFile(path.join(workflowDir, 'repository-quality.yml'), 'utf8'),
    readFile(path.join(workflowDir, 'exact-head-qualification.yml'), 'utf8'),
    readFile(path.join(workflowDir, 'milestone-qualification.yml'), 'utf8'),
    readFile(path.join(workflowDir, 'r4-candidate-maintenance.yml'), 'utf8'),
    readFile(path.join(root, 'package.json'), 'utf8'),
    readdir(workflowDir),
  ])

  return {
    preflightSource,
    fullSource,
    exactHeadSource,
    milestoneSource,
    maintenanceSource,
    workflowFiles,
    packageJson: JSON.parse(packageSource),
  }
}

export function validateCiExecutionPolicy({
  preflightSource,
  fullSource,
  exactHeadSource,
  milestoneSource,
  maintenanceSource,
  workflowFiles = [],
  packageJson,
}) {
  const errors = []
  preflightSource = normalizeLineEndings(preflightSource)
  fullSource = normalizeLineEndings(fullSource)
  exactHeadSource = normalizeLineEndings(exactHeadSource)
  milestoneSource = normalizeLineEndings(milestoneSource)
  maintenanceSource = normalizeLineEndings(maintenanceSource)

  for (const token of [
    'name: TALOS Repository Preflight',
    'workflow_dispatch:',
    'cancel-in-progress: true',
    '- talos-ops',
    '- windows-x64',
    '- prototype',
    'pnpm validate:preflight',
    'cargo metadata --manifest-path backend/Cargo.toml --locked --no-deps',
    'git diff --check origin/prototype...HEAD',
  ]) {
    requireText(errors, preflightSource, token, 'preflight workflow')
  }

  for (const token of [
    'pull_request:',
    'push:',
    'pull_request_target',
    '- opened',
    '- reopened',
    '- synchronize',
    '- ready_for_review',
    '- labeled',
    'pnpm install',
    'cargo check',
    'cargo test',
    'cargo audit',
    'pnpm --dir frontend typecheck',
    'pnpm --dir frontend build',
    'quality:harness',
    'self-hosted',
  ]) {
    forbidText(errors, preflightSource, token, 'preflight workflow')
  }

  for (const token of [
    'name: TALOS Repository Quality',
    'workflow_dispatch:',
    'Frontend, Registry, repository and dependency audit',
    'Warm Windows Rust toolchain for Runtime Doctor',
    'rustc --version',
    'cargo --version',
    'Run runtime doctor through start.cmd',
    'Rust workspace check and tests',
    'PostgreSQL feature check',
    'RustSec audit',
    'Rust workspace, PostgreSQL and RustSec',
    'cargo check --workspace --locked',
    'cargo test --workspace --locked --no-fail-fast',
    'cargo check --workspace --no-default-features --features postgres --locked',
    'cargo audit',
  ]) {
    requireText(errors, fullSource, token, 'full workflow')
  }

  for (const token of [
    'pull_request:',
    'push:',
    'pull_request_target',
    '- opened',
    '- reopened',
    '- synchronize',
    '- ready_for_review',
    '- labeled',
  ]) {
    forbidText(errors, fullSource, token, 'full workflow')
  }

  const rustWarmup = fullSource.indexOf('- name: Warm Windows Rust toolchain for Runtime Doctor')
  const runtimeDoctor = fullSource.indexOf('- name: Run runtime doctor through start.cmd')
  if (rustWarmup < 0 || runtimeDoctor < 0 || rustWarmup >= runtimeDoctor) {
    errors.push('full workflow: Windows Rust toolchain warmup must run before Runtime Doctor')
  }

  for (const token of [
    'name: TALOS Exact-Head Qualification',
    'pull_request:',
    '- ready_for_review',
    'workflow_dispatch:',
    'group: talos-exact-head-${{ github.event.pull_request.number || github.ref }}',
    'cancel-in-progress: true',
    'Validate R4 platform security hardening',
    'Rust format, workspace check, tests and provenance binary',
    'PostgreSQL feature and migration schema',
    'Final exact-head whitespace check',
    "'agent/r4-p*-*'",
    'Current package qualification',
    'TALOS_OPS_STRICT_ZERO_DEBT=1',
    'ci-package-qualification.mjs',
    'ci-run-package-qualification.mjs',
  ]) {
    requireText(errors, exactHeadSource, token, 'exact-head workflow')
  }

  for (const token of [
    '- opened',
    '- reopened',
    '- synchronize',
    'pull_request_target',
    'P9-SP02 clean Linux deployment',
    'r4-p9-sp02-clean-stack.sh',
    'Recheck R4 legacy compatibility closure',
    'Recheck R3 structural boundaries',
  ]) {
    forbidText(errors, exactHeadSource, token, 'exact-head workflow')
  }

  for (const token of [
    'name: TALOS Milestone Qualification',
    'workflow_dispatch:',
    'expected_sha:',
    'qualification_set:',
    'Pin frozen milestone SHA',
    'P9-SP02 clean Linux deployment',
    'P9-SP03 deployed human and machine authentication',
    'P9-SP04 deployed business read/write and audit evidence',
    'P9-SP05 metrics, plugin fixture and network negative path',
    'P9-SP06 restart recovery and clean-environment qualification',
    'P9 historical deployment evidence gate',
    'r4-p9-sp02-clean-stack.sh',
    'r4-p9-sp03-deployed-auth.sh',
    'r4-p9-sp04-business-evidence.sh',
    'r4-p9-sp05-observability-plugin-network.sh',
    'r4-p9-sp06-restart-recovery-clean-shutdown.sh',
  ]) {
    requireText(errors, milestoneSource, token, 'milestone workflow')
  }

  for (const token of ['pull_request:', 'pull_request_target', 'push:']) {
    forbidText(errors, milestoneSource, token, 'milestone workflow')
  }

  for (const token of [
    'name: TALOS / R4 / Candidate Maintenance',
    'pull_request:',
    '- labeled',
    'contents: write',
    'group: talos-r4-candidate-maintenance-${{ github.event.pull_request.number }}',
    "github.event.label.name == 'ci:rustfmt-repair'",
    'github.event.pull_request.draft == true',
    'github.event.pull_request.head.repo.full_name == github.repository',
    "startsWith(github.event.pull_request.head.ref, 'agent/r4-p')",
    'cargo fmt --all',
    "git commit -m 'style(r4): apply rustfmt to candidate'",
  ]) {
    requireText(errors, maintenanceSource, token, 'R4 candidate maintenance workflow')
  }

  for (const token of [
    'ready_for_review',
    'synchronize',
    'workflow_dispatch',
    'pull_request_target',
    'agent/r4-p3-interconnect-fabric',
    'r4-p3-rustfmt-once',
  ]) {
    forbidText(errors, maintenanceSource, token, 'R4 candidate maintenance workflow')
  }

  if (!workflowFiles.includes('milestone-qualification.yml')) {
    errors.push('workflow directory: milestone-qualification.yml is required')
  }

  if (workflowFiles.includes('r4-p3-rustfmt-once.yml')) {
    errors.push('workflow directory: retired stage-specific r4-p3-rustfmt-once.yml must not return')
  }

  const scripts = packageJson?.scripts ?? {}
  for (const name of [
    'quality:ci-policy:test',
    'quality:ci-policy',
    'quality:ci-package:test',
    'quality:ci-package',
    'validate:preflight',
    'validate:frontend',
    'validate:talos-ops',
  ]) {
    if (typeof scripts[name] !== 'string' || scripts[name].trim() === '') {
      errors.push('package.json: missing non-empty script ' + name)
    }
  }

  for (const name of [
    'quality:docs',
    'quality:harness',
    'quality:feature-status',
    'harness:check',
    'harness:feature:migrate',
    'harness:feature:check',
    'harness:gc',
    'harness:eval',
    'harness:report',
  ]) {
    if (Object.hasOwn(scripts, name)) {
      errors.push('package.json: retired documentation/Harness script must not return: ' + name)
    }
  }

  for (const token of ['quality:docs', 'quality:harness', 'quality:feature-status']) {
    if (fullSource.includes(token)) errors.push('full workflow: retired gate must not return: ' + token)
    if (exactHeadSource.includes(token)) errors.push('exact-head workflow: retired gate must not return: ' + token)
  }

  const preflightCommand = scripts['validate:preflight'] ?? ''
  for (const token of [
    'quality:ci-policy:test',
    'quality:ci-policy',
    'quality:ci-package:test',
    'quality:ci-package',
    'quality:layout',
    'quality:talos-ops',
    'ui:registry:check',
  ]) {
    if (!preflightCommand.includes(token)) {
      errors.push('package.json: validate:preflight missing ' + token)
    }
  }
  for (const token of ['pnpm install', 'quality:harness', 'quality:frontend', 'cargo test']) {
    if (preflightCommand.includes(token)) {
      errors.push(
        'package.json: validate:preflight must remain dependency-install-free and non-compiling (' +
          token +
          ')',
      )
    }
  }

  return errors
}

async function main() {
  const errors = validateCiExecutionPolicy(await loadPolicySources())
  if (errors.length > 0) {
    console.error('CI execution policy validation failed:')
    for (const error of errors) console.error('- ' + error)
    process.exit(1)
  }

  console.log(
    'CI execution policy valid: Exact-Head is the sole automatic SP qualifier; historical milestone rehearsal is explicit-dispatch and SHA-pinned.',
  )
}

if (path.resolve(process.argv[1] ?? '') === scriptPath) {
  main()
}
