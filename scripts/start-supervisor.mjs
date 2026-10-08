#!/usr/bin/env node

import { spawn, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  appendFileSync,
  copyFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import http from 'node:http'
import https from 'node:https'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(scriptDir, '..')
const isWindows = process.platform === 'win32'
const runtimeRootDefault = path.join(repoRoot, '.talos-runtime')

const SECRET_KEY = /(password|secret|token|api[_-]?key|cookie|dsn|database_url|remote_sync)/i
const ANSI_RE = /\x1b\[[0-?]*[ -/]*[@-~]/g
const DEFAULT_KEEP_DAYS = 14
const DEFAULT_KEEP_SESSIONS = 30
const DEFAULT_READY_TIMEOUT_MS = 45_000

function printUsage() {
  console.log(`
TALOS Runtime Supervisor

Usage:
  start.cmd [start] [options] [-- launcher-options]
  start.cmd debug [options] [-- launcher-options]
  start.cmd trace [options] [-- launcher-options]
  start.cmd doctor [--ci]
  start.cmd diagnose [options]
  start.cmd dump [options]
  start.cmd clean-logs [--keep-days N] [--keep-sessions N]

Modes:
  start          Normal development startup (default)
  debug          RUST_BACKTRACE=1, RUST_LOG=debug, Node source maps/warnings
  trace          Full Rust backtrace, trace logging, Node uncaught/warning traces
  doctor         Validate tools, files, dependency state, ports and Git state
  diagnose       Create a diagnostic snapshot without starting TALOS
  dump           Snapshot the latest runtime process tree; use ProcDump when available
  clean-logs     Apply session retention immediately

Options:
  --install              Force pnpm frozen install before startup
  --no-install           Never auto-install missing workspace dependencies
  --strict-ready         Exit non-zero when backend/frontend readiness times out
  --ready-timeout N      Readiness timeout in seconds (default: 45)
  --log-root PATH        Override .talos-runtime location
  --keep-days N          Delete sessions older than N days (default: 14)
  --keep-sessions N      Keep at most N newest sessions (default: 30)
  --native-dump MODE     auto | on | off (default: auto)
  --pid N                Additional PID for diagnose/dump
  --no-archive           Keep diagnostic directory without ZIP packaging
  --ci                   Machine-oriented doctor exit status
  --help                 Show this help

Examples:
  start.cmd
  start.cmd debug -- --port 3001 --frontend-port 5174
  start.cmd doctor --ci
  start.cmd dump --native-dump on
  start.cmd clean-logs --keep-days 7 --keep-sessions 20
`.trim())
}

function parseArgs(argv) {
  const knownModes = new Set(['start', 'debug', 'trace', 'doctor', 'diagnose', 'dump', 'clean-logs', 'help'])
  let mode = 'start'
  let index = 0
  if (argv[0] && knownModes.has(argv[0])) {
    mode = argv[0]
    index = 1
  }

  const options = {
    mode,
    install: false,
    autoInstall: true,
    strictReady: false,
    readyTimeoutMs: DEFAULT_READY_TIMEOUT_MS,
    logRoot: runtimeRootDefault,
    keepDays: DEFAULT_KEEP_DAYS,
    keepSessions: DEFAULT_KEEP_SESSIONS,
    nativeDump: mode === 'dump' ? 'on' : 'auto',
    archive: true,
    ci: false,
    pid: null,
    passthrough: [],
  }

  let passthrough = false
  for (; index < argv.length; index += 1) {
    const arg = argv[index]
    if (passthrough) {
      options.passthrough.push(arg)
      continue
    }
    if (arg === '--') {
      passthrough = true
      continue
    }
    if (arg === '--help' || arg === '-h') {
      options.help = true
      continue
    }
    if (arg === '--install') {
      options.install = true
      continue
    }
    if (arg === '--no-install') {
      options.autoInstall = false
      continue
    }
    if (arg === '--strict-ready') {
      options.strictReady = true
      continue
    }
    if (arg === '--no-archive') {
      options.archive = false
      continue
    }
    if (arg === '--ci') {
      options.ci = true
      continue
    }
    if (arg === '--ready-timeout' && argv[index + 1]) {
      options.readyTimeoutMs = Math.max(1, Number(argv[++index])) * 1000
      continue
    }
    if (arg === '--log-root' && argv[index + 1]) {
      options.logRoot = path.resolve(repoRoot, argv[++index])
      continue
    }
    if (arg === '--keep-days' && argv[index + 1]) {
      options.keepDays = Math.max(0, Number(argv[++index]))
      continue
    }
    if (arg === '--keep-sessions' && argv[index + 1]) {
      options.keepSessions = Math.max(1, Number(argv[++index]))
      continue
    }
    if (arg === '--native-dump' && argv[index + 1]) {
      options.nativeDump = argv[++index]
      continue
    }
    if (arg.startsWith('--native-dump=')) {
      options.nativeDump = arg.slice('--native-dump='.length)
      continue
    }
    if (arg === '--pid' && argv[index + 1]) {
      options.pid = Number(argv[++index])
      continue
    }

    options.passthrough.push(arg)
  }

  if (!['auto', 'on', 'off'].includes(options.nativeDump)) {
    throw new Error(`--native-dump must be auto, on or off; got ${options.nativeDump}`)
  }
  if (!Number.isFinite(options.readyTimeoutMs)) throw new Error('--ready-timeout must be numeric')
  if (!Number.isFinite(options.keepDays)) throw new Error('--keep-days must be numeric')
  if (!Number.isFinite(options.keepSessions)) throw new Error('--keep-sessions must be numeric')
  if (options.pid !== null && (!Number.isInteger(options.pid) || options.pid <= 0)) {
    throw new Error('--pid must be a positive integer')
  }
  return options
}

function timestampId(date = new Date()) {
  return date.toISOString().replace(/[:.]/g, '-')
}

function stripAnsi(value) {
  return String(value).replace(ANSI_RE, '')
}

function ensureDir(dir) {
  mkdirSync(dir, { recursive: true })
}

function writeJson(file, value) {
  ensureDir(path.dirname(file))
  const temp = `${file}.${process.pid}.tmp`
  writeFileSync(temp, `${JSON.stringify(value, null, 2)}\n`, 'utf8')
  try {
    rmSync(file, { force: true })
  } catch {}
  copyFileSync(temp, file)
  rmSync(temp, { force: true })
}

function readJson(file, fallback = null) {
  try {
    return JSON.parse(readFileSync(file, 'utf8'))
  } catch {
    return fallback
  }
}

function redactValue(key, value) {
  if (SECRET_KEY.test(key)) return '<redacted>'
  if (typeof value !== 'string') return value
  return value
    .replace(/(password|secret|token|api[_-]?key)=([^&\s]+)/gi, '$1=<redacted>')
    .replace(/(--?(?:password|secret|token|api[_-]?key)\s+)("[^"]*"|'[^']*'|\S+)/gi, '$1<redacted>')
    .replace(/(postgres(?:ql)?|mysql|sqlite):\/\/[^\s]+/gi, '<redacted-database-url>')
}

function sanitizedArgs(args) {
  return args.map((arg, index) => {
    const previous = args[index - 1] ?? ''
    if (SECRET_KEY.test(arg) || SECRET_KEY.test(previous)) return '<redacted>'
    return redactValue('arg', arg)
  })
}

function selectedEnvironment(env) {
  const allow = [
    'NODE_ENV',
    'NODE_OPTIONS',
    'RUST_LOG',
    'RUST_BACKTRACE',
    'DB_BACKEND',
    'DB_PATH',
    'DATABASE_URL',
    'HOST',
    'PORT',
    'VITE_DEV_URL',
    'VITE_BACKEND_TARGET',
    'PUBLIC_DIR',
    'PG_DATA_DIR',
    'CORS_ALLOWED_ORIGIN',
    'SENTRY_DSN',
    'AUTH_BOOTSTRAP_ON_START',
  ]
  const out = {}
  for (const key of allow) {
    if (env[key] !== undefined) out[key] = redactValue(key, env[key])
  }
  return out
}

function commandResult(command, args = [], options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? repoRoot,
    encoding: 'utf8',
    windowsHide: true,
    shell: options.shell ?? false,
    env: options.env ?? process.env,
    timeout: options.timeout ?? 15_000,
  })
  return {
    command: [command, ...args].join(' '),
    status: result.status,
    signal: result.signal,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
    error: result.error ? String(result.error.message ?? result.error) : null,
  }
}

function toolVersion(name, command, args, { required = true, shell = false } = {}) {
  const result = commandResult(command, args, { shell })
  const output = `${result.stdout}${result.stderr}`.trim().split(/\r?\n/)[0] ?? ''
  return {
    name,
    required,
    ok: result.status === 0,
    version: output || null,
    error: result.error || (result.status === 0 ? null : `exit ${result.status}`),
  }
}

function gitInfo() {
  const branch = commandResult('git', ['branch', '--show-current'])
  const head = commandResult('git', ['rev-parse', 'HEAD'])
  const status = commandResult('git', ['status', '--short', '--branch'])
  return {
    branch: branch.status === 0 ? branch.stdout.trim() : null,
    head: head.status === 0 ? head.stdout.trim() : null,
    status: status.stdout.trim(),
    dirty: status.status === 0 && status.stdout
      .split(/\r?\n/)
      .some((line) => line && !line.startsWith('##')),
  }
}

function dependencyState() {
  const rootModules = path.join(repoRoot, 'node_modules')
  const frontendModules = path.join(repoRoot, 'frontend', 'node_modules')
  return {
    rootNodeModules: existsSync(rootModules),
    frontendNodeModules: existsSync(frontendModules),
    pnpmLock: existsSync(path.join(repoRoot, 'pnpm-lock.yaml')),
    cargoLock: existsSync(path.join(repoRoot, 'backend', 'Cargo.lock')),
  }
}

function runDoctor(options, { print = true } = {}) {
  const tools = [
    toolVersion('Node.js', process.execPath, ['--version']),
    toolVersion('pnpm', isWindows ? 'pnpm.cmd' : 'pnpm', ['--version'], { shell: isWindows }),
    toolVersion('Git', 'git', ['--version']),
    toolVersion('Rust compiler', 'rustc', ['--version']),
    toolVersion('Cargo', 'cargo', ['--version']),
    toolVersion('ProcDump', isWindows ? 'where.exe' : 'which', [isWindows ? 'procdump.exe' : 'procdump'], {
      required: false,
    }),
    toolVersion('Archive tool', isWindows ? 'where.exe' : 'which', [isWindows ? 'tar.exe' : 'tar'], {
      required: false,
    }),
  ]

  const requiredFiles = [
    'start.js',
    'package.json',
    'pnpm-lock.yaml',
    'pnpm-workspace.yaml',
    'frontend/package.json',
    'backend/Cargo.toml',
    'backend/Cargo.lock',
  ].map((relative) => ({
    path: relative,
    ok: existsSync(path.join(repoRoot, relative)),
  }))

  const result = {
    timestamp: new Date().toISOString(),
    platform: {
      platform: process.platform,
      arch: process.arch,
      release: os.release(),
      hostname: os.hostname(),
    },
    tools,
    files: requiredFiles,
    dependencies: dependencyState(),
    git: gitInfo(),
  }

  const failures = [
    ...tools.filter((tool) => tool.required && !tool.ok).map((tool) => `tool:${tool.name}`),
    ...requiredFiles.filter((file) => !file.ok).map((file) => `file:${file.path}`),
  ]
  if (options.ci && (!result.dependencies.rootNodeModules || !result.dependencies.frontendNodeModules)) {
    failures.push('dependencies:node_modules')
  }
  result.ok = failures.length === 0
  result.failures = failures

  if (print) {
    console.log('TALOS Runtime Doctor')
    console.log('────────────────────────────────────────────────────────')
    for (const tool of tools) {
      console.log(`${tool.ok ? 'PASS' : tool.required ? 'FAIL' : 'INFO'}  ${tool.name.padEnd(16)} ${tool.version ?? tool.error ?? 'not found'}`)
    }
    for (const file of requiredFiles) {
      console.log(`${file.ok ? 'PASS' : 'FAIL'}  ${file.path}`)
    }
    console.log(`${result.dependencies.rootNodeModules ? 'PASS' : 'WARN'}  root node_modules`)
    console.log(`${result.dependencies.frontendNodeModules ? 'PASS' : 'WARN'}  frontend node_modules`)
    console.log(`${result.git.dirty ? 'WARN' : 'PASS'}  Git working tree${result.git.branch ? ` (${result.git.branch}@${result.git.head?.slice(0, 12)})` : ''}`)
    console.log('────────────────────────────────────────────────────────')
    console.log(result.ok ? 'Doctor result: healthy' : `Doctor result: ${failures.join(', ')}`)
  }

  return result
}

function pruneSessions(logRoot, keepDays, keepSessions, logger = null) {
  const sessionsDir = path.join(logRoot, 'sessions')
  ensureDir(sessionsDir)
  const now = Date.now()
  const maxAge = keepDays * 24 * 60 * 60 * 1000
  const entries = readdirSync(sessionsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const fullPath = path.join(sessionsDir, entry.name)
      let modified = 0
      try {
        modified = statSync(fullPath).mtimeMs
      } catch {}
      return { name: entry.name, fullPath, modified }
    })
    .sort((a, b) => b.modified - a.modified)

  const removed = []
  for (let index = 0; index < entries.length; index += 1) {
    const entry = entries[index]
    const expired = keepDays === 0 ? false : now - entry.modified > maxAge
    const excess = index >= keepSessions
    if (!expired && !excess) continue
    try {
      rmSync(entry.fullPath, { recursive: true, force: true })
      removed.push(entry.name)
    } catch (error) {
      logger?.(`Unable to prune ${entry.fullPath}: ${error.message}`, 'warn')
    }
  }
  return removed
}

