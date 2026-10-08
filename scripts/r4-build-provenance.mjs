#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  createReadStream,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

export const PROVENANCE_SCHEMA = 'talos.build-provenance/v1';
const SCRIPT_PATH = fileURLToPath(import.meta.url);
const DEFAULT_ROOT = path.resolve(path.dirname(SCRIPT_PATH), '..');
const DEFAULT_LOCKS = ['pnpm-lock.yaml', 'backend/Cargo.lock'];

function posixPath(value) {
  return value.split(path.sep).join('/');
}

function assertSha(value, label = 'source SHA') {
  if (!/^[0-9a-f]{40}$/i.test(value ?? '')) {
    throw new Error(`${label} must be an exact 40-character Git SHA`);
  }
  return value.toLowerCase();
}

function resolveInsideRoot(root, candidate, label = 'path') {
  const rootReal = realpathSync(root);
  const absolute = path.resolve(rootReal, candidate);
  const relative = path.relative(rootReal, absolute);
  if (relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))) {
    return { absolute, relative: relative || '.' };
  }
  throw new Error(`${label} escapes repository root: ${candidate}`);
}

async function sha256File(absolute) {
  const stat = lstatSync(absolute);
  if (stat.isSymbolicLink()) throw new Error(`symlink is not accepted as provenance input: ${absolute}`);
  if (!stat.isFile()) throw new Error(`provenance file input is not a regular file: ${absolute}`);

  const hash = createHash('sha256');
  await new Promise((resolve, reject) => {
    const stream = createReadStream(absolute);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.on('error', reject);
    stream.on('end', resolve);
  });
  return { sha256: hash.digest('hex'), bytes: stat.size };
}

async function walkDirectory(rootAbsolute, directoryAbsolute, entries) {
  const names = readdirSync(directoryAbsolute, { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name, 'en'));

  for (const entry of names) {
    const absolute = path.join(directoryAbsolute, entry.name);
    const stat = lstatSync(absolute);
    if (stat.isSymbolicLink()) {
      throw new Error(`symlink is not accepted as provenance input: ${absolute}`);
    }
    if (stat.isDirectory()) {
      await walkDirectory(rootAbsolute, absolute, entries);
      continue;
    }
    if (!stat.isFile()) {
      throw new Error(`special file is not accepted as provenance input: ${absolute}`);
    }
    const hashed = await sha256File(absolute);
    entries.push({
      path: posixPath(path.relative(rootAbsolute, absolute)),
      bytes: hashed.bytes,
      sha256: hashed.sha256,
    });
  }
}

export async function hashProvenancePath(root, candidate) {
  const rootReal = realpathSync(root);
  const resolved = resolveInsideRoot(rootReal, candidate, 'provenance input');
  if (!existsSync(resolved.absolute)) throw new Error(`provenance input does not exist: ${candidate}`);
  const stat = lstatSync(resolved.absolute);
  if (stat.isSymbolicLink()) throw new Error(`symlink is not accepted as provenance input: ${candidate}`);

  if (stat.isFile()) {
    const hashed = await sha256File(resolved.absolute);
    return {
      path: posixPath(resolved.relative),
      kind: 'file',
      bytes: hashed.bytes,
      fileCount: 1,
      sha256: hashed.sha256,
    };
  }
  if (!stat.isDirectory()) throw new Error(`unsupported provenance input type: ${candidate}`);

  const entries = [];
  await walkDirectory(resolved.absolute, resolved.absolute, entries);
  const hash = createHash('sha256');
  let bytes = 0;
  for (const entry of entries) {
    bytes += entry.bytes;
    hash.update(JSON.stringify(entry));
    hash.update('\n');
  }
  return {
    path: posixPath(resolved.relative),
    kind: 'directory',
    bytes,
    fileCount: entries.length,
    sha256: hash.digest('hex'),
  };
}

