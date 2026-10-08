#!/usr/bin/env node

import assert from 'node:assert/strict'

import {
  loadPolicySources,
  validateCiExecutionPolicy,
} from './check-ci-execution-policy.mjs'

const baseline = await loadPolicySources()
assert.deepEqual(validateCiExecutionPolicy(baseline), [])

const normalizedBaseline = {
  ...baseline,
  preflightSource: baseline.preflightSource.replace(/\r\n?/g, '\n'),
  fullSource: baseline.fullSource.replace(/\r\n?/g, '\n'),
  exactHeadSource: baseline.exactHeadSource.replace(/\r\n?/g, '\n'),
  maintenanceSource: baseline.maintenanceSource.replace(/\r\n?/g, '\n'),
}
assert.deepEqual(validateCiExecutionPolicy(normalizedBaseline), [])

const crlfBaseline = {
  ...normalizedBaseline,
  preflightSource: normalizedBaseline.preflightSource.replaceAll('\n', '\r\n'),
  fullSource: normalizedBaseline.fullSource.replaceAll('\n', '\r\n'),
  exactHeadSource: normalizedBaseline.exactHeadSource.replaceAll('\n', '\r\n'),
  maintenanceSource: normalizedBaseline.maintenanceSource.replaceAll('\n', '\r\n'),
}
assert.deepEqual(validateCiExecutionPolicy(crlfBaseline), [])

const cases = [
  {
    name: 'requires manual preflight dispatch',
    mutate: value => ({
      ...value,
      preflightSource: value.preflightSource.replace('  workflow_dispatch:\n', ''),
    }),
  },
  {
    name: 'rejects automatic pull-request preflight',
    mutate: value => ({
      ...value,
      preflightSource: value.preflightSource + '\npull_request:\n  types: [ready_for_review]\n',
    }),
  },
  {
    name: 'rejects dependency installation in preflight',
    mutate: value => ({
      ...value,
      preflightSource: value.preflightSource + '\n# pnpm install',
    }),
  },
  {
    name: 'rejects full Rust compilation in preflight',
    mutate: value => ({
      ...value,
      preflightSource: value.preflightSource + '\n# cargo test --workspace',
    }),
  },
  {
    name: 'requires manual full-quality dispatch',
    mutate: value => ({
      ...value,
      fullSource: value.fullSource.replace('  workflow_dispatch:\n', ''),
    }),
  },
  {
    name: 'rejects automatic push full quality',
    mutate: value => ({
      ...value,
      fullSource: value.fullSource + '\npush:\n  branches: [prototype]\n',
    }),
  },
  {
    name: 'rejects automatic PR full quality',
    mutate: value => ({
      ...value,
      fullSource: value.fullSource + '\npull_request:\n  types: [ready_for_review]\n',
    }),
  },
  {
    name: 'requires exact-head ready-for-review activation',
    mutate: value => ({
      ...value,
      exactHeadSource: value.exactHeadSource.replace('      - ready_for_review\n', ''),
    }),
  },
  {
    name: 'rejects synchronize-triggered exact-head qualification',
    mutate: value => ({
      ...value,
      exactHeadSource: value.exactHeadSource.replace(
        '      - ready_for_review\n',
        '      - synchronize\n      - ready_for_review\n',
      ),
    }),
  },
  {
    name: 'requires canonical fingerprint baseline projection in mirror qualification',
    mutate: value => ({
      ...value,
      exactHeadSource: value.exactHeadSource.replace(
        '            echo "TALOS_OPS_BASELINE_PROJECTION_FILE=policy/qualification/talos-ops-baseline-projection.json" >> "$GITHUB_ENV"\n',
        '',
      ),
    }),
  },
  {
    name: 'requires mirror changed-path base object',
    mutate: value => ({
      ...value,
      exactHeadSource: value.exactHeadSource.replace(
        '            echo "TALOS_CHANGED_PATH_BASE_OBJECT=$SOURCE_TREE_SHA" >> "$GITHUB_ENV"\n',
        '',
      ),
    }),
  },
  {
    name: 'requires exact-head Rust provenance build step',
    mutate: value => ({
      ...value,
      exactHeadSource: value.exactHeadSource.replace(
        'Rust format, workspace check, tests and provenance binary',
        'Rust checks without provenance build',
      ),
    }),
  },
  {
    name: 'requires Rust proxy warmup before Runtime Doctor',
    mutate: value => ({
      ...value,
      fullSource: value.fullSource.replace(
        /      - name: Warm Windows Rust toolchain for Runtime Doctor\n        shell: cmd\n        run: \|\n          rustc --version\n          cargo --version\n\n/,
        '',
      ),
    }),
  },
  {
    name: 'requires the local preflight command',
    mutate: value => ({
      ...value,
      packageJson: {
        ...value.packageJson,
        scripts: {
          ...value.packageJson.scripts,
          'validate:preflight': '',
        },
      },
    }),
  },
  {
    name: 'keeps legacy Harness out of dependency-free preflight',
    mutate: value => ({
      ...value,
      packageJson: {
        ...value.packageJson,
        scripts: {
          ...value.packageJson.scripts,
          'validate:preflight':
            value.packageJson.scripts['validate:preflight'] + ' && pnpm quality:harness',
        },
      },
    }),
  },
  {
    name: 'requires maintenance to remain Draft-only',
    mutate: value => ({
      ...value,
      maintenanceSource: value.maintenanceSource.replace(
        'github.event.pull_request.draft == true',
        'true',
      ),
    }),
  },
  {
    name: 'requires explicit rustfmt repair label',
    mutate: value => ({
      ...value,
      maintenanceSource: value.maintenanceSource.replace(
        "github.event.label.name == 'ci:rustfmt-repair'",
        'true',
      ),
    }),
  },
  {
    name: 'rejects automatic ready-for-review maintenance',
    mutate: value => ({
      ...value,
      maintenanceSource: value.maintenanceSource.replace(
        '      - labeled\n',
        '      - labeled\n      - ready_for_review\n',
      ),
    }),
  },
  {
    name: 'rejects retired P3 workflow filename',
    mutate: value => ({
      ...value,
      workflowFiles: [...value.workflowFiles, 'r4-p3-rustfmt-once.yml'],
    }),
  },

  {
    name: 'rejects historical P9 rehearsal in automatic exact-head',
    mutate: value => ({
      ...value,
      exactHeadSource:
        value.exactHeadSource +
        '\n# P9-SP02 clean Linux deployment\n# bash scripts/r4-p9-sp02-clean-stack.sh\n',
    }),
  },
  {
    name: 'rejects automatic milestone qualification',
    mutate: value => ({
      ...value,
      milestoneSource:
        value.milestoneSource + '\npull_request:\n  types: [ready_for_review]\n',
    }),
  },
  {
    name: 'requires current-package dispatcher in exact-head',
    mutate: value => ({
      ...value,
      exactHeadSource: value.exactHeadSource.replace('Current package qualification', 'Package job'),
    }),
  },
]

for (const testCase of cases) {
  const errors = validateCiExecutionPolicy(testCase.mutate(normalizedBaseline))
  assert.ok(errors.length > 0, testCase.name + ': expected a policy failure')
}

console.log(
  'CI execution policy fixtures passed: ' +
    cases.length +
    ' negative + CRLF parity under Exact-Head authority',
)