function createSession(options, mode = options.mode) {
  ensureDir(options.logRoot)
  const id = `${timestampId()}-${process.pid}`
  const dir = path.join(options.logRoot, 'sessions', id)
  ensureDir(dir)
  const paths = {
    dir,
    launcherLog: path.join(dir, 'launcher.log'),
    consoleLog: path.join(dir, 'console.log'),
    stdoutLog: path.join(dir, 'stdout.log'),
    stderrLog: path.join(dir, 'stderr.log'),
    eventsLog: path.join(dir, 'events.jsonl'),
    sessionJson: path.join(dir, 'session.json'),
    runtimeJson: path.join(dir, 'runtime.json'),
    exitJson: path.join(dir, 'exit.json'),
    diagnosticsDir: path.join(dir, 'diagnostics'),
  }
  ensureDir(paths.diagnosticsDir)

  const state = {
    id,
    mode,
    status: 'initializing',
    createdAt: new Date().toISOString(),
    repoRoot,
    logRoot: options.logRoot,
    paths,
    supervisorPid: process.pid,
    launcherPid: null,
    args: sanitizedArgs(process.argv.slice(2)),
    passthrough: sanitizedArgs(options.passthrough),
    tools: null,
    git: gitInfo(),
    environment: selectedEnvironment(process.env),
    readiness: {
      backend: { url: null, status: 'unknown' },
      frontend: { url: null, status: 'unknown' },
    },
  }
  writeJson(paths.sessionJson, state)
  writeJson(paths.runtimeJson, state)
  writeJson(path.join(options.logRoot, 'latest.json'), {
    sessionId: id,
    sessionDir: dir,
    runtimeJson: paths.runtimeJson,
    supervisorPid: process.pid,
    launcherPid: null,
    status: state.status,
    updatedAt: new Date().toISOString(),
  })
  writeFileSync(path.join(options.logRoot, 'latest.txt'), `${dir}\n`, 'utf8')
  return { state, paths }
}

