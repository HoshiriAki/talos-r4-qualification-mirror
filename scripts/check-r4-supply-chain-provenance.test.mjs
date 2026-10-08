#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectSupplyChainSnapshot,
  validateSupplyChainSnapshot,
} from './check-r4-supply-chain-provenance.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectSupplyChainSnapshot());
  mutate(snapshot);
  const errors = validateSupplyChainSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateSupplyChainSnapshot(collectSupplyChainSnapshot());
assert.deepEqual(baseline, [], `baseline must pass before supply-chain mutations: ${baseline.join('; ')}`);

expectFailure(
  'pnpm manager digest removed',
  (snapshot) => {
    const parsed = JSON.parse(snapshot.packageJson);
    parsed.packageManager = 'pnpm@10.33.4';
    snapshot.packageJson = JSON.stringify(parsed);
  },
  'exact version and sha512',
);

expectFailure(
  'pnpm frozen lock discipline removed',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replaceAll('--frozen-lockfile', '--no-frozen-lockfile');
  },
  'frozen lockfile',
);

expectFailure(
  'exact-head excludes build dependencies from pnpm audit',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'pnpm audit --audit-level high',
      'pnpm audit --prod --audit-level high',
    );
  },
  'complete pnpm dependency graph',
);

expectFailure(
  'tracked config removes the bounded braces exception',
  (snapshot) => {
    const parsed = JSON.parse(snapshot.packageJson);
    parsed.pnpm.auditConfig.ignoreCves = [];
    snapshot.packageJson = JSON.stringify(parsed);
  },
  'package audit exception set must contain only',
);

expectFailure(
  'exact-head broadens the waiver to all unfixable advisories',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'pnpm audit --audit-level high',
      'pnpm audit --audit-level high --ignore-unfixable',
    );
  },
  'must not broadly ignore',
);

expectFailure(
  'tracked config adds a second ignored advisory',
  (snapshot) => {
    const parsed = JSON.parse(snapshot.packageJson);
    parsed.pnpm.auditConfig.ignoreCves.push('CVE-2099-00001');
    snapshot.packageJson = JSON.stringify(parsed);
  },
  'package audit exception set must contain only',
);

expectFailure(
  'exact-head restores mutating CLI ignore',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'pnpm audit --audit-level high',
      'pnpm audit --audit-level high --ignore CVE-2026-93687',
    );
  },
  'version-controlled',
);

expectFailure(
  'braces exception enters frontend runtime dependencies',
  (snapshot) => {
    const parsed = JSON.parse(snapshot.frontendPackageJson);
    parsed.dependencies['eslint-plugin-boundaries'] =
      parsed.devDependencies['eslint-plugin-boundaries'];
    delete parsed.devDependencies['eslint-plugin-boundaries'];
    snapshot.frontendPackageJson = JSON.stringify(parsed);
  },
  'must remain dev-build-only',
);

expectFailure(
  'braces lockfile gains an unapproved consumer',
  (snapshot) => {
    snapshot.pnpmLock +=
      '\n  runtime-braces-consumer@1.0.0:\n    dependencies:\n      braces: 3.0.3\n';
  },
  'reachable only through micromatch',
);

expectFailure(
  'braces exception review window becomes indefinite',
  (snapshot) => {
    snapshot.auditException = snapshot.auditException.replace(
      'review_by: 2026-11-03',
      'review_by: 2027-10-03',
    );
  },
  'review window must not exceed 45 days',
);

expectFailure(
  'braces exception loses advisory identity',
  (snapshot) => {
    snapshot.auditException = snapshot.auditException.replace(
      'github_advisory: GHSA-vfj7-8cjw-p6xm',
      'github_advisory: removed',
    );
  },
  'temporary braces audit exception evidence missing',
);

expectFailure(
  'PostgreSQL qualification service returns to floating tag',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      /postgres:18-alpine@sha256:[0-9a-f]{64}/,
      'postgres:18-alpine',
    );
  },
  'pinned by OCI digest',
);

expectFailure(
  'Cargo workspace check unlocks dependency resolution',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace('cargo check --workspace --locked', 'cargo check --workspace');
  },
  'workspace check must use Cargo.lock',
);

expectFailure(
  'cargo-audit tool version becomes floating',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'cargo install cargo-audit --version 0.22.2 --locked',
      'cargo install cargo-audit',
    );
  },
  'version-pinned and locked',
);

expectFailure(
  'exact-head removes the bounded RustSec exception',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'cargo audit --ignore RUSTSEC-2023-0071',
      'cargo audit',
    );
  },
  'RustSec exception set must contain only RUSTSEC-2023-0071',
);

expectFailure(
  'repository-quality broadens the RustSec exception set',
  (snapshot) => {
    snapshot.repositoryQuality = snapshot.repositoryQuality.replace(
      '--ignore RUSTSEC-2023-0071',
      '--ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2099-0001',
    );
  },
  'RustSec exception set must contain only RUSTSEC-2023-0071',
);

expectFailure(
  'workspace enables SQLx MySQL while the RSA exception is active',
  (snapshot) => {
    snapshot.cargoManifests['backend/Cargo.toml'] =
      snapshot.cargoManifests['backend/Cargo.toml'].replace(
        'sqlite = ["sqlx/sqlite", "sqlx/runtime-tokio"]',
        'sqlite = ["sqlx/sqlite", "sqlx/mysql", "sqlx/runtime-tokio"]',
      );
  },
  'must not enable MySQL SQLx features',
);

