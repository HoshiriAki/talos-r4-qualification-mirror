import { createHash } from 'node:crypto'
import { inflateRawSync } from 'node:zlib'
import { readFile, readdir, stat } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const frontendDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rootDir = path.resolve(frontendDir, '..')
const stagingDir = path.join(rootDir, 'artifacts', 'ui-lab')
const MIN_PNG_COUNT = 22
const REQUIRED_FILES = ['snapshot.json', 'diff.json', 'prompt.txt', 'diagnostics.json', 'registry.json', 'build-metadata.json', 'manifest.json']
const REQUIRED_SNAPSHOT_NAMES = [
  'button-dark-zh-desktop.png', 'button-light-en-mobile.png', 'loading-reduced-motion-zh-desktop.png',
  'status-rtl-en-desktop.png', 'button-motion-active-zh-desktop.png', 'button-slot-active-zh-desktop.png',
  'button-inspector-modified-en-desktop.png', 'button-journal-populated-en-desktop.png',
  'button-diff-output-en-desktop.png', 'button-diagnostics-error-en-desktop.png',
]
const PR_EVIDENCE_ENV = [
  'UI_LAB_SOURCE_HEAD_SHA',
  'UI_LAB_SOURCE_HEAD_REF',
  'UI_LAB_BASE_SHA',
  'UI_LAB_TESTED_CHECKOUT_SHA',
  'UI_LAB_TESTED_REF',
]
const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
const EOCD_SIGNATURE = 0x06054b50
const CENTRAL_SIGNATURE = 0x02014b50
const LOCAL_SIGNATURE = 0x04034b50

function fail(message) { throw new Error(`UI Lab ZIP verification failed: ${message}`) }
function sha256(buffer) { return createHash('sha256').update(buffer).digest('hex') }

async function exists(target) {
  try { await stat(target); return true } catch { return false }
}

async function filesUnder(directory, prefix = '') {
  const output = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const relative = path.join(prefix, entry.name)
    const absolute = path.join(directory, entry.name)
    if (entry.isDirectory()) output.push(...await filesUnder(absolute, relative))
    else output.push({ absolute, relative: relative.replaceAll('\\', '/') })
  }
  return output
}

function assertCanonicalPath(name) {
  if (!name || name.includes('\\') || name.startsWith('/') || /^[A-Za-z]:/.test(name) || name.split('/').some((part) => !part || part === '.' || part === '..')) {
    fail(`ZIP entry path is not canonical POSIX relative path: ${JSON.stringify(name)}`)
  }
}

function crc32(bytes) {
  let crc = 0xffffffff
  for (const byte of bytes) {
    crc ^= byte
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1))
  }
  return (crc ^ 0xffffffff) >>> 0
}

function zipEntries(bytes) {
  const minimum = Math.max(0, bytes.length - 65_557)
  let eocd = -1
  for (let offset = bytes.length - 22; offset >= minimum; offset -= 1) {
    if (bytes.readUInt32LE(offset) === EOCD_SIGNATURE) { eocd = offset; break }
  }
  if (eocd < 0) fail('ZIP end-of-central-directory record is missing')
  const count = bytes.readUInt16LE(eocd + 10)
  const centralOffset = bytes.readUInt32LE(eocd + 16)
  let offset = centralOffset
  const entries = []
  const seenNames = new Set()
  for (let index = 0; index < count; index += 1) {
    if (offset + 46 > bytes.length || bytes.readUInt32LE(offset) !== CENTRAL_SIGNATURE) fail(`invalid ZIP central-directory record ${index}`)
    const flags = bytes.readUInt16LE(offset + 8)
    const method = bytes.readUInt16LE(offset + 10)
    const crc = bytes.readUInt32LE(offset + 16)
    const compressedSize = bytes.readUInt32LE(offset + 20)
    const uncompressedSize = bytes.readUInt32LE(offset + 24)
    const nameLength = bytes.readUInt16LE(offset + 28)
    const extraLength = bytes.readUInt16LE(offset + 30)
    const commentLength = bytes.readUInt16LE(offset + 32)
    const localOffset = bytes.readUInt32LE(offset + 42)
    const name = bytes.subarray(offset + 46, offset + 46 + nameLength).toString('utf8')
    if (flags & 0x1) fail(`encrypted ZIP entry is not allowed: ${name}`)
    assertCanonicalPath(name)
    if (seenNames.has(name)) fail(`duplicate ZIP entry is not allowed: ${name}`)
    seenNames.add(name)
    entries.push({ name, method, crc, compressedSize, uncompressedSize, localOffset })
    offset += 46 + nameLength + extraLength + commentLength
  }
  return entries
}