function createLogger(paths) {
  function log(message, level = 'info', extra = {}) {
    const event = {
      timestamp: new Date().toISOString(),
      level,
      source: 'supervisor',
      message: stripAnsi(message),
      ...extra,
    }
    const line = `[${event.timestamp}] [${level.toUpperCase()}] ${event.message}`
    appendFileSync(paths.launcherLog, `${line}\n`, 'utf8')
    appendFileSync(paths.consoleLog, `${line}\n`, 'utf8')
    appendFileSync(paths.eventsLog, `${JSON.stringify(event)}\n`, 'utf8')
    const prefix = level === 'error' ? '\x1b[31m' : level === 'warn' ? '\x1b[33m' : '\x1b[90m'
    console.log(`${prefix}${line}\x1b[0m`)
  }
  return log
}

function updateRuntime(session, options, patch) {
  Object.assign(session.state, patch)
  session.state.updatedAt = new Date().toISOString()
  writeJson(session.paths.runtimeJson, session.state)
  writeJson(path.join(options.logRoot, 'latest.json'), {
    sessionId: session.state.id,
    sessionDir: session.paths.dir,
    runtimeJson: session.paths.runtimeJson,
    supervisorPid: session.state.supervisorPid,
    launcherPid: session.state.launcherPid,
    status: session.state.status,
    updatedAt: session.state.updatedAt,
  })
}

