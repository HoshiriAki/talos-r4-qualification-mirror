#!/usr/bin/env node

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  buildProvenanceManifest,
  hashProvenancePath,
  verifyProvenanceManifest,
  writeProvenanceManifest,
} from './r4-build-provenance.mjs';

const root = mkdtempSync(path.join(os.tmpdir(), 'talos-r4-provenance-'));

function git(...args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
}

try {
  mkdirSync(path.join(root, 'backend'), { recursive: true });
  mkdirSync(path.join(root, 'dist', 'assets'), { recursive: true });
  writeFileSync(path.join(root, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\n", 'utf8');
  writeFileSync(path.join(root, 'backend', 'Cargo.lock'), 'version = 4\n', 'utf8');
  writeFileSync(path.join(root, 'tracked-source.txt'), 'canonical source\n', 'utf8');
  writeFileSync(path.join(root, 'dist', 'index.html'), '<h1>TALOS</h1>\n', 'utf8');
  writeFileSync(path.join(root, 'dist', 'assets', 'app.js'), 'console.log("talos")\n', 'utf8');

  git('init');
  git('config', 'user.name', 'TALOS Test');
  git('config', 'user.email', 'test@talos.invalid');
  git('add', 'pnpm-lock.yaml', 'backend/Cargo.lock', 'tracked-source.txt');
  git('commit', '-m', 'fixture');
  const sourceSha = git('rev-parse', 'HEAD');
  const sourceTreeSha = git('rev-parse', 'HEAD^{tree}');

  const firstDirectoryHash = await hashProvenancePath(root, 'dist');
  const secondDirectoryHash = await hashProvenancePath(root, 'dist');
  assert.deepEqual(firstDirectoryHash, secondDirectoryHash, 'directory digest must be deterministic');
  assert.equal(firstDirectoryHash.fileCount, 2);

  const manifest = await buildProvenanceManifest({
    root,
    sourceSha,
    sourceTreeSha,
    repository: 'talos/test',
    ref: 'refs/heads/test',
    artifacts: ['dist'],
    locks: ['pnpm-lock.yaml', 'backend/Cargo.lock'],
    toolchain: {
      node: 'test-node',
      pnpm: 'test-pnpm',
      rustc: 'test-rustc',
      cargo: 'test-cargo',
    },
    ci: {
      workflow: 'fixture',
      runId: '1',
      runAttempt: '1',
      job: 'test',
      runnerOs: 'fixture',
      runnerArch: 'fixture',
    },
    generatedAt: '2026-09-12T00:00:00.000Z',
  });

  assert.equal(manifest.source.sha, sourceSha);
  assert.equal(manifest.source.treeSha, sourceTreeSha);
  assert.equal(manifest.transport.sha, sourceSha);
  assert.equal(manifest.transport.treeSha, sourceTreeSha);
  assert.equal(manifest.transport.mode, 'direct');
  assert.equal(manifest.dependencyLocks.length, 2);
  assert.equal(manifest.artifacts.length, 1);
  assert.equal(manifest.profileExtensions.signature, 'optional-external');

  writeProvenanceManifest(root, '.talos-provenance/manifest.json', manifest);
  const verified = await verifyProvenanceManifest({
    root,
    manifestPath: '.talos-provenance/manifest.json',
    expectedSourceSha: sourceSha,
    expectedSourceTreeSha: sourceTreeSha,
  });
  assert.equal(verified.source.sha, sourceSha);

  const mirrorCommit = execFileSync(
    'git',
    ['commit-tree', sourceTreeSha],
    {
      cwd: root,
      encoding: 'utf8',
      input: 'mirror transport\n',
    },
  ).trim();
  git('reset', '--hard', mirrorCommit);

  const mirrorManifest = await buildProvenanceManifest({
    root,
    sourceSha,
    sourceTreeSha,
    repository: 'talos/private-source',
    ref: 'refs/heads/private-candidate',
    artifacts: ['dist'],
    locks: ['pnpm-lock.yaml', 'backend/Cargo.lock'],
    toolchain: {
      node: 'test-node',
      pnpm: 'test-pnpm',
      rustc: 'test-rustc',
      cargo: 'test-cargo',
    },
    ci: {
      workflow: 'mirror-fixture',
      runId: '2',
      runAttempt: '1',
      job: 'test',
      runnerOs: 'fixture',
      runnerArch: 'fixture',
    },
    generatedAt: '2026-10-08T00:00:00.000Z',
  });
  assert.equal(mirrorManifest.source.sha, sourceSha);
  assert.equal(mirrorManifest.source.treeSha, sourceTreeSha);
  assert.equal(mirrorManifest.transport.sha, mirrorCommit);
  assert.equal(mirrorManifest.transport.treeSha, sourceTreeSha);
  assert.equal(mirrorManifest.transport.mode, 'public-mirror');

  writeProvenanceManifest(root, '.talos-provenance/mirror.json', mirrorManifest);
  const verifiedMirror = await verifyProvenanceManifest({
    root,
    manifestPath: '.talos-provenance/mirror.json',
    expectedSourceSha: sourceSha,
    expectedSourceTreeSha: sourceTreeSha,
  });
  assert.equal(verifiedMirror.transport.sha, mirrorCommit);

  await assert.rejects(
    buildProvenanceManifest({
      root,
      sourceSha,
      sourceTreeSha: '0000000000000000000000000000000000000000',
      repository: 'talos/private-source',
      artifacts: ['dist'],
      locks: ['pnpm-lock.yaml', 'backend/Cargo.lock'],
    }),
    /source tree mismatch/,
  );

  git('reset', '--hard', sourceSha);

  writeFileSync(path.join(root, 'dist', 'assets', 'app.js'), 'tampered\n', 'utf8');
  await assert.rejects(
    verifyProvenanceManifest({
      root,
      manifestPath: '.talos-provenance/manifest.json',
      expectedSourceSha: sourceSha,
    }),
    /artifacts mismatch/,
  );

  writeFileSync(path.join(root, 'dist', 'assets', 'app.js'), 'console.log("talos")\n', 'utf8');
  await verifyProvenanceManifest({
    root,
    manifestPath: '.talos-provenance/manifest.json',
    expectedSourceSha: sourceSha,
    expectedSourceTreeSha: sourceTreeSha,
  });

  writeFileSync(path.join(root, 'tracked-source.txt'), 'dirty tracked source\n', 'utf8');
  await assert.rejects(
    verifyProvenanceManifest({
      root,
      manifestPath: '.talos-provenance/manifest.json',
      expectedSourceSha: sourceSha,
    }),
    /tracked source tree is dirty;[^]*tracked-source\.txt/,
  );
  await assert.rejects(
    buildProvenanceManifest({
      root,
      sourceSha,
      repository: 'talos/test',
      ref: 'refs/heads/test',
      artifacts: ['dist'],
      locks: ['pnpm-lock.yaml', 'backend/Cargo.lock'],
    }),
    /tracked source tree is dirty;[^]*tracked-source\.txt/,
  );
  writeFileSync(path.join(root, 'tracked-source.txt'), 'canonical source\n', 'utf8');

  writeFileSync(path.join(root, 'pnpm-lock.yaml'), "lockfileVersion: 'tampered'\n", 'utf8');
  await assert.rejects(
    verifyProvenanceManifest({
      root,
      manifestPath: '.talos-provenance/manifest.json',
      expectedSourceSha: sourceSha,
    }),
    /dependencyLocks mismatch/,
  );

  writeFileSync(path.join(root, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\n", 'utf8');
  const manifestText = readFileSync(path.join(root, '.talos-provenance', 'manifest.json'), 'utf8');
  writeFileSync(path.join(root, '.talos-provenance', 'manifest.json'), manifestText.replace('test-node', 'tampered-node'), 'utf8');
  await assert.rejects(
    verifyProvenanceManifest({
      root,
      manifestPath: '.talos-provenance/manifest.json',
      expectedSourceSha: sourceSha,
    }),
    /manifest digest mismatch/,
  );

  await assert.rejects(hashProvenancePath(root, '../outside'), /escapes repository root/);

  console.log('R4-P7 build provenance tests PASS');
} finally {
  rmSync(root, { recursive: true, force: true });
}