expectFailure(
  'workspace adds a direct RSA dependency while the exception is active',
  (snapshot) => {
    snapshot.cargoManifests['backend/Cargo.toml'] += '\nrsa = "0.9.10"\n';
  },
  'direct rsa dependency',
);

expectFailure(
  'Cargo lock adds a second RSA consumer',
  (snapshot) => {
    snapshot.cargoLock +=
      '\n[[package]]\nname = "runtime-rsa-consumer"\nversion = "1.0.0"\ndependencies = [\n "rsa",\n]\n';
  },
  'escaped the approved sqlx-mysql-only lockfile edge',
);

expectFailure(
  'RustSec exception review window becomes indefinite',
  (snapshot) => {
    snapshot.rustsecAuditException = snapshot.rustsecAuditException.replace(
      'review_by: 2026-11-03',
      'review_by: 2027-10-04',
    );
  },
  'review window must not exceed 45 days',
);

expectFailure(
  'RustSec exception loses advisory identity',
  (snapshot) => {
    snapshot.rustsecAuditException = snapshot.rustsecAuditException.replace(
      'advisory: RUSTSEC-2023-0071',
      'advisory: removed',
    );
  },
  'temporary RustSec audit exception evidence missing',
);

for (const [name, field, action, mutableRef] of [
  ['exact-head checkout', 'exactHead', 'actions/checkout', 'v5'],
  ['repository-quality checkout', 'repositoryQuality', 'actions/checkout', 'v4'],
  ['repository-preflight checkout', 'repositoryPreflight', 'actions/checkout', 'v5'],
  ['candidate-maintenance checkout', 'maintenance', 'actions/checkout', 'v5'],
  ['UI Lab checkout', 'uiLab', 'actions/checkout', 'v4'],
]) {
  expectFailure(
    `${name} returns to a mutable tag`,
    (snapshot) => {
      snapshot[field] = snapshot[field].replace(
        new RegExp(`${action.replace('/', '\\/')}@[0-9a-f]{40}`),
        `${action}@${mutableRef}`,
      );
    },
    'immutable 40-character commit SHA',
  );
}

expectFailure(
  'write-capable maintenance rust toolchain becomes mutable',
  (snapshot) => {
    snapshot.maintenance = snapshot.maintenance.replace(
      /dtolnay\/rust-toolchain@[0-9a-f]{40}/,
      'dtolnay/rust-toolchain@stable',
    );
  },
  'immutable 40-character commit SHA',
);

expectFailure(
  'repository-quality drops the P7-D gate',
  (snapshot) => {
    snapshot.repositoryQuality = snapshot.repositoryQuality.replace(
      'node scripts/check-r4-supply-chain-provenance.mjs',
      'echo skipped-supply-chain-gate',
    );
  },
  'repository-quality must execute',
);

expectFailure(
  'repository-preflight drops the P7-D gate',
  (snapshot) => {
    snapshot.repositoryPreflight = snapshot.repositoryPreflight.replace(
      'node scripts/check-r4-supply-chain-provenance.mjs',
      'echo skipped-supply-chain-gate',
    );
  },
  'repository-preflight must execute',
);

expectFailure(
  'exact-head drops the unified P7 aggregate',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replaceAll(
      'pnpm quality:r4-platform-security:test',
      'echo skipped-platform-security-tests',
    );
  },
  'unified P7 mutation/unit aggregate',
);

expectFailure(
  'exact-head drops the unified P7 structural aggregate',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replaceAll(
      'pnpm quality:r4-platform-security\n',
      'echo skipped-platform-security-structural\n',
    );
  },
  'unified P7 structural aggregate',
);

expectFailure(
  'P7 aggregate drops provenance unit tests',
  (snapshot) => {
    const parsed = JSON.parse(snapshot.packageJson);
    parsed.scripts['quality:r4-platform-security:test'] =
      parsed.scripts['quality:r4-platform-security:test'].replace(
        'node scripts/r4-build-provenance.test.mjs && ',
        '',
      );
    snapshot.packageJson = JSON.stringify(parsed);
  },
  'provenance negative/unit tests',
);

expectFailure(
  'frontend artifact removed from provenance',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace('--artifact public', '');
  },
  'built frontend artifact',
);

expectFailure(
  'backend artifact removed from provenance',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace('--artifact backend/target/debug/talos-backend', '');
  },
  'built backend binary',
);

expectFailure(
  'provenance verify step removed',
  (snapshot) => {
    snapshot.exactHead = snapshot.exactHead.replace('r4-build-provenance.mjs verify', 'r4-build-provenance.mjs disabled-verify');
  },
  'must verify generated provenance',
);

expectFailure(
  'provenance stops checking repository escape',
  (snapshot) => {
    snapshot.provenance = snapshot.provenance.replaceAll('escapes repository root', 'outside path accepted');
  },
  'reject repository-root escape',
);

expectFailure(
  'provenance stops verifying artifact hashes',
  (snapshot) => {
    snapshot.provenance = snapshot.provenance.replace(
      "await verifyEntrySet(rootReal, 'artifacts', manifest.artifacts);",
      '// removed artifact verification',
    );
  },
  're-hash built artifacts',
);

expectFailure(
  'public transparency service becomes mandatory semantic',
  (snapshot) => {
    snapshot.provenance = snapshot.provenance.replace(
      "transparencyLog: 'optional-external'",
      "transparencyLog: 'mandatory-public-service'",
    );
  },
  'optional/Profile-specific',
);

console.log('R4-P7 supply-chain/provenance mutation suite PASS');