function buildChildEnvironment(options, session) {
  const env = { ...process.env }
  env.TALOS_SESSION_ID = session.state.id
  env.TALOS_SESSION_DIR = session.paths.dir
  env.TALOS_LOG_DIR = session.paths.dir

  if (options.mode === 'debug') {
    env.RUST_BACKTRACE = env.RUST_BACKTRACE || '1'
    env.RUST_LOG = env.RUST_LOG || 'debug'
    env.NODE_OPTIONS = [env.NODE_OPTIONS, '--enable-source-maps', '--trace-warnings'].filter(Boolean).join(' ')
  } else if (options.mode === 'trace') {
    env.RUST_BACKTRACE = 'full'
    env.RUST_LOG = env.RUST_LOG || 'trace'
    env.NODE_OPTIONS = [
      env.NODE_OPTIONS,
      '--enable-source-maps',
      '--trace-warnings',
      '--trace-uncaught',
    ].filter(Boolean).join(' ')
  }
  return env
}

function runInstall(session, log) {
  log('Workspace dependencies are missing or --install was requested; running frozen pnpm install.')
  const installLog = path.join(session.paths.dir, 'install.log')
  const output = createWriteStream(installLog, { flags: 'a' })
  return new Promise((resolve) => {
    const child = spawn(isWindows ? 'pnpm.cmd' : 'pnpm', ['install', '--frozen-lockfile'], {
      cwd: repoRoot,
      env: process.env,
      shell: isWindows,
      windowsHide: false,
      stdio: ['inherit', 'pipe', 'pipe'],
    })
    const tee = (chunk, stream) => {
      stream.write(chunk)
      output.write(chunk)
    }
    child.stdout.on('data', (chunk) => tee(chunk, process.stdout))
    child.stderr.on('data', (chunk) => tee(chunk, process.stderr))
    child.on('error', (error) => {
      output.end()
      log(`pnpm install failed to start: ${error.message}`, 'error')
      resolve(false)
    })
    child.on('exit', (code) => {
      output.end()
      log(`pnpm install exited with code ${code}.`, code === 0 ? 'info' : 'error')
      resolve(code === 0)
    })
  })
}

function lineCollector(source, streamName, session, onLine) {
  let buffer = ''
  const targetLog = streamName === 'stdout' ? session.paths.stdoutLog : session.paths.stderrLog

  function emit(line) {
    const clean = stripAnsi(line.replace(/\r$/, ''))
    appendFileSync(targetLog, `${clean}\n`, 'utf8')
    appendFileSync(session.paths.consoleLog, `${clean}\n`, 'utf8')
    appendFileSync(session.paths.eventsLog, `${JSON.stringify({
      timestamp: new Date().toISOString(),
      level: streamName === 'stderr' ? 'error-stream' : 'info',
      source,
      stream: streamName,
      message: clean,
    })}\n`, 'utf8')
    onLine(clean)
  }

  return {
    push(chunk) {
      buffer += String(chunk)
      const lines = buffer.split(/\n/)
      buffer = lines.pop() ?? ''
      for (const line of lines) emit(line)
    },
    flush() {
      if (buffer) emit(buffer)
      buffer = ''
    },
  }
}

