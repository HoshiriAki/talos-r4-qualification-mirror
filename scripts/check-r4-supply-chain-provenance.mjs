#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const read = (root, relativePath) => fs.readFileSync(path.join(root, relativePath), 'utf8');

function readWorkspaceCargoManifests(root) {
  const backendRoot = path.join(root, 'backend');
  const manifests = {};

  const visit = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === 'target' || entry.name === '.git') continue;
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolute);
      } else if (entry.isFile() && entry.name === 'Cargo.toml') {
        const relative = path.relative(root, absolute).split(path.sep).join('/');
        manifests[relative] = fs.readFileSync(absolute, 'utf8');
      }
    }
  };

  visit(backendRoot);
  return manifests;
}

export function collectSupplyChainSnapshot(root = ROOT) {
  return {
    packageJson: read(root, 'package.json'),
    frontendPackageJson: read(root, 'frontend/package.json'),
    pnpmLock: read(root, 'pnpm-lock.yaml'),
    auditException: read(
      root,
      'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p7d-braces-cve-2026-93687-exception.md',
    ),
    rustsecAuditException: read(
      root,
      'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p7d-rustsec-2023-0071-sqlx-mysql-exception.md',
    ),
    cargoManifests: readWorkspaceCargoManifests(root),
    cargoLock: read(root, 'backend/Cargo.lock'),
    exactHead: read(root, '.github/workflows/exact-head-qualification.yml'),
    repositoryQuality: read(root, '.github/workflows/repository-quality.yml'),
    repositoryPreflight: read(root, '.github/workflows/repository-preflight.yml'),
    maintenance: read(root, '.github/workflows/r4-candidate-maintenance.yml'),
    uiLab: read(root, '.github/workflows/ui-lab-export.yml'),
    provenance: read(root, 'scripts/r4-build-provenance.mjs'),
    provenanceTest: read(root, 'scripts/r4-build-provenance.test.mjs'),
  };
}

function requireMatch(errors, source, pattern, message) {
  if (!pattern.test(source)) errors.push(message);
}

function forbidMatch(errors, source, pattern, message) {
  if (pattern.test(source)) errors.push(message);
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^$()|[\]{}\\]/g, '\\$&');
}
function normalizeLockKey(value) {
  return value.replace(/^['"]|['"]$/g, '');
}

function lockConsumers(lockfile, dependency, versionPrefix) {
  const lines = lockfile.split(/\r?\n/);
  const consumers = [];
  let key = null;
  let body = [];

  const flush = () => {
    if (!key) return;
    const pattern = new RegExp(
      '^\\s+' + escapeRegExp(dependency) + ':\\s+' + escapeRegExp(versionPrefix),
      'm',
    );
    if (pattern.test(body.join('\n'))) consumers.push(normalizeLockKey(key));
  };

  for (const line of lines) {
    const header = line.match(/^  (\S.*):\s*$/);
    if (header) {
      flush();
      key = header[1];
      body = [];
      continue;
    }
    if (key) body.push(line);
  }
  flush();
  return [...new Set(consumers)].sort();
}

function consumersMatch(actual, expectedPatterns) {
  if (actual.length !== expectedPatterns.length) return false;
  return actual.every((value) => expectedPatterns.some((pattern) => pattern.test(value)));
}

function cargoPackages(lockfile) {
  return lockfile
    .split(/\n(?=\[\[package\]\])/)
    .map((block) => {
      const name = block.match(/^name\s*=\s*"([^"]+)"$/m)?.[1];
      const version = block.match(/^version\s*=\s*"([^"]+)"$/m)?.[1];
      if (!name || !version) return null;
      const dependencyBlock = block.match(/dependencies\s*=\s*\[\n([\s\S]*?)\n\]/)?.[1] ?? '';
      const dependencies = [...dependencyBlock.matchAll(/"([^"]+)"/g)]
        .map((match) => match[1].replace(/\s+\d.*$/, ''));
      return { name, version, dependencies };
    })
    .filter(Boolean);
}

function cargoReverseConsumers(packages, dependency) {
  return packages
    .filter((pkg) => pkg.dependencies.includes(dependency))
    .map((pkg) => `${pkg.name}@${pkg.version}`)
    .sort();
}

