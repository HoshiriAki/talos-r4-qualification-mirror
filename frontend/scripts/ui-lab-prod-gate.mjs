// UI Lab 生产门禁：默认生产构建关闭 /ui-lab；VITE_ENABLE_UI_LAB=true 时开启。
// 用法：node scripts/ui-lab-prod-gate.mjs [both|closed|open]
// 通过标准库 + playwright 驱动，避免污染 vitest/playwright 主配置。
//
// 可靠性设计（Windows 自托管 runner）：
// - 每轮使用独立端口（closed=5198 / open=5197），避免上一轮残留进程占用
//   --strictPort 端口导致 preview 起不来；
// - 停止 preview 时用 taskkill /T /F 杀整棵进程树（pnpm wrapper + vite node
//   孙进程），并在杀完后轮询等待端口释放；
// - 断言失败时输出 page URL、HTML 片段、console 消息、preview exit code 与
//   preview stderr tail，供 CI 远程排错。
import { spawn, spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import net from 'node:net'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const frontendRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const PORTS = { closed: 5198, open: 5197 }
// 直接用 node 启动 vite（绕过 pnpm.cmd / cmd.exe 嵌套），使 child.pid 就是
// vite 进程本身，taskkill /T /F 从根可杀干净、端口必然释放。
const VITE_BIN = resolve(frontendRoot, 'node_modules/vite/bin/vite.js')

function run(command, args, env) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd: frontendRoot,
      env: { ...process.env, ...env },
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: false,
    })
    let output = ''
    let settled = false
    const timeout = setTimeout(() => {
      if (settled) return
      settled = true
      killProcessTree(child)
      rejectPromise(new Error(`command timed out after 120000ms: ${command} ${args.join(' ')}`))
    }, 120_000)
    child.stdout.on('data', (d) => { output += d })
    child.stderr.on('data', (d) => { output += d })
    child.on('error', (error) => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      rejectPromise(error)
    })
    child.on('close', (code) => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      if (code === 0) resolvePromise(output)
      else rejectPromise(new Error(`command failed (${code}): ${command} ${args.join(' ')}\n${output.slice(-2000)}`))
    })
  })
}

async function waitForServer(url, timeoutMs = 60_000) {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(url, { method: 'HEAD', signal: AbortSignal.timeout(2_000) })
      if (res.ok || res.status === 404) return
    } catch { /* retry */ }
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`server did not come up at ${url}`)
}

/** Windows 上可靠终止进程树：taskkill /T 连带子进程，/F 强制。非 Windows 用 SIGTERM。 */
function killProcessTree(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return
  if (process.platform === 'win32') {
    try {
      spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' })
    } catch { /* 进程已退出则忽略 */ }
  } else {
    try { child.kill('SIGTERM') } catch { /* 忽略 */ }
  }
}

/** 轮询等待端口释放：net 连接被拒绝即视为释放。 */
async function waitForPortFree(port, timeoutMs = 15_000) {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    const inUse = await new Promise((resolvePromise) => {
      const socket = net.connect(port, '127.0.0.1')
      socket.once('connect', () => { socket.destroy(); resolvePromise(true) })
      socket.once('error', () => resolvePromise(false))
      socket.setTimeout(800, () => { socket.destroy(); resolvePromise(true) })
    })
    if (!inUse) return
    await new Promise((r) => setTimeout(r, 400))
  }
  console.warn(`warn: port ${port} still in use after ${timeoutMs}ms`)
}

let preview = null
let previewPort = null
let previewOutput = ''
let previewExitCode = null