function extractUrls(line, session, options, log) {
  const urlMatches = [...line.matchAll(/https?:\/\/localhost:(\d+)/g)]
  if (urlMatches.length === 0) return

  if (/服务/.test(line) && !session.state.readiness.backend.url) {
    session.state.readiness.backend.url = urlMatches[0][0]
    updateRuntime(session, options, { readiness: session.state.readiness })
    log(`Detected backend URL ${urlMatches[0][0]}.`)
    void monitorReadiness('backend', `${urlMatches[0][0]}/health`, session, options, log)
  } else if (/Vite/.test(line) && !session.state.readiness.frontend.url) {
    session.state.readiness.frontend.url = urlMatches[0][0]
    updateRuntime(session, options, { readiness: session.state.readiness })
    log(`Detected frontend URL ${urlMatches[0][0]}.`)
    void monitorReadiness('frontend', urlMatches[0][0], session, options, log)
  }
}

function probeUrl(url, timeoutMs = 2_000) {
  return new Promise((resolve) => {
    const client = url.startsWith('https:') ? https : http
    const request = client.get(url, {
      timeout: timeoutMs,
      rejectUnauthorized: false,
      headers: { 'user-agent': 'talos-runtime-supervisor/1' },
    }, (response) => {
      response.resume()
      resolve({
        ok: response.statusCode !== undefined && response.statusCode >= 200 && response.statusCode < 500,
        statusCode: response.statusCode ?? null,
      })
    })
    request.on('timeout', () => {
      request.destroy(new Error('timeout'))
    })
    request.on('error', (error) => resolve({ ok: false, error: error.message }))
  })
}

async function monitorReadiness(name, url, session, options, log) {
  const started = Date.now()
  const readiness = session.state.readiness[name]
  readiness.status = 'probing'
  readiness.probeUrl = url
  updateRuntime(session, options, { readiness: session.state.readiness })

  while (Date.now() - started < options.readyTimeoutMs && session.state.status === 'running') {
    const result = await probeUrl(url)
    readiness.lastProbeAt = new Date().toISOString()
    readiness.lastResult = result
    if (result.ok) {
      readiness.status = 'ready'
      readiness.readyAt = new Date().toISOString()
      updateRuntime(session, options, { readiness: session.state.readiness })
      log(`${name} readiness passed (${url}, HTTP ${result.statusCode}).`)
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 1000))
  }

  if (session.state.status !== 'running') {
    readiness.status = 'cancelled'
    updateRuntime(session, options, { readiness: session.state.readiness })
    return
  }

  readiness.status = 'timeout'
  updateRuntime(session, options, { readiness: session.state.readiness })
  log(`${name} readiness timed out after ${options.readyTimeoutMs / 1000}s (${url}).`, 'warn')
  await collectDiagnostics(`readiness-${name}`, session, options, log, {
    native: false,
    pids: [session.state.launcherPid].filter(Boolean),
  })

  if (options.strictReady && session.state.status === 'running') {
    session.state.strictReadyFailure = name
    updateRuntime(session, options, { strictReadyFailure: name })
    requestTermination(session, log, 'strict readiness failure')
  }
}

function captureText(file, command, args = [], options = {}) {
  const result = commandResult(command, args, options)
  const text = [
    `$ ${result.command}`,
    `status=${result.status} signal=${result.signal ?? ''} error=${result.error ?? ''}`,
    '',
    result.stdout,
    result.stderr,
  ].join('\n')
  writeFileSync(file, text, 'utf8')
  return result
}

function tailFile(source, destination, maxBytes = 256 * 1024) {
  if (!existsSync(source)) return false
  const data = readFileSync(source)
  const tail = data.length > maxBytes ? data.subarray(data.length - maxBytes) : data
  writeFileSync(destination, tail)
  return true
}

function sha256(file) {
  const hash = createHash('sha256')
  hash.update(readFileSync(file))
  return hash.digest('hex')
}

function discoverWindowsProcessTree(rootPids) {
  if (!isWindows || rootPids.length === 0) return []
  const roots = rootPids.filter((pid) => Number.isInteger(pid) && pid > 0)
  const script = [
    '$all = Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine',
    `$roots = @(${roots.join(',')})`,
    '$selected = @()',
    '$queue = New-Object System.Collections.Queue',
    'foreach ($r in $roots) { $queue.Enqueue([int]$r) }',
    'while ($queue.Count -gt 0) {',
    '  $id = $queue.Dequeue()',
    '  $p = $all | Where-Object { $_.ProcessId -eq $id }',
    '  if ($p) { $selected += $p }',
    '  foreach ($child in ($all | Where-Object { $_.ParentProcessId -eq $id })) {',
    '    if (-not ($selected | Where-Object { $_.ProcessId -eq $child.ProcessId })) { $queue.Enqueue([int]$child.ProcessId) }',
    '  }',
    '}',
    '$selected | Sort-Object ProcessId -Unique | ConvertTo-Json -Compress',
  ].join('; ')

  const result = commandResult('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-ExecutionPolicy',
    'Bypass',
    '-Command',
    script,
  ], { timeout: 30_000 })

  if (result.status !== 0 || !result.stdout.trim()) return []
  try {
    const parsed = JSON.parse(result.stdout)
    return Array.isArray(parsed) ? parsed : [parsed]
  } catch {
    return []
  }
}