function rustsecIgnoredAdvisories(workflow) {
  return [...workflow.matchAll(/cargo audit[^\n]*--ignore(?:\s+|=)(RUSTSEC-\d{4}-\d+)/g)]
    .map((match) => match[1])
    .sort();
}

function validatePinnedActions(errors, source, label) {
  const actionPattern = /^\s*uses:\s*([^\s@]+)@([^\s#]+)(?:\s*#.*)?$/gm;
  let match;
  let count = 0;
  while ((match = actionPattern.exec(source)) !== null) {
    count += 1;
    const [, action, ref] = match;
    if (action.startsWith('./')) continue;
    if (!/^[0-9a-f]{40}$/i.test(ref)) {
      errors.push(`${label}: action ${action} must be pinned to an immutable 40-character commit SHA`);
    }
  }
  if (count === 0) errors.push(`${label}: expected at least one GitHub Action reference`);
}

export function validateSupplyChainSnapshot(snapshot) {
  const errors = [];
  let packageJson;
  try {
    packageJson = JSON.parse(snapshot.packageJson);
  } catch {
    errors.push('package.json must remain valid JSON');
    return errors;
  }
  const scripts = packageJson.scripts ?? {};
  let frontendPackageJson;
  try {
    frontendPackageJson = JSON.parse(snapshot.frontendPackageJson);
  } catch {
    errors.push('frontend/package.json must remain valid JSON');
    return errors;
  }

  if (!/^pnpm@\d+\.\d+\.\d+\+sha512\.[0-9a-f]+$/i.test(packageJson.packageManager ?? '')) {
    errors.push('packageManager must pin pnpm by exact version and sha512 digest');
  }

  requireMatch(errors, snapshot.pnpmLock, /^lockfileVersion:\s*['"]?9(?:\.0)?['"]?\s*$/m, 'pnpm lockfile must remain lockfileVersion 9');
  if (/patchedDependencies:/m.test(snapshot.pnpmLock)) {
    requireMatch(errors, snapshot.pnpmLock, /^\s+hash:\s*[0-9a-f]{64}\s*$/mi, 'patched pnpm dependencies must retain content hashes');
  }
  requireMatch(errors, snapshot.cargoLock, /^version\s*=\s*4\s*$/m, 'Cargo.lock must remain version 4');
  requireMatch(errors, snapshot.cargoLock, /^checksum\s*=\s*"[0-9a-f]{64}"\s*$/mi, 'Cargo registry entries must retain checksums');

  for (const [label, workflow] of [
    ['exact-head workflow', snapshot.exactHead],
    ['repository-quality workflow', snapshot.repositoryQuality],
    ['repository-preflight workflow', snapshot.repositoryPreflight],
    ['candidate-maintenance workflow', snapshot.maintenance],
    ['UI Lab artifact workflow', snapshot.uiLab],
  ]) {
    validatePinnedActions(errors, workflow, label);
    forbidMatch(errors, workflow, /uses:\s*[^\s]+@(v\d+|stable|main|master)\b/, `${label} must not trust mutable Action refs`);
  }

  requireMatch(errors, snapshot.exactHead, /pnpm install --frozen-lockfile/, 'exact-head must install pnpm dependencies from the frozen lockfile');
  requireMatch(errors, snapshot.repositoryQuality, /pnpm install --frozen-lockfile/, 'repository-quality must install pnpm dependencies from the frozen lockfile');
  requireMatch(errors, snapshot.uiLab, /pnpm install --frozen-lockfile/, 'UI Lab artifact build must install pnpm dependencies from the frozen lockfile');
  requireMatch(errors, snapshot.exactHead, /pnpm audit --audit-level high/, 'exact-head must audit the complete pnpm dependency graph');
  forbidMatch(errors, snapshot.exactHead, /pnpm audit --prod\b/, 'exact-head must not exclude build/dev dependencies from vulnerability audit');
  forbidMatch(
    errors,
    snapshot.exactHead,
    /pnpm audit[^\n]*--ignore(?:\s|=)/,
    'exact-head audit exceptions must be version-controlled, not passed through mutating CLI --ignore',
  );
  forbidMatch(
    errors,
    snapshot.exactHead,
    /pnpm audit[^\n]*--ignore-unfixable/,
    'exact-head must not broadly ignore unfixable pnpm advisories',
  );
  const ignoredCves = packageJson?.pnpm?.auditConfig?.ignoreCves ?? [];
  if (
    !Array.isArray(ignoredCves)
    || ignoredCves.length !== 1
    || ignoredCves[0] !== 'CVE-2026-93687'
  ) {
    errors.push('package audit exception set must contain only CVE-2026-93687');
  }

  for (const token of [
    'status: TEMPORARY_AUDIT_EXCEPTION',
    'owner: R4-P7_SUPPLY_CHAIN',
    'advisory: CVE-2026-93687',
    'github_advisory: GHSA-vfj7-8cjw-p6xm',
    'package: braces@3.0.3',
    'scope: dev-build-only',
    'upstream_issue: https://github.com/micromatch/braces/issues/73',
    'review_condition: upstream patched release or dependency-chain removal',
  ]) {
    if (!snapshot.auditException.includes(token)) {
      errors.push('temporary braces audit exception evidence missing: ' + token);
    }
  }
  const approved = snapshot.auditException.match(/^approved_at:\s*(\d{4}-\d{2}-\d{2})$/m)?.[1];
  const reviewBy = snapshot.auditException.match(/^review_by:\s*(\d{4}-\d{2}-\d{2})$/m)?.[1];
  if (!approved || !reviewBy) {
    errors.push('temporary braces audit exception must define approved_at and review_by');
  } else {
    const approvedMs = Date.parse(approved + 'T00:00:00Z');
    const reviewMs = Date.parse(reviewBy + 'T23:59:59Z');
    const maxWindowMs = 45 * 24 * 60 * 60 * 1000;
    if (!Number.isFinite(approvedMs) || !Number.isFinite(reviewMs) || reviewMs <= approvedMs) {
      errors.push('temporary braces audit exception dates are invalid');
    } else {
      if (reviewMs - approvedMs > maxWindowMs) {
        errors.push('temporary braces audit exception review window must not exceed 45 days');
      }
      if (Date.now() > reviewMs) {
        errors.push('temporary braces audit exception review date has expired');
      }
    }
  }

  const runtimeDependencies = new Set([
    ...Object.keys(packageJson.dependencies ?? {}),
    ...Object.keys(frontendPackageJson.dependencies ?? {}),
  ]);
  for (const dependency of [
    'braces',
    'micromatch',
    'fast-glob',
    '@ts-morph/common',
    'ts-morph',
    '@boundaries/elements',
    'eslint-plugin-boundaries',
  ]) {
    if (runtimeDependencies.has(dependency)) {
      errors.push('temporary braces audit exception must remain dev-build-only: ' + dependency);
    }
  }
  if (!packageJson.devDependencies?.['ts-morph']) {
    errors.push('braces exception proof expects root ts-morph to remain a devDependency');
  }
  if (!frontendPackageJson.devDependencies?.['ts-morph']) {
    errors.push('braces exception proof expects frontend ts-morph to remain a devDependency');
  }
  if (!frontendPackageJson.devDependencies?.['eslint-plugin-boundaries']) {
    errors.push('braces exception proof expects eslint-plugin-boundaries to remain a devDependency');
  }

  const bracesConsumers = lockConsumers(snapshot.pnpmLock, 'braces', '3.0.3');
  if (!consumersMatch(bracesConsumers, [/^micromatch@4\.0\.8$/])) {
    errors.push('braces@3.0.3 must remain reachable only through micromatch@4.0.8');
  }
  const micromatchConsumers = lockConsumers(snapshot.pnpmLock, 'micromatch', '4.0.8');
  if (!consumersMatch(micromatchConsumers, [
    /^@boundaries\/elements@1\.2\.0\(/,
    /^eslint-plugin-boundaries@5\.4\.0\(/,
    /^fast-glob@3\.3\.3$/,
  ])) {
    errors.push('micromatch@4.0.8 consumers escaped the approved dev-build toolchain');
  }
  const fastGlobConsumers = lockConsumers(snapshot.pnpmLock, 'fast-glob', '3.3.3');
  if (!consumersMatch(fastGlobConsumers, [/^@ts-morph\/common@0\.26\.1$/])) {
    errors.push('fast-glob@3.3.3 must remain reachable only through @ts-morph/common');
  }
  const boundariesConsumers = lockConsumers(snapshot.pnpmLock, "'@boundaries/elements'", '1.2.0');
  if (!consumersMatch(boundariesConsumers, [/^eslint-plugin-boundaries@5\.4\.0\(/])) {
    errors.push('@boundaries/elements must remain reachable only through eslint-plugin-boundaries');
  }
  const tsMorphCommonConsumers = lockConsumers(snapshot.pnpmLock, "'@ts-morph/common'", '0.26.1');
  if (!consumersMatch(tsMorphCommonConsumers, [/^ts-morph@25\.0\.1$/])) {
    errors.push('@ts-morph/common must remain reachable only through ts-morph');
  }
  for (const token of [
    'status: TEMPORARY_AUDIT_EXCEPTION',
    'owner: R4-P7_SUPPLY_CHAIN',
    'advisory: RUSTSEC-2023-0071',
    'package: rsa@0.9.10',
    'scope: lockfile-only-unreachable-optional-sqlx-mysql',
    'review_condition: upstream patched release, sqlx lockfile graph change, or MySQL feature activation',
  ]) {
    if (!snapshot.rustsecAuditException.includes(token)) {
      errors.push('temporary RustSec audit exception evidence missing: ' + token);
    }
  }

  const rustsecApproved = snapshot.rustsecAuditException.match(
    /^approved_at:\s*(\d{4}-\d{2}-\d{2})$/m,
  )?.[1];
  const rustsecReviewBy = snapshot.rustsecAuditException.match(
    /^review_by:\s*(\d{4}-\d{2}-\d{2})$/m,
  )?.[1];
  if (!rustsecApproved || !rustsecReviewBy) {
    errors.push('temporary RustSec audit exception must define approved_at and review_by');
  } else {
    const approvedMs = Date.parse(rustsecApproved + 'T00:00:00Z');
    const reviewMs = Date.parse(rustsecReviewBy + 'T23:59:59Z');
    const maxWindowMs = 45 * 24 * 60 * 60 * 1000;
    if (!Number.isFinite(approvedMs) || !Number.isFinite(reviewMs) || reviewMs <= approvedMs) {
      errors.push('temporary RustSec audit exception dates are invalid');
    } else {
      if (reviewMs - approvedMs > maxWindowMs) {
        errors.push('temporary RustSec audit exception review window must not exceed 45 days');
      }
      if (Date.now() > reviewMs) {
        errors.push('temporary RustSec audit exception review date has expired');
      }
    }
  }

  const exactRustsecIgnores = rustsecIgnoredAdvisories(snapshot.exactHead);
  const qualityRustsecIgnores = rustsecIgnoredAdvisories(snapshot.repositoryQuality);
  for (const [label, ignores] of [
    ['exact-head', exactRustsecIgnores],
    ['repository-quality', qualityRustsecIgnores],
  ]) {
    if (ignores.length !== 1 || ignores[0] !== 'RUSTSEC-2023-0071') {
      errors.push(`${label} RustSec exception set must contain only RUSTSEC-2023-0071`);
    }
  }

  for (const [manifestPath, manifest] of Object.entries(snapshot.cargoManifests ?? {})) {
    if (/\bsqlx-mysql\s*=/.test(manifest)) {
      errors.push('RustSec exception invalid: direct sqlx-mysql dependency in ' + manifestPath);
    }
    if (/^\s*rsa\s*=/m.test(manifest)) {
      errors.push('RustSec exception invalid: direct rsa dependency in ' + manifestPath);
    }
    if (/sqlx\/mysql/.test(manifest)
      || /\bsqlx\s*=\s*\{[^}]*\bfeatures\s*=\s*\[[^\]]*["']mysql["']/s.test(manifest)) {
      errors.push('RustSec exception invalid: workspace must not enable MySQL SQLx features in ' + manifestPath);
    }
  }

  const cargo = cargoPackages(snapshot.cargoLock);
  const rsaPackages = cargo.filter((pkg) => pkg.name === 'rsa');
  if (rsaPackages.length !== 1 || rsaPackages[0].version !== '0.9.10') {
    errors.push('RustSec exception expects exactly rsa@0.9.10 in Cargo.lock');
  }
  const sqlxMysqlPackages = cargo.filter((pkg) => pkg.name === 'sqlx-mysql');
  if (sqlxMysqlPackages.length !== 1 || sqlxMysqlPackages[0].version !== '0.8.6') {
    errors.push('RustSec exception expects exactly sqlx-mysql@0.8.6 in Cargo.lock');
  }

  const rsaConsumers = cargoReverseConsumers(cargo, 'rsa');
  if (!consumersMatch(rsaConsumers, [/^sqlx-mysql@0\.8\.6$/])) {
    errors.push('rsa@0.9.10 escaped the approved sqlx-mysql-only lockfile edge');
  }
  const sqlxMysqlConsumers = cargoReverseConsumers(cargo, 'sqlx-mysql');
  if (!consumersMatch(sqlxMysqlConsumers, [
    /^sqlx@0\.8\.6$/,
    /^sqlx-macros-core@0\.8\.6$/,
  ])) {
    errors.push('sqlx-mysql@0.8.6 consumers escaped the approved SQLx optional lockfile shape');
  }

  requireMatch(errors, snapshot.repositoryQuality, /pnpm audit --prod --audit-level high/, 'repository-quality must retain its fast production dependency audit');
  requireMatch(errors, snapshot.exactHead, /image:\s*postgres:18-alpine@sha256:[0-9a-f]{64}/, 'exact-head PostgreSQL service image must be pinned by OCI digest');
  forbidMatch(errors, snapshot.exactHead, /image:\s*postgres:18-alpine\s*$/m, 'exact-head must not use the floating PostgreSQL qualification tag');
  requireMatch(errors, snapshot.exactHead, /cargo check --workspace --locked/, 'exact-head Rust workspace check must use Cargo.lock');
  requireMatch(errors, snapshot.exactHead, /cargo test --workspace --locked --no-fail-fast/, 'exact-head Rust workspace tests must use Cargo.lock');
  requireMatch(errors, snapshot.exactHead, /cargo build --locked --bin talos-backend/, 'exact-head must build the provenance binary from Cargo.lock');
  requireMatch(errors, snapshot.repositoryQuality, /cargo check --workspace --locked/, 'repository-quality Rust workspace check must use Cargo.lock');
  requireMatch(errors, snapshot.repositoryQuality, /cargo test --workspace --locked --no-fail-fast/, 'repository-quality Rust workspace tests must use Cargo.lock');
  requireMatch(errors, snapshot.repositoryPreflight, /cargo metadata --manifest-path backend\/Cargo\.toml --locked --no-deps/, 'repository-preflight must validate Cargo.lock consistency without resolving new dependencies');
  requireMatch(errors, snapshot.exactHead, /cargo install cargo-audit --version 0\.22\.2 --locked/, 'cargo-audit tooling must remain version-pinned and locked');
  requireMatch(errors, snapshot.repositoryQuality, /cargo install cargo-audit --version 0\.22\.2 --locked/, 'repository-quality cargo-audit tooling must remain version-pinned and locked');
  requireMatch(errors, snapshot.exactHead, /cargo audit/, 'exact-head must retain RustSec vulnerability audit');
  requireMatch(errors, snapshot.repositoryQuality, /cargo audit/, 'repository-quality must retain RustSec vulnerability audit');

  requireMatch(errors, snapshot.exactHead, /pnpm quality:r4-platform-security:test/, 'exact-head must execute the unified P7 mutation/unit aggregate');
  requireMatch(errors, snapshot.exactHead, /pnpm quality:r4-platform-security(?:\s|$)/m, 'exact-head must execute the unified P7 structural aggregate');
  const p7TestAggregate = scripts['quality:r4-platform-security:test'] ?? '';
  if (!p7TestAggregate.includes('r4-build-provenance.test.mjs')) {
    errors.push('P7 test aggregate must execute provenance negative/unit tests');
  }
  if (!p7TestAggregate.includes('check-r4-supply-chain-provenance.test.mjs')) {
    errors.push('P7 test aggregate must execute supply-chain mutation tests');
  }
  requireMatch(errors, snapshot.exactHead, /r4-build-provenance\.mjs generate[\s\S]*?--source-sha "\$EXPECTED_HEAD"/, 'exact-head provenance generation must bind the exact qualified SHA');
  requireMatch(errors, snapshot.exactHead, /--artifact public/, 'exact-head provenance must include the built frontend artifact');
  requireMatch(errors, snapshot.exactHead, /--artifact backend\/target\/debug\/talos-backend/, 'exact-head provenance must include the built backend binary');
  requireMatch(errors, snapshot.exactHead, /r4-build-provenance\.mjs verify[\s\S]*?--source-sha "\$EXPECTED_HEAD"/, 'exact-head must verify generated provenance against the exact qualified SHA');
  requireMatch(errors, snapshot.exactHead, /\.talos-provenance\/exact-head\.json/, 'exact-head must persist a bounded provenance manifest path');
  requireMatch(errors, snapshot.exactHead, /actions\/upload-artifact@[0-9a-f]{40}/, 'exact-head provenance evidence must use an immutable upload-artifact action');

  requireMatch(errors, snapshot.repositoryQuality, /check-r4-supply-chain-provenance\.test\.mjs[\s\S]*?check-r4-supply-chain-provenance\.mjs/, 'repository-quality must execute the P7-D structural/mutation gate');
  requireMatch(errors, snapshot.repositoryPreflight, /check-r4-supply-chain-provenance\.test\.mjs[\s\S]*?check-r4-supply-chain-provenance\.mjs/, 'repository-preflight must execute the P7-D structural/mutation gate');

  requireMatch(errors, snapshot.provenance, /talos\.build-provenance\/v1/, 'provenance manifest must retain a versioned TALOS schema');
  requireMatch(errors, snapshot.provenance, /DEFAULT_LOCKS\s*=\s*\['pnpm-lock\.yaml',\s*'backend\/Cargo\.lock'\]/, 'provenance must bind both dependency lockfiles by default');
  requireMatch(errors, snapshot.provenance, /createHash\('sha256'\)/, 'provenance artifact and manifest identity must use SHA-256');
  requireMatch(errors, snapshot.provenance, /source SHA mismatch/, 'provenance generation must fail closed when requested source differs from HEAD');
  requireMatch(errors, snapshot.provenance, /escapes repository root/, 'provenance inputs must reject repository-root escape');
  requireMatch(errors, snapshot.provenance, /symlink is not accepted as provenance input/, 'provenance hashing must reject symlink indirection');
  requireMatch(errors, snapshot.provenance, /verifyManifestSidecar/, 'provenance verification must validate the manifest digest sidecar');
  requireMatch(errors, snapshot.provenance, /verifyEntrySet\(rootReal, 'dependencyLocks'/, 'provenance verify must re-hash dependency locks');
  requireMatch(errors, snapshot.provenance, /verifyEntrySet\(rootReal, 'artifacts'/, 'provenance verify must re-hash built artifacts');
  requireMatch(errors, snapshot.provenance, /signature:\s*'optional-external'/, 'provenance must expose signing as an optional Profile extension');
  requireMatch(errors, snapshot.provenance, /transparencyLog:\s*'optional-external'/, 'provenance must keep transparency-log integration optional/Profile-specific');

  for (const token of [
    'artifacts mismatch',
    'dependencyLocks mismatch',
    'manifest digest mismatch',
    'escapes repository root',
  ]) {
    requireMatch(errors, snapshot.provenanceTest, new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), `provenance tests must cover ${token}`);
  }

  return errors;
}

export function run(root = ROOT) {
  const errors = validateSupplyChainSnapshot(collectSupplyChainSnapshot(root));
  if (errors.length > 0) {
    console.error('R4-P7 supply-chain/provenance boundary FAILED');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return false;
  }
  console.log('R4-P7 supply-chain/provenance boundary PASS');
  return true;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  run();
}