function readZipEntry(bytes, entry) {
  const offset = entry.localOffset
  if (offset + 30 > bytes.length || bytes.readUInt32LE(offset) !== LOCAL_SIGNATURE) fail(`invalid local ZIP record for ${entry.name}`)
  const nameLength = bytes.readUInt16LE(offset + 26)
  const extraLength = bytes.readUInt16LE(offset + 28)
  const start = offset + 30 + nameLength + extraLength
  const compressed = bytes.subarray(start, start + entry.compressedSize)
  const result = entry.method === 0 ? compressed : entry.method === 8 ? inflateRawSync(compressed) : fail(`unsupported ZIP compression method ${entry.method} for ${entry.name}`)
  if (result.byteLength !== entry.uncompressedSize) fail(`ZIP size mismatch for ${entry.name}`)
  if (crc32(result) !== entry.crc) fail(`ZIP CRC mismatch for ${entry.name}`)
  return result
}

function pngDimensions(bytes, name) {
  if (bytes.length < 24 || !bytes.subarray(0, 8).equals(PNG_SIGNATURE) || bytes.toString('ascii', 12, 16) !== 'IHDR') fail(`invalid PNG IHDR for ${name}`)
  const width = bytes.readUInt32BE(16)
  const height = bytes.readUInt32BE(20)
  if (width === 0 || height === 0) fail(`invalid PNG dimensions for ${name}`)
  return { width, height }
}

function assertExactSet(actual, expected, label) {
  if (actual.size !== expected.size) fail(`${label} count mismatch: actual=${actual.size} expected=${expected.size}`)
  for (const item of expected) if (!actual.has(item)) fail(`${label} missing ${item}`)
  for (const item of actual) if (!expected.has(item)) fail(`${label} contains undeclared ${item}`)
}

async function assertStagingMatchesManifest(manifest) {
  const actual = new Set((await filesUnder(stagingDir)).map((file) => file.relative).filter((name) => name !== 'manifest.json'))
  const declared = new Set(manifest.files.map((file) => file.path))
  assertExactSet(actual, declared, 'staging manifest closure')
  for (const file of manifest.files) {
    assertCanonicalPath(file.path)
    const bytes = await readFile(path.join(stagingDir, ...file.path.split('/')))
    if (bytes.byteLength !== file.bytes || sha256(bytes) !== file.sha256) fail(`staging hash mismatch for ${file.path}`)
    if (file.path.toLowerCase().endsWith('.png')) {
      const dimensions = pngDimensions(bytes, file.path)
      if (!file.png || file.png.width !== dimensions.width || file.png.height !== dimensions.height) fail(`staging PNG dimensions mismatch for ${file.path}`)
    }
  }
}

async function assertZipMatchesManifest(manifest, zipPath) {
  const zip = await readFile(zipPath)
  const entries = zipEntries(zip)
  const expected = new Set([...manifest.files.map((file) => file.path), 'manifest.json'])
  if (entries.length !== expected.size) fail(`ZIP entry count mismatch: actual=${entries.length} expected=${expected.size}`)
  const byName = new Map(entries.map((entry) => [entry.name, entry]))
  assertExactSet(new Set(byName.keys()), expected, 'ZIP manifest closure')
  const stagingManifest = await readFile(path.join(stagingDir, 'manifest.json'))
  if (!readZipEntry(zip, byName.get('manifest.json')).equals(stagingManifest)) fail('ZIP manifest.json differs from staged manifest')
  for (const file of manifest.files) {
    const bytes = readZipEntry(zip, byName.get(file.path))
    if (bytes.byteLength !== file.bytes || sha256(bytes) !== file.sha256) fail(`ZIP hash mismatch for ${file.path}`)
    if (file.path.toLowerCase().endsWith('.png')) {
      const dimensions = pngDimensions(bytes, file.path)
      if (!file.png || file.png.width !== dimensions.width || file.png.height !== dimensions.height) fail(`ZIP PNG dimensions mismatch for ${file.path}`)
    }
  }
}

const zipArg = process.argv.slice(2).find((arg) => arg !== '--' && !arg.startsWith('-'))
const zipPath = zipArg || path.join(rootDir, 'artifacts', 'ui-lab.zip')
if (!await exists(zipPath)) fail(`ZIP not found: ${zipPath}`)
if (!await exists(path.join(stagingDir, 'manifest.json'))) fail('staging manifest.json not found')