function findProcDump() {
  if (!isWindows) return null
  const result = commandResult('where.exe', ['procdump.exe'])
  if (result.status !== 0) return null
  return result.stdout.split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? null
}

function createNativeDumps(processes, diagnosticsDir, options, log) {
  const procDump = findProcDump()
  if (!procDump) {
    log('ProcDump is not installed; native .dmp capture was skipped.', options.nativeDump === 'on' ? 'warn' : 'info')
    return []
  }

  const results = []
  for (const processInfo of processes) {
    const pid = Number(processInfo.ProcessId)
    if (!Number.isInteger(pid) || pid <= 0) continue
    const name = String(processInfo.Name ?? 'process').replace(/[^a-zA-Z0-9._-]/g, '_')
    const dumpPath = path.join(diagnosticsDir, `${name}-${pid}.dmp`)
    const result = commandResult(procDump, ['-accepteula', '-ma', String(pid), dumpPath], {
      timeout: 120_000,
    })
    results.push({
      pid,
      name,
      dumpPath,
      ok: result.status === 0 && existsSync(dumpPath),
      status: result.status,
      stderr: result.stderr.trim(),
    })
  }
  return results
}

function archiveDirectory(directory, log) {
  const archivePath = `${directory}.zip`
  const parent = path.dirname(directory)
  const name = path.basename(directory)
  const result = commandResult(isWindows ? 'tar.exe' : 'tar', [
    '-a',
    '-c',
    '-f',
    archivePath,
    '-C',
    parent,
    name,
  ], { timeout: 120_000 })
  if (result.status === 0 && existsSync(archivePath)) {
    log(`Diagnostic archive created: ${archivePath}`)
    return archivePath
  }
  log(`Diagnostic archive was not created; directory remains at ${directory}.`, 'warn')
  return null
}

async function collectDiagnostics(reason, session, options, log, {
  native = options.nativeDump === 'on',
  pids = [],
} = {}) {
  const diagnosticsDir = path.join(session.paths.diagnosticsDir, `${timestampId()}-${reason.replace(/[^a-zA-Z0-9._-]/g, '_')}`)
  ensureDir(diagnosticsDir)
  log(`Collecting diagnostics: ${diagnosticsDir}`)

  const targetPids = [...new Set([
    ...pids,
    options.pid,
    session.state.supervisorPid,
    session.state.launcherPid,
  ].filter((pid) => Number.isInteger(pid) && pid > 0))]

  const processTree = discoverWindowsProcessTree(targetPids).map((entry) => ({
    ...entry,
    CommandLine: entry.CommandLine ? redactValue('process-command-line', String(entry.CommandLine)) : entry.CommandLine,
  }))
  writeJson(path.join(diagnosticsDir, 'process-tree.json'), processTree)

  const summary = {
    reason,
    createdAt: new Date().toISOString(),
    sessionId: session.state.id,
    repoRoot,
    platform: {
      platform: process.platform,
      arch: process.arch,
      release: os.release(),
      hostname: os.hostname(),
      cpus: os.cpus().map((cpu) => cpu.model),
      totalMemory: os.totalmem(),
      freeMemory: os.freemem(),
      uptime: os.uptime(),
    },
    pids: targetPids,
    git: gitInfo(),
    environment: selectedEnvironment(process.env),
    runtime: readJson(session.paths.runtimeJson, session.state),
    doctor: runDoctor(options, { print: false }),
  }
  writeJson(path.join(diagnosticsDir, 'summary.json'), summary)

  try {
    process.report.writeReport(path.join(diagnosticsDir, 'supervisor-node-report.json'))
  } catch (error) {
    writeFileSync(path.join(diagnosticsDir, 'supervisor-node-report.error.txt'), String(error.stack ?? error), 'utf8')
  }

  captureText(path.join(diagnosticsDir, 'git-status.txt'), 'git', ['status', '--short', '--branch'])
  captureText(path.join(diagnosticsDir, 'git-log.txt'), 'git', ['log', '-10', '--oneline', '--decorate'])
  captureText(path.join(diagnosticsDir, 'versions.txt'), process.execPath, ['--version'])
  captureText(path.join(diagnosticsDir, 'cargo-version.txt'), 'cargo', ['--version'])
  captureText(path.join(diagnosticsDir, 'rustc-version.txt'), 'rustc', ['--version'])
  captureText(path.join(diagnosticsDir, 'pnpm-version.txt'), isWindows ? 'pnpm.cmd' : 'pnpm', ['--version'], {
    shell: isWindows,
  })

  if (isWindows) {
    captureText(path.join(diagnosticsDir, 'tasklist.txt'), 'tasklist.exe', ['/v', '/fo', 'csv'], { timeout: 30_000 })
    captureText(path.join(diagnosticsDir, 'netstat.txt'), 'netstat.exe', ['-ano'], { timeout: 30_000 })
    captureText(path.join(diagnosticsDir, 'systeminfo.txt'), 'systeminfo.exe', [], { timeout: 60_000 })
  } else {
    captureText(path.join(diagnosticsDir, 'processes.txt'), 'ps', ['aux'])
    captureText(path.join(diagnosticsDir, 'network.txt'), 'sh', ['-lc', 'ss -lntup || netstat -an'])
    captureText(path.join(diagnosticsDir, 'system.txt'), 'uname', ['-a'])
  }

  const tailed = []
  for (const [name, file] of Object.entries({
    launcher: session.paths.launcherLog,
    console: session.paths.consoleLog,
    stdout: session.paths.stdoutLog,
    stderr: session.paths.stderrLog,
  })) {
    const destination = path.join(diagnosticsDir, `${name}.tail.log`)
    if (tailFile(file, destination)) {
      tailed.push({ name, file: destination, sha256: sha256(destination) })
    }
  }

  let nativeDumps = []
  if (native && options.nativeDump !== 'off') {
    nativeDumps = createNativeDumps(processTree, diagnosticsDir, options, log)
  }

  const manifest = {
    reason,
    files: readdirSync(diagnosticsDir).sort(),
    logTails: tailed,
    nativeDumps,
    sensitiveDataWarning: 'Diagnostic logs may contain application data. Review before sharing.',
  }
  writeJson(path.join(diagnosticsDir, 'manifest.json'), manifest)

  const archivePath = options.archive ? archiveDirectory(diagnosticsDir, log) : null
  return { diagnosticsDir, archivePath, manifest }
}

