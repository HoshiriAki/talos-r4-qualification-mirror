import { createHash } from 'node:crypto'
import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const frontendDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rootDir = path.resolve(frontendDir, '..')
const stagingDir = path.join(rootDir, 'artifacts', 'ui-lab')
const playwrightDir = path.join(rootDir, '.playwright-mcp')
const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
const REQUIRED_FILES = ['snapshot.json', 'diff.json', 'prompt.txt', 'diagnostics.json', 'registry.json', 'build-metadata.json', 'manifest.json']
const PR_EVIDENCE_ENV = [
  'UI_LAB_SOURCE_HEAD_SHA',
  'UI_LAB_SOURCE_HEAD_REF',
  'UI_LAB_BASE_SHA',
  'UI_LAB_TESTED_CHECKOUT_SHA',
  'UI_LAB_TESTED_REF',
]

async function exists(target) {
  try { await stat(target); return true } catch { return false }
}

async function filesUnder(directory, prefix = '') {
  if (!await exists(directory)) return []
  const output = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const relative = path.join(prefix, entry.name)
    const absolute = path.join(directory, entry.name)
    if (entry.isDirectory()) output.push(...await filesUnder(absolute, relative))
    else output.push({ absolute, relative: relative.replaceAll('\\', '/') })
  }
  return output
}

async function copyIfPresent(source, destination) {
  if (!await exists(source)) return
  await mkdir(path.dirname(destination), { recursive: true })
  await cp(source, destination, { recursive: true })
}

function pngDimensions(bytes, file) {
  if (bytes.length < 24 || !bytes.subarray(0, 8).equals(PNG_SIGNATURE) || bytes.toString('ascii', 12, 16) !== 'IHDR') {
    throw new Error(`UI Lab visual bundle contains invalid PNG: ${file}`)
  }
  const width = bytes.readUInt32BE(16)
  const height = bytes.readUInt32BE(20)
  if (width === 0 || height === 0) throw new Error(`UI Lab visual bundle PNG has invalid dimensions: ${file}`)
  return { width, height }
}

await rm(stagingDir, { recursive: true, force: true })
await mkdir(stagingDir, { recursive: true })
await copyIfPresent(path.join(playwrightDir, 'report'), path.join(stagingDir, 'report'))
// 只复制本矩阵在 .playwright-mcp 根目录生成的截图（组件前缀命名），排除
// 旧残留 / 报告 / trace / 无关截图，避免 manifest 被无关文件污染。
for (const entry of await filesUnder(playwrightDir)) {
  const name = path.basename(entry.absolute)
  if (entry.relative.toLowerCase().endsWith('.png') && !entry.relative.includes('/')
    && /^(button|status|loading)-/i.test(name)) {
    const destination = path.join(stagingDir, 'playwright', name)
    await mkdir(path.dirname(destination), { recursive: true })
    await cp(entry.absolute, destination)
  }
}

const registryPath = path.join(frontendDir, 'src', 'ui', 'component-registry.json')
const registry = JSON.parse(await readFile(registryPath, 'utf8'))
const runtimeExportPath = path.join(playwrightDir, 'ui-lab-export.json')
if (!await exists(runtimeExportPath)) throw new Error(`UI Lab runtime export is missing: ${runtimeExportPath}`)
const runtimeExport = JSON.parse(await readFile(runtimeExportPath, 'utf8'))
if (!runtimeExport.snapshot || !runtimeExport.diff || typeof runtimeExport.prompt !== 'string') {
  throw new Error('UI Lab runtime export is not a captureExportBundle projection')
}

const isPullRequest = process.env.GITHUB_EVENT_NAME === 'pull_request'
if (isPullRequest) {
  const missing = PR_EVIDENCE_ENV.filter((name) => !process.env[name])
  if (missing.length) throw new Error(`UI Lab PR evidence environment is incomplete: ${missing.join(', ')}`)
}

const testedCheckoutSha = process.env.UI_LAB_TESTED_CHECKOUT_SHA || process.env.GITHUB_SHA || process.env.GIT_COMMIT || 'local'
const testedRef = process.env.UI_LAB_TESTED_REF || process.env.GITHUB_REF || process.env.GITHUB_REF_NAME || process.env.GIT_BRANCH || 'local'
const sourceHeadSha = process.env.UI_LAB_SOURCE_HEAD_SHA || process.env.GITHUB_HEAD_SHA || testedCheckoutSha
const sourceHeadRef = process.env.UI_LAB_SOURCE_HEAD_REF || process.env.GITHUB_HEAD_REF || testedRef
const baseShaValue = process.env.UI_LAB_BASE_SHA ?? process.env.GITHUB_BASE_SHA ?? ''
const baseSha = baseShaValue || null

if (isPullRequest) {
  if (!/^refs\/pull\/\d+\/merge$/.test(testedRef)) {
    throw new Error(`UI Lab pull-request evidence must test a synthetic merge ref, got ${testedRef}`)
  }
  if (!baseSha) throw new Error('UI Lab pull-request evidence is missing the base SHA')
  if (sourceHeadSha === testedCheckoutSha) {
    throw new Error('UI Lab source head SHA must be distinct from the tested synthetic merge checkout')
  }
}

const metadata = {
  schemaVersion: 'talos.ui-lab.visual-bundle/2',
  generatedAt: new Date().toISOString(),
  sourceHeadSha,
  sourceHeadRef,
  testedCheckoutSha,
  testedRef,
  baseSha,
  node: process.version,
  source: 'frontend/scripts/ui-lab-visual-bundle.mjs',
}
await writeFile(path.join(stagingDir, 'build-metadata.json'), JSON.stringify(metadata, null, 2))
await writeFile(path.join(stagingDir, 'registry.json'), JSON.stringify(registry, null, 2))
await writeFile(path.join(stagingDir, 'snapshot.json'), JSON.stringify(runtimeExport.snapshot, null, 2))
await writeFile(path.join(stagingDir, 'diff.json'), JSON.stringify(runtimeExport.diff, null, 2))
await writeFile(path.join(stagingDir, 'prompt.txt'), `${runtimeExport.prompt}\n`)
await writeFile(path.join(stagingDir, 'diagnostics.json'), JSON.stringify({ diagnostics: runtimeExport.snapshot.diagnostics ?? [] }, null, 2))

const stagedFiles = await filesUnder(stagingDir)
const entries = []
for (const file of stagedFiles) {
  const bytes = await readFile(file.absolute)
  const entry = { path: file.relative, bytes: bytes.byteLength, sha256: createHash('sha256').update(bytes).digest('hex') }
  if (file.relative.toLowerCase().endsWith('.png')) entry.png = pngDimensions(bytes, file.relative)
  entries.push(entry)
}
const pngCount = entries.filter((entry) => entry.path.toLowerCase().endsWith('.png')).length
const manifest = {
  ...metadata,
  files: entries,
  counts: { files: entries.length, png: pngCount, reports: entries.filter((entry) => entry.path.startsWith('report/')).length },
  required: REQUIRED_FILES,
}
await writeFile(path.join(stagingDir, 'manifest.json'), JSON.stringify(manifest, null, 2))
console.log(JSON.stringify({ stagingDir, counts: manifest.counts, sourceHeadSha, sourceHeadRef, testedCheckoutSha, testedRef, baseSha }, null, 2))