const manifest = JSON.parse(await readFile(path.join(stagingDir, 'manifest.json'), 'utf8'))
if (manifest.schemaVersion !== 'talos.ui-lab.visual-bundle/2' || !Array.isArray(manifest.files) || !manifest.counts) fail('manifest schema is incomplete')
if (!manifest.sourceHeadSha || !manifest.sourceHeadRef || !manifest.testedCheckoutSha || !manifest.testedRef || !('baseSha' in manifest)) fail('manifest tested-ref evidence is incomplete')
if (manifest.counts.files !== manifest.files.length) fail(`manifest file count ${manifest.counts.files} does not equal manifest entries ${manifest.files.length}`)
const pngPaths = manifest.files.filter((file) => file.path.toLowerCase().endsWith('.png'))
if (manifest.counts.png !== pngPaths.length || manifest.counts.png < MIN_PNG_COUNT) fail(`manifest PNG count is invalid: ${manifest.counts.png}`)
if (!Array.isArray(manifest.required) || REQUIRED_FILES.some((name) => !manifest.required.includes(name))) fail('manifest required-file declaration is incomplete')
for (const required of REQUIRED_FILES) {
  const bytes = await readFile(path.join(stagingDir, required))
  if (bytes.length === 0) fail(`required file is empty: ${required}`)
}
for (const name of REQUIRED_SNAPSHOT_NAMES) if (!pngPaths.some((file) => file.path.endsWith(name))) fail(`required snapshot missing: ${name}`)
const diagnostics = JSON.parse(await readFile(path.join(stagingDir, 'diagnostics.json'), 'utf8'))
if (!Array.isArray(diagnostics.diagnostics) || diagnostics.diagnostics.length === 0) fail('diagnostics.json is an empty placeholder')

const isPullRequest = process.env.GITHUB_EVENT_NAME === 'pull_request'
if (isPullRequest) {
  const missing = PR_EVIDENCE_ENV.filter((name) => !process.env[name])
  if (missing.length) fail(`pull-request evidence environment is incomplete: ${missing.join(', ')}`)
}

const expectedCheckout = process.env.UI_LAB_TESTED_CHECKOUT_SHA || process.env.GITHUB_SHA || process.env.GIT_COMMIT
const expectedRef = process.env.UI_LAB_TESTED_REF || process.env.GITHUB_REF || process.env.GITHUB_REF_NAME || process.env.GIT_BRANCH
const expectedHead = process.env.UI_LAB_SOURCE_HEAD_SHA || process.env.GITHUB_HEAD_SHA || expectedCheckout
const expectedHeadRef = process.env.UI_LAB_SOURCE_HEAD_REF || process.env.GITHUB_HEAD_REF || expectedRef
const expectedBaseValue = process.env.UI_LAB_BASE_SHA ?? process.env.GITHUB_BASE_SHA ?? ''
const expectedBase = expectedBaseValue || null
if (expectedCheckout && manifest.testedCheckoutSha !== expectedCheckout) fail('manifest testedCheckoutSha does not match checkout')
if (expectedRef && manifest.testedRef !== expectedRef) fail('manifest testedRef does not match checkout ref')
if (expectedHead && manifest.sourceHeadSha !== expectedHead) fail('manifest sourceHeadSha does not match PR head')
if (expectedHeadRef && manifest.sourceHeadRef !== expectedHeadRef) fail('manifest sourceHeadRef does not match PR head ref')
if (manifest.baseSha !== expectedBase) fail('manifest baseSha does not match PR base')
if (isPullRequest) {
  if (!/^refs\/pull\/\d+\/merge$/.test(manifest.testedRef)) fail(`pull-request testedRef is not a synthetic merge ref: ${manifest.testedRef}`)
  if (!manifest.baseSha) fail('pull-request manifest baseSha is missing')
  if (manifest.sourceHeadSha === manifest.testedCheckoutSha) fail('pull-request sourceHeadSha must differ from testedCheckoutSha')
  if (manifest.sourceHeadRef === manifest.testedRef) fail('pull-request sourceHeadRef must differ from testedRef')
}

await assertStagingMatchesManifest(manifest)
await assertZipMatchesManifest(manifest, zipPath)
console.log(`Verified UI Lab visual bundle: ${manifest.counts.files} files, ${manifest.counts.png} PNGs, checkout ${manifest.testedCheckoutSha} (${manifest.testedRef}), source ${manifest.sourceHeadSha} (${manifest.sourceHeadRef}), base ${manifest.baseSha ?? 'none'}`)