async function startPreview(port) {
  const viteBin = existsSync(VITE_BIN)
    ? VITE_BIN
    : await (async () => {
        const { createRequire } = await import('node:module')
        const require = createRequire(import.meta.url)
        const pkg = require.resolve('vite/package.json')
        const { dirname: pkgDir, join } = await import('node:path')
        return join(pkgDir(pkg), require(pkg).bin.vite)
      })()
  preview = spawn(process.execPath, [viteBin, 'preview', '--config', 'vite.e2e.config.ts', '--port', String(port), '--strictPort'], {
    cwd: frontendRoot,
    env: { ...process.env },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  previewPort = port
  previewOutput = ''
  previewExitCode = null
  preview.stdout.on('data', (d) => { previewOutput += d })
  preview.stderr.on('data', (d) => { previewOutput += d })
  preview.on('close', (code) => { previewExitCode = code })
  await waitForServer(`http://127.0.0.1:${port}/`)
}

async function stopPreview(port) {
  if (preview) {
    killProcessTree(preview)
    preview = null
    previewPort = null
  }
  await waitForPortFree(port)
}

/** 断言页面状态；失败时输出完整诊断（URL / HTML / console / exit code / preview tail）。 */
async function assertPage(port, expectedOpen) {
  const browser = await chromium.launch({ timeout: 20_000 })
  const consoleMessages = []
  let page
  try {
    page = await browser.newPage()
    page.setDefaultTimeout(20_000)
    page.setDefaultNavigationTimeout(30_000)
    page.on('console', (msg) => consoleMessages.push(`[${msg.type()}] ${msg.text()}`))
    page.on('pageerror', (error) => consoleMessages.push(`[pageerror] ${error}`))
    await page.goto(`http://127.0.0.1:${port}/ui-lab`, { waitUntil: 'networkidle', timeout: 30_000 })
    if (expectedOpen) {
      await page.waitForSelector('.ui-lab-vnext', { timeout: 20_000 })
      console.log(`PROD GATE OK: /ui-lab open when VITE_ENABLE_UI_LAB=true (port ${port})`)
    } else {
      await page.waitForTimeout(500)
      const hasLab = await page.locator('.ui-lab-vnext').count()
      if (hasLab > 0) throw new Error('expected /ui-lab to be CLOSED in default production build, but it rendered')
      console.log(`PROD GATE OK: /ui-lab closed in default production build (NotFound rendered) (port ${port})`)
    }
  } catch (error) {
    const lines = [`PAGE DIAGNOSTICS (port ${port}):`]
    try { lines.push(`url: ${page ? page.url() : '<no page>'}`) } catch { /* 忽略 */ }
    try {
      if (page) {
        const html = await page.content()
        lines.push(`html[0..1200]:\n${html.slice(0, 1200)}`)
      }
    } catch { /* 忽略 */ }
    if (consoleMessages.length) {
      lines.push(`console (first 20):\n${consoleMessages.slice(0, 20).join('\n')}`)
    }
    lines.push(`preview exit code: ${previewExitCode}`)
    if (previewOutput.trim()) lines.push(`preview output (tail):\n${previewOutput.slice(-1200)}`)
    throw new Error(`${error.message}\n${lines.join('\n')}`)
  } finally {
    await browser.close()
  }
}

const mode = process.argv[2] ?? 'both'
try {
  if (mode === 'both' || mode === 'closed') {
    console.log('Building default production bundle (UI Lab disabled)...')
    await run(process.execPath, [VITE_BIN, 'build', '--config', 'vite.e2e.config.ts'], {})
    await startPreview(PORTS.closed)
    await assertPage(PORTS.closed, false)
    await stopPreview(PORTS.closed)
  }
  if (mode === 'both' || mode === 'open') {
    console.log('Building UI Lab-enabled production bundle...')
    await run(process.execPath, [VITE_BIN, 'build', '--config', 'vite.e2e.config.ts'], { VITE_ENABLE_UI_LAB: 'true' })
    await startPreview(PORTS.open)
    await assertPage(PORTS.open, true)
    await stopPreview(PORTS.open)
  }
  console.log('PROD GATE PASSED')
  process.exit(0)
} catch (error) {
  if (preview) {
    const port = previewPort
    killProcessTree(preview)
    if (port) await stopPreview(port)
  }
  console.error('PROD GATE FAILED:', error instanceof Error ? error.message : error)
  process.exit(1)
}
