import { execFileSync } from 'node:child_process'
import { appendFileSync } from 'node:fs'
import net from 'node:net'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const DEFAULT_PORT = 5199
const PORT_RANGE = { start: 5200, size: 800 }
const SYSTEM_COMMAND_TIMEOUT_MS = 5_000
const ALLOCATION_TIMEOUT_MS = 15_000
const frontendRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const workspace = (process.env.GITHUB_WORKSPACE ?? resolve(frontendRoot, '..')).replace(/\\/g, '/').toLowerCase()
const viteEntrypoint = resolve(frontendRoot, 'node_modules/vite/bin/vite.js').replace(/\\/g, '/').toLowerCase()
const e2eConfig = resolve(frontendRoot, 'vite.e2e.config.ts').replace(/\\/g, '/').toLowerCase()

function readPort(value = process.env.UI_LAB_E2E_PORT ?? String(DEFAULT_PORT)) {
  const port = Number.parseInt(value, 10)
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`UI_LAB_E2E_PORT must be a valid TCP port, received: ${value}`)
  }
  return port
}

function isPortInUse(port) {
  return new Promise((resolvePromise) => {
    const socket = net.connect({ host: '127.0.0.1', port })
    socket.once('connect', () => {
      socket.destroy()
      resolvePromise(true)
    })
    socket.once('error', () => resolvePromise(false))
    socket.setTimeout(800, () => {
      socket.destroy()
      resolvePromise(true)
    })
  })
}

function windowsListeners(port) {
  const script = [
    `$listeners = Get-NetTCPConnection -LocalPort ${port} -State Listen -ErrorAction Stop`,
    '$listeners | ForEach-Object {',
    '  $process = Get-CimInstance Win32_Process -Filter "ProcessId = $($_.OwningProcess)" -ErrorAction Stop',
    '  [pscustomobject]@{ pid = $_.OwningProcess; commandLine = $process.CommandLine }',
    '} | ConvertTo-Json -Compress',
  ].join('; ')
  let stdout
  try {
    stdout = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: SYSTEM_COMMAND_TIMEOUT_MS,
    }).trim()
  } catch (error) {
    throw new Error(`UI Lab E2E preflight: listener query for port ${port} failed: ${error.message}`)
  }
  if (!stdout) return []
  const parsed = JSON.parse(stdout)
  return Array.isArray(parsed) ? parsed : [parsed]
}

function isStaleUiLabVite(listener) {
  const commandLine = String(listener.commandLine ?? '').replace(/\\/g, '/').toLowerCase()
  return commandLine.includes(workspace)
    && commandLine.includes(viteEntrypoint)
    && commandLine.includes(e2eConfig)
}

async function waitForPortFree(port, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (!(await isPortInUse(port))) return
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 300))
  }
  throw new Error(`UI Lab E2E preflight: port ${port} is still in use after ${timeoutMs}ms`)
}

async function releaseStaleWorktreeVite(port, { rejectForeign }) {
  if (!(await isPortInUse(port))) return false
  if (process.platform !== 'win32') {
    if (rejectForeign) throw new Error(`UI Lab E2E preflight: port ${port} is already in use; refusing to terminate a process on ${process.platform}`)
    return false
  }

  const listeners = windowsListeners(port)
  if (listeners.length === 0) {
    if (rejectForeign) throw new Error(`UI Lab E2E preflight: port ${port} is in use but no Windows listener PID was found`)
    return false
  }
  const foreign = listeners.filter((listener) => !isStaleUiLabVite(listener))
  if (foreign.length > 0) {
    if (rejectForeign) {
      throw new Error(`UI Lab E2E preflight: port ${port} is owned by a non-worktree process; refusing to terminate it: ${JSON.stringify(foreign)}`)
    }
    return false
  }

  for (const listener of listeners) {
    console.log(`UI Lab E2E preflight: terminating stale Vite tree PID ${listener.pid} on port ${port}`)
    try {
      execFileSync('taskkill.exe', ['/pid', String(listener.pid), '/T', '/F'], {
        stdio: 'inherit',
        timeout: SYSTEM_COMMAND_TIMEOUT_MS,
      })
    } catch (error) {
      throw new Error(`UI Lab E2E preflight: failed to terminate stale Vite tree PID ${listener.pid}: ${error.message}`)
    }
  }
  await waitForPortFree(port)
  console.log(`UI Lab E2E preflight: port ${port} released`)
  return true
}

async function allocatePort() {
  const githubEnv = process.env.GITHUB_ENV
  if (!githubEnv) throw new Error('UI Lab E2E allocator requires GITHUB_ENV')

  const runId = Number.parseInt(process.env.GITHUB_RUN_ID ?? '0', 10)
  const baseOffset = Number.isFinite(runId) ? runId % PORT_RANGE.size : 0
  const deadline = Date.now() + ALLOCATION_TIMEOUT_MS
  for (let offset = 0; offset < PORT_RANGE.size && Date.now() < deadline; offset += 1) {
    const candidate = PORT_RANGE.start + ((baseOffset + offset) % PORT_RANGE.size)
    if (!(await isPortInUse(candidate)) || await releaseStaleWorktreeVite(candidate, { rejectForeign: false })) {
      appendFileSync(githubEnv, `UI_LAB_E2E_PORT=${candidate}\n`, 'utf8')
      console.log(`UI Lab E2E allocator: selected port ${candidate}`)
      return
    }
  }
  throw new Error(`UI Lab E2E allocator: no safe free port found in ${PORT_RANGE.start}-${PORT_RANGE.start + PORT_RANGE.size - 1} within ${ALLOCATION_TIMEOUT_MS}ms`)
}

async function preflightPort() {
  const port = readPort()
  if (!(await isPortInUse(port))) {
    console.log(`UI Lab E2E preflight: port ${port} is free`)
    return
  }
  await releaseStaleWorktreeVite(port, { rejectForeign: true })
}

if (process.argv[2] === '--allocate') await allocatePort()
else await preflightPort()