function commandVersion(command, args = ['--version']) {
  try {
    return execFileSync(command, args, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
  } catch {
    return null;
  }
}

function gitHead(root) {
  return execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function gitTree(root) {
  return execFileSync('git', ['rev-parse', 'HEAD^{tree}'], {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function gitCommitTree(root, sha) {
  return execFileSync('git', ['rev-parse', `${sha}^{tree}`], {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function gitFirstParent(root, sha = 'HEAD') {
  return execFileSync('git', ['rev-parse', `${sha}^`], {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function gitIsAncestor(root, ancestor, descendant) {
  try {
    execFileSync('git', ['merge-base', '--is-ancestor', ancestor, descendant], {
      cwd: root,
      stdio: ['ignore', 'ignore', 'ignore'],
    });
    return true;
  } catch {
    return false;
  }
}

function assertTrackedSourceClean(root) {
  const dirty = execFileSync(
    'git',
    ['status', '--porcelain=v1', '--untracked-files=no'],
    {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  ).trim();
  if (dirty) {
    throw new Error(
      `tracked source tree is dirty; provenance source must equal the exact Git commit; changes:\n${dirty}`,
    );
  }
}

function defaultToolchain() {
  return {
    node: process.version,
    pnpm: commandVersion('pnpm'),
    rustc: commandVersion('rustc'),
    cargo: commandVersion('cargo'),
  };
}

function defaultCiMetadata() {
  return {
    workflow: process.env.GITHUB_WORKFLOW || null,
    runId: process.env.GITHUB_RUN_ID || null,
    runAttempt: process.env.GITHUB_RUN_ATTEMPT || null,
    job: process.env.GITHUB_JOB || null,
    runnerOs: process.env.RUNNER_OS || process.platform,
    runnerArch: process.env.RUNNER_ARCH || process.arch,
  };
}

export async function buildProvenanceManifest({
  root = DEFAULT_ROOT,
  sourceSha,
  sourceTreeSha,
  repairId,
  repairOriginMirrorSha,
  repairBaseSha,
  repository = process.env.GITHUB_REPOSITORY || null,
  ref = process.env.GITHUB_REF || null,
  artifacts,
  locks = DEFAULT_LOCKS,
  sboms = [],
  toolchain = defaultToolchain(),
  ci = defaultCiMetadata(),
  generatedAt = new Date().toISOString(),
}) {
  const rootReal = realpathSync(root);
  const exactSourceSha = assertSha(sourceSha ?? gitHead(rootReal));
  const head = assertSha(gitHead(rootReal), 'repository HEAD');
  const headTree = assertSha(gitTree(rootReal), 'repository tree SHA');
  const exactSourceTreeSha = sourceTreeSha
    ? assertSha(sourceTreeSha, 'source tree SHA')
    : headTree;
  const mirrorTransport = head !== exactSourceSha;
  const divergentMirrorTree = mirrorTransport && headTree !== exactSourceTreeSha;
  let repair = null;

  if (mirrorTransport) {
    if (!sourceTreeSha) {
      throw new Error(`source SHA mismatch: manifest=${exactSourceSha} HEAD=${head}; mirror transport requires source tree SHA`);
    }
    if (divergentMirrorTree) {
      if (!repairId || !repairOriginMirrorSha || !repairBaseSha) {
        throw new Error(`source tree mismatch: source=${exactSourceTreeSha} transport=${headTree}; divergent mirror qualification requires repair identity`);
      }
      if (!/^[A-Z0-9._-]{1,64}$/.test(repairId)) throw new Error('repair ID has invalid format');
      const originSha = assertSha(repairOriginMirrorSha, 'repair origin mirror SHA');
      const baseSha = assertSha(repairBaseSha, 'repair base SHA');
      const originTree = assertSha(gitCommitTree(rootReal, originSha), 'repair origin mirror tree SHA');
      const baseTree = assertSha(gitCommitTree(rootReal, baseSha), 'repair base tree SHA');
      if (originTree !== exactSourceTreeSha) {
        throw new Error(`repair origin tree mismatch: origin=${originTree} source=${exactSourceTreeSha}`);
      }
      if (assertSha(gitFirstParent(rootReal, head), 'repair candidate parent SHA') !== baseSha) {
        throw new Error(`repair base mismatch: HEAD parent does not equal ${baseSha}`);
      }
      if (!gitIsAncestor(rootReal, originSha, head)) {
        throw new Error('repair origin is not an ancestor of the mirror candidate');
      }
      repair = {
        id: repairId,
        originTransportSha: originSha,
        originTransportTreeSha: originTree,
        baseTransportSha: baseSha,
        baseTransportTreeSha: baseTree,
        convergence: 'provisional-until-reverse-integrated',
      };
    } else if (repairId || repairOriginMirrorSha || repairBaseSha) {
      throw new Error('repair identity is only valid for a divergent mirror tree');
    }
  }
  assertTrackedSourceClean(rootReal);
  if (!Array.isArray(artifacts) || artifacts.length === 0) {
    throw new Error('at least one --artifact is required');
  }

  const dependencyLocks = [];
  for (const lock of locks) dependencyLocks.push(await hashProvenancePath(rootReal, lock));
  const artifactEntries = [];
  for (const artifact of artifacts) artifactEntries.push(await hashProvenancePath(rootReal, artifact));
  const sbomEntries = [];
  for (const sbom of sboms) sbomEntries.push(await hashProvenancePath(rootReal, sbom));

  return {
    schema: PROVENANCE_SCHEMA,
    generatedAt,
    source: {
      repository,
      sha: exactSourceSha,
      treeSha: exactSourceTreeSha,
      ref,
    },
    transport: {
      repository: process.env.GITHUB_REPOSITORY || repository,
      sha: head,
      treeSha: headTree,
      ref: process.env.GITHUB_REF || ref,
      mode: repair ? 'public-mirror-repair' : (mirrorTransport ? 'public-mirror' : 'direct'),
    },
    ...(repair ? { repair } : {}),
    dependencyLocks,
    artifacts: artifactEntries,
    sboms: sbomEntries,
    toolchain,
    ci,
    profileExtensions: {
      signature: 'optional-external',
      transparencyLog: 'optional-external',
      sbom: sbomEntries.length > 0 ? 'attached' : 'optional-external',
    },
  };
}

function manifestText(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

export function writeProvenanceManifest(root, output, manifest) {
  const rootReal = realpathSync(root);
  const resolved = resolveInsideRoot(rootReal, output, 'provenance output');
  mkdirSync(path.dirname(resolved.absolute), { recursive: true });
  const text = manifestText(manifest);
  writeFileSync(resolved.absolute, text, 'utf8');
  const digest = createHash('sha256').update(text).digest('hex');
  const sidecar = `${resolved.absolute}.sha256`;
  writeFileSync(sidecar, `${digest}  ${path.basename(resolved.absolute)}\n`, 'utf8');
  return {
    manifestPath: resolved.absolute,
    sidecarPath: sidecar,
    sha256: digest,
  };
}

function verifyManifestSidecar(manifestAbsolute) {
  const sidecar = `${manifestAbsolute}.sha256`;
  if (!existsSync(sidecar)) throw new Error(`provenance sidecar missing: ${sidecar}`);
  const expected = readFileSync(sidecar, 'utf8').trim().split(/\s+/)[0];
  if (!/^[0-9a-f]{64}$/i.test(expected)) throw new Error('invalid provenance sidecar digest');
  const actual = createHash('sha256').update(readFileSync(manifestAbsolute)).digest('hex');
  if (actual !== expected.toLowerCase()) {
    throw new Error(`provenance manifest digest mismatch: expected=${expected} actual=${actual}`);
  }
}

async function verifyEntrySet(root, label, entries) {
  if (!Array.isArray(entries)) throw new Error(`${label} must be an array`);
  for (const expected of entries) {
    const actual = await hashProvenancePath(root, expected.path);
    for (const field of ['kind', 'bytes', 'fileCount', 'sha256']) {
      if (actual[field] !== expected[field]) {
        throw new Error(
          `${label} mismatch for ${expected.path}: ${field} expected=${expected[field]} actual=${actual[field]}`,
        );
      }
    }
  }
}

export async function verifyProvenanceManifest({
  root = DEFAULT_ROOT,
  manifestPath,
  expectedSourceSha,
  expectedSourceTreeSha,
}) {
  const rootReal = realpathSync(root);
  const resolved = resolveInsideRoot(rootReal, manifestPath, 'provenance manifest');
  if (!existsSync(resolved.absolute)) throw new Error(`provenance manifest missing: ${manifestPath}`);
  verifyManifestSidecar(resolved.absolute);

  const manifest = JSON.parse(readFileSync(resolved.absolute, 'utf8'));
  if (manifest.schema !== PROVENANCE_SCHEMA) throw new Error(`unsupported provenance schema: ${manifest.schema}`);
  const sourceSha = assertSha(manifest?.source?.sha);
  const expected = expectedSourceSha ? assertSha(expectedSourceSha, 'expected source SHA') : gitHead(rootReal);
  if (sourceSha !== expected.toLowerCase()) {
    throw new Error(`provenance source mismatch: manifest=${sourceSha} expected=${expected}`);
  }

  const head = assertSha(gitHead(rootReal), 'repository HEAD');
  const headTree = assertSha(gitTree(rootReal), 'repository tree SHA');
  const manifestSourceTree = manifest?.source?.treeSha
    ? assertSha(manifest.source.treeSha, 'manifest source tree SHA')
    : null;
  const expectedTree = expectedSourceTreeSha
    ? assertSha(expectedSourceTreeSha, 'expected source tree SHA')
    : manifestSourceTree;

  if (expectedSourceTreeSha && manifestSourceTree !== expectedTree) {
    throw new Error(`provenance source tree mismatch: manifest=${manifestSourceTree} expected=${expectedTree}`);
  }

  if (head !== sourceSha) {
    if (!expectedTree) {
      throw new Error('mirror provenance requires a source tree SHA');
    }
    if (headTree !== expectedTree) {
      if (manifest?.transport?.mode !== 'public-mirror-repair' || !manifest?.repair) {
        throw new Error(`repository tree does not match provenance source tree: ${headTree} != ${expectedTree}`);
      }
      const repairId = manifest.repair.id;
      if (!/^[A-Z0-9._-]{1,64}$/.test(repairId ?? '')) throw new Error('manifest repair ID has invalid format');
      const originSha = assertSha(manifest.repair.originTransportSha, 'manifest repair origin SHA');
      const baseSha = assertSha(manifest.repair.baseTransportSha, 'manifest repair base SHA');
      const originTree = assertSha(gitCommitTree(rootReal, originSha), 'manifest repair origin tree SHA');
      const baseTree = assertSha(gitCommitTree(rootReal, baseSha), 'manifest repair base tree SHA');
      if (originTree !== expectedTree || manifest.repair.originTransportTreeSha !== originTree) {
        throw new Error('manifest repair origin does not bind the authoritative source tree');
      }
      if (manifest.repair.baseTransportTreeSha !== baseTree) {
        throw new Error('manifest repair base tree mismatch');
      }
      if (assertSha(gitFirstParent(rootReal, head), 'repair candidate parent SHA') !== baseSha) {
        throw new Error('manifest repair base does not equal candidate parent');
      }
      if (!gitIsAncestor(rootReal, originSha, head)) {
        throw new Error('manifest repair origin is not an ancestor of the candidate');
      }
    }
  }

  if (manifest.transport) {
    const transportSha = assertSha(manifest.transport.sha, 'transport SHA');
    const transportTree = assertSha(manifest.transport.treeSha, 'transport tree SHA');
    if (transportSha !== head) {
      throw new Error(`repository HEAD does not match provenance transport: ${head} != ${transportSha}`);
    }
    if (transportTree !== headTree) {
      throw new Error(`repository tree does not match provenance transport tree: ${headTree} != ${transportTree}`);
    }
  } else if (head !== sourceSha) {
    throw new Error('mirror provenance requires transport identity');
  }

  await verifyEntrySet(rootReal, 'dependencyLocks', manifest.dependencyLocks);
  await verifyEntrySet(rootReal, 'artifacts', manifest.artifacts);
  await verifyEntrySet(rootReal, 'sboms', manifest.sboms ?? []);
  assertTrackedSourceClean(rootReal);
  return manifest;
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  const values = { artifacts: [], locks: [], sboms: [] };
  for (let index = 0; index < rest.length; index += 1) {
    const token = rest[index];
    const value = rest[index + 1];
    if (!token.startsWith('--') || value === undefined || value.startsWith('--')) {
      throw new Error(`invalid argument sequence near ${token}`);
    }
    index += 1;
    switch (token) {
      case '--artifact': values.artifacts.push(value); break;
      case '--lock': values.locks.push(value); break;
      case '--sbom': values.sboms.push(value); break;
      case '--output': values.output = value; break;
      case '--manifest': values.manifest = value; break;
      case '--source-sha': values.sourceSha = value; break;
      case '--source-tree-sha': values.sourceTreeSha = value; break;
      case '--repair-id': values.repairId = value; break;
      case '--repair-origin-mirror-sha': values.repairOriginMirrorSha = value; break;
      case '--repair-base-sha': values.repairBaseSha = value; break;
      case '--repository': values.repository = value; break;
      case '--ref': values.ref = value; break;
      default: throw new Error(`unknown argument: ${token}`);
    }
  }
  return { command, values };
}

async function main() {
  const { command, values } = parseArgs(process.argv.slice(2));
  if (command === 'generate') {
    if (!values.output) throw new Error('--output is required for generate');
    const manifest = await buildProvenanceManifest({
      root: DEFAULT_ROOT,
      sourceSha: values.sourceSha,
      sourceTreeSha: values.sourceTreeSha,
      repairId: values.repairId,
      repairOriginMirrorSha: values.repairOriginMirrorSha,
      repairBaseSha: values.repairBaseSha,
      repository: values.repository ?? process.env.GITHUB_REPOSITORY ?? null,
      ref: values.ref ?? process.env.GITHUB_REF ?? null,
      artifacts: values.artifacts,
      locks: values.locks.length > 0 ? values.locks : DEFAULT_LOCKS,
      sboms: values.sboms,
    });
    const written = writeProvenanceManifest(DEFAULT_ROOT, values.output, manifest);
    console.log(`TALOS provenance generated: ${posixPath(path.relative(DEFAULT_ROOT, written.manifestPath))}`);
    console.log(`manifest sha256: ${written.sha256}`);
    return;
  }
  if (command === 'verify') {
    if (!values.manifest) throw new Error('--manifest is required for verify');
    await verifyProvenanceManifest({
      root: DEFAULT_ROOT,
      manifestPath: values.manifest,
      expectedSourceSha: values.sourceSha,
      expectedSourceTreeSha: values.sourceTreeSha,
    });
    console.log(`TALOS provenance verified: ${values.manifest}`);
    return;
  }
  throw new Error('usage: r4-build-provenance.mjs <generate|verify> [options]');
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) {
  main().catch((error) => {
    console.error(`TALOS provenance FAILED: ${error.message}`);
    process.exitCode = 1;
  });
}