function requestTermination(session, log, reason) {
  if (!session.child || session.terminationRequested) return
  session.terminationRequested = true
  log(`Termination requested: ${reason}.`, 'warn')
  try {
    session.child.kill('SIGTERM')
  } catch {}

  if (isWindows && session.state.launcherPid) {
    setTimeout(() => {
      if (session.child && session.child.exitCode === null) {
        commandResult('taskkill.exe', ['/T', '/F', '/PID', String(session.state.launcherPid)], {
          timeout: 15_000,
        })
      }
    }, 8_000).unref()
  }
}

async function runStart(options) {
  process.chdir(repoRoot)
  ensureDir(options.logRoot)
  pruneSessions(options.logRoot, options.keepDays, options.keepSessions)

  const session = createSession(options)
  const log = createLogger(session.paths)
  session.log = log

  const doctor = runDoctor(options, { print: false })
  session.state.tools = doctor.tools
  updateRuntime(session, options, { tools: doctor.tools, doctor })

  if (!doctor.ok) {
    log(`Preflight failed: ${doctor.failures.join(', ')}`, 'error')
    await collectDiagnostics('preflight-failure', session, options, log, { native: false })
    return 2
  }

  const dependencies = dependencyState()
  const missingDependencies = !dependencies.rootNodeModules || !dependencies.frontendNodeModules
  if (options.install || (missingDependencies && options.autoInstall)) {
    const installed = await runInstall(session, log)
    if (!installed) {
      await collectDiagnostics('install-failure', session, options, log, { native: false })
      return 3
    }
  } else if (missingDependencies) {
    log('Workspace dependencies are missing and --no-install is active.', 'error')
    await collectDiagnostics('missing-dependencies', session, options, log, { native: false })
    return 3
  }

  const childEnv = buildChildEnvironment(options, session)
  session.state.environment = selectedEnvironment(childEnv)
  session.state.status = 'starting'
  updateRuntime(session, options, {
    status: 'starting',
    environment: session.state.environment,
  })

  log(`Starting legacy interactive launcher in ${options.mode} mode.`)
  log(`Session directory: ${session.paths.dir}`)

  const child = spawn(process.execPath, [path.join(repoRoot, 'start.js'), ...options.passthrough], {
    cwd: repoRoot,
    env: childEnv,
    windowsHide: false,
    stdio: ['inherit', 'pipe', 'pipe'],
  })
  session.child = child
  session.state.launcherPid = child.pid
  session.state.status = 'running'
  updateRuntime(session, options, {
    launcherPid: child.pid,
    status: 'running',
    startedAt: new Date().toISOString(),
  })

  const stdoutCollector = lineCollector('launcher', 'stdout', session, (line) => {
    extractUrls(line, session, options, log)
  })
  const stderrCollector = lineCollector('launcher', 'stderr', session, (line) => {
    extractUrls(line, session, options, log)
  })

  child.stdout.on('data', (chunk) => {
    process.stdout.write(chunk)
    stdoutCollector.push(chunk)
  })
  child.stderr.on('data', (chunk) => {
    process.stderr.write(chunk)
    stderrCollector.push(chunk)
  })

  let settled = false
  const finish = async (code, signal, error = null) => {
    if (settled) return
    settled = true
    stdoutCollector.flush()
    stderrCollector.flush()

    const unexpected = !session.terminationRequested && (error !== null || code !== 0)
    session.state.status = unexpected ? 'failed' : 'stopped'
    updateRuntime(session, options, {
      status: session.state.status,
      exitAt: new Date().toISOString(),
      exitCode: code,
      exitSignal: signal,
      exitError: error ? String(error.stack ?? error) : null,
    })
    writeJson(session.paths.exitJson, {
      sessionId: session.state.id,
      timestamp: new Date().toISOString(),
      code,
      signal,
      unexpected,
      strictReadyFailure: session.state.strictReadyFailure ?? null,
      error: error ? String(error.stack ?? error) : null,
    })

    if (unexpected || session.state.strictReadyFailure) {
      await collectDiagnostics(
        session.state.strictReadyFailure ? `strict-ready-${session.state.strictReadyFailure}` : 'launcher-failure',
        session,
        options,
        log,
        {
          native: options.nativeDump === 'on',
          pids: [child.pid].filter(Boolean),
        },
      )
    }
    pruneSessions(options.logRoot, options.keepDays, options.keepSessions, log)
  }

  child.on('error', (error) => {
    log(`Launcher process error: ${error.message}`, 'error')
    void finish(1, null, error)
  })
  child.on('exit', (code, signal) => {
    log(`Launcher exited with code=${code} signal=${signal ?? 'none'}.`, code === 0 ? 'info' : 'error')
    void finish(code, signal).then(() => {
      process.exitCode = session.state.strictReadyFailure ? 4 : (code ?? 1)
    })
  })

  process.on('SIGINT', () => {
    requestTermination(session, log, 'SIGINT')
  })
  process.on('SIGTERM', () => {
    requestTermination(session, log, 'SIGTERM')
  })
  process.on('uncaughtException', async (error) => {
    log(`Supervisor uncaught exception: ${error.stack ?? error}`, 'error')
    await collectDiagnostics('supervisor-uncaught-exception', session, options, log, {
      native: options.nativeDump === 'on',
      pids: [child.pid].filter(Boolean),
    })
    requestTermination(session, log, 'supervisor exception')
    process.exitCode = 1
  })
  process.on('unhandledRejection', async (reason) => {
    const error = reason instanceof Error ? reason : new Error(String(reason))
    log(`Supervisor unhandled rejection: ${error.stack ?? error}`, 'error')
    await collectDiagnostics('supervisor-unhandled-rejection', session, options, log, {
      native: options.nativeDump === 'on',
      pids: [child.pid].filter(Boolean),
    })
    requestTermination(session, log, 'supervisor rejection')
    process.exitCode = 1
  })

  return await new Promise((resolve) => {
    child.on('close', (code) => {
      const exitCode = session.state.strictReadyFailure ? 4 : (code ?? 1)
      resolve(exitCode)
    })
  })
}

async function runSnapshotMode(options, native) {
  process.chdir(repoRoot)
  ensureDir(options.logRoot)
  pruneSessions(options.logRoot, options.keepDays, options.keepSessions)

  const latest = readJson(path.join(options.logRoot, 'latest.json'))
  let session
  if (latest?.sessionDir && existsSync(latest.sessionDir)) {
    const runtime = readJson(latest.runtimeJson, {})
    const paths = {
      dir: latest.sessionDir,
      launcherLog: path.join(latest.sessionDir, 'launcher.log'),
      consoleLog: path.join(latest.sessionDir, 'console.log'),
      stdoutLog: path.join(latest.sessionDir, 'stdout.log'),
      stderrLog: path.join(latest.sessionDir, 'stderr.log'),
      eventsLog: path.join(latest.sessionDir, 'events.jsonl'),
      sessionJson: path.join(latest.sessionDir, 'session.json'),
      runtimeJson: latest.runtimeJson,
      exitJson: path.join(latest.sessionDir, 'exit.json'),
      diagnosticsDir: path.join(latest.sessionDir, 'diagnostics'),
    }
    ensureDir(paths.diagnosticsDir)
    session = { state: runtime, paths }
  } else {
    session = createSession(options, options.mode)
  }

  const log = createLogger(session.paths)
  const pids = [
    options.pid,
    session.state.supervisorPid,
    session.state.launcherPid,
  ].filter((pid) => Number.isInteger(pid) && pid > 0)

  const result = await collectDiagnostics(options.mode, session, options, log, {
    native,
    pids,
  })
  console.log(`Diagnostics: ${result.archivePath ?? result.diagnosticsDir}`)
  return 0
}

async function main() {
  let options
  try {
    options = parseArgs(process.argv.slice(2))
  } catch (error) {
    console.error(`Argument error: ${error.message}`)
    printUsage()
    return 64
  }

  if (options.help || options.mode === 'help') {
    printUsage()
    return 0
  }

  if (options.mode === 'doctor') {
    const result = runDoctor(options)
    return result.ok ? 0 : 2
  }
  if (options.mode === 'clean-logs') {
    ensureDir(options.logRoot)
    const removed = pruneSessions(options.logRoot, options.keepDays, options.keepSessions)
    console.log(`Removed ${removed.length} runtime session(s).`)
    for (const session of removed) console.log(`  ${session}`)
    return 0
  }
  if (options.mode === 'diagnose') return await runSnapshotMode(options, false)
  if (options.mode === 'dump') return await runSnapshotMode(options, options.nativeDump !== 'off')
  return await runStart(options)
}

const exitCode = await main()
process.exitCode = exitCode
