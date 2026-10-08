/**
 * Talos 单终端启动器
 * 启动后端 (cargo run) + 前端 (npm run dev)，日志带作用域标签 + 微秒精度时间戳。
 *
 * CLI 风格交互命令（键入命令 + 回车执行）:
 *   restart [all|backend|frontend]    重启服务
 *   status                            查看进程与端口状态
 *   kill    <backend|frontend|pg>      停止服务（不自动重启）
 *   start   <backend|frontend|pg>      手动启动已停止的服务
 *   port    [set|scan]                查看/修改端口 / 扫描空闲端口
 *   clear                             清屏
 *   quit                              退出全部服务
 *   help                              显示此帮助
 *
 * 单字母别名仍可用: r/rb/rf  s  k  q  h
 *
 * 用法: node start.js [--port 8080] [--frontend-port 5173] [--pg-port 5432]
 */

const { spawn, execSync } = require('child_process')
const { platform } = require('os')
const { existsSync } = require('fs')
const path = require('path')
const readline = require('readline')

const args = process.argv.slice(2)
let backendPort = 0
let frontendPort = 0
let pgPort = 0

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--port' && args[i + 1]) backendPort = parseInt(args[++i])
  if (args[i] === '--frontend-port' && args[i + 1]) frontendPort = parseInt(args[++i])
  if (args[i] === '--pg-port' && args[i + 1]) pgPort = parseInt(args[++i])
}

// ── SSL detection ──
const certDir = path.join(__dirname, 'certs')
const hasSsl = existsSync(path.join(certDir, 'localhost.key')) && existsSync(path.join(certDir, 'localhost.crt'))
const protocol = hasSsl ? 'https' : 'http'

// ── Platform detection ──
const isWin = platform() === 'win32'

// ── Well-known ports — avoid these when scanning ──
const KNOWN_PORTS = new Set([
  // System
  20, 21, 22, 23, 25, 53, 67, 68, 69, 80, 110, 119, 123, 135, 137, 138, 139,
  143, 161, 162, 389, 443, 445, 465, 514, 515, 587, 636, 873, 993, 995,
  // Remote / VPN
  1080, 1433, 1521, 1723, 2082, 2083, 2086, 2087, 2095, 2096, 3260, 3389,
  4444, 4500, 5000, 5900, 5984,
  // Databases & message queues
  3306, 5432, 5672, 6379, 7687, 9042, 9092, 9200, 9300, 9418, 11211,
  27017, 27018, 27019, 28017,
  // DevOps & containers
  2181, 2375, 2376, 3000, 3128, 4200, 5000, 6443, 6881, 7000, 7474, 7777,
  8000, 8080, 8443, 8888, 8983, 9000, 9090, 9100, 9999,
  // Hadoop / big data
  10000, 50000, 50010, 50020, 50030, 50070, 50075, 50090,
  // Minecraft / games
  25565,
  // Misc
  5353, 6969, 33306,
])

// ── Port helpers ──
function portFree(p) {
  try {
    execSync(`netstat -ano | findstr ":${p} "`, { timeout: 2000, encoding: 'utf8', stdio: 'ignore' })
    return false
  } catch { return true }
}

function getWindowsExcludedPorts() {
  const s = new Set()
  if (!isWin) return s
  try {
    const out = execSync('netsh interface ipv4 show excludedportrange protocol=tcp', {
      timeout: 3000, encoding: 'utf8', stdio: 'pipe',
    })
    for (const line of out.split('\n')) {
      const m = line.trim().match(/^(\d+)\s+(\d+)/)
      if (m) {
        const lo = parseInt(m[1]), hi = parseInt(m[2])
        for (let p = lo; p <= hi && p <= 65535; p++) s.add(p)
      }
    }
  } catch {} // netsh unavailable → assume no exclusions
  return s
}

/** Check if a port is unavailable (occupied OR excluded OR well-known). */
function portBlocked(p) {
  if (KNOWN_PORTS.has(p)) return true
  if (!portFree(p)) return true
  return isWin && getWindowsExcludedPorts().has(p)
}

/**
 * Scan for `count` free ports starting from `start` (inclusive).
 * Skips well-known ports, occupied ports, and Windows-excluded ports.
 * Returns an array of { port, isKnown } objects.
 */
function portScan(start = backendPort + 1, count = 10) {
  const results = []
  const excluded = isWin ? getWindowsExcludedPorts() : new Set()
  let p = Math.max(start, 1024) // skip privileged ports
  while (results.length < count && p <= 65535) {
    if (!KNOWN_PORTS.has(p) && portFree(p) && !excluded.has(p)) {
      results.push(p)
    }
    p++
  }
  return results
}

/** Update env vars to reflect current port variables. */
function syncPortEnv() {
  process.env.PORT = String(backendPort)
  process.env.VITE_DEV_URL = `${protocol}://localhost:${frontendPort}`
  process.env.VITE_BACKEND_TARGET = `http://127.0.0.1:${backendPort}`
}

if (backendPort === 0) {
  backendPort = 3000
  const excluded = getWindowsExcludedPorts()
  while (!portFree(backendPort) || excluded.has(backendPort)) backendPort++
}
if (frontendPort === 0) { frontendPort = 5173; while (!portFree(frontendPort) || frontendPort === backendPort) frontendPort++ }
if (pgPort === 0) pgPort = 5432

// ── Colour helpers ──
const C = { reset: '\x1b[0m', cyan: '\x1b[36m', yellow: '\x1b[33m', grey: '\x1b[90m', red: '\x1b[31m', white: '\x1b[1m', magenta: '\x1b[35m', green: '\x1b[32m' }

// ── Unified ISO-8601 microsecond timestamp: YYYY-MM-DDTHH:MM:SS.mmmµµµZ ──
function ts() {
  const d = new Date()
  const Y = d.getFullYear()
  const M = String(d.getMonth() + 1).padStart(2, '0')
  const D = String(d.getDate()).padStart(2, '0')
  const h = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  const s = String(d.getSeconds()).padStart(2, '0')
  const ms = String(d.getMilliseconds()).padStart(3, '0')
  const hr = process.hrtime()
  const us = String(Math.floor(hr[1] / 1000) % 1000).padStart(3, '0')
  return `${Y}-${M}-${D}T${h}:${mi}:${s}.${ms}${us}Z`
}

// ═══════════════════════════════════════════════════════════════════════════
// Scope system
// ═══════════════════════════════════════════════════════════════════════════

const BACKEND_SCOPES = [
  { key: 'http',     label: 'HTTP ', color: C.yellow,   patterns: [/^\S+\s+\S+\s+http(?:::|$)/] },
  { key: 'db',       label: '数据 ', color: C.magenta,  patterns: [/^\S+\s+\S+\s+db::/, /sqlite/i, /sqlx/i, /database/i] },
  { key: 'migration',label: '迁移 ', color: C.white,    patterns: [/migration/i, /Applied.*migrations/i, /No pending.*migrations/i] },
  { key: 'auth',     label: '认证 ', color: C.green,    patterns: [/^\S+\s+\S+\s+auth(?:::|$)/, /^\S+\s+\S+\s+services::auth/] },
  { key: 'order',    label: '订单 ', color: C.cyan,     patterns: [/orders/i, /order_/] },
  { key: 'task',     label: '任务 ', color: C.white,    patterns: [/\[tasks\]/, /overdue/i, /seed/i] },
  { key: 'pg',       label: ' PG  ', color: C.magenta,  patterns: [/postgres/i, /pg_/, /PG\s/, /pg\s/] },
  { key: 'warn',     label: '警告 ', color: C.red,      patterns: [/^\S+\s+\S+\s+WARN\s/] },
  { key: 'error',    label: '错误 ', color: C.red,      patterns: [/^\S+\s+\S+\s+ERROR\s/, /\[ERROR\]/i] },
  { key: 'server',   label: '服务 ', color: C.cyan,     patterns: [/^[^\s]+\s+[^\s]+\s+(INFO|DEBUG|TRACE)\s/] },
  { key: '_default', label: '后端 ', color: C.grey,     patterns: [/.*/] },
]

const FRONTEND_SCOPES = [
  { key: 'hmr',      label: ' HMR ', color: C.green,   patterns: [/hmr/i, /hot\s?mod/i, /update/i] },
  { key: 'error',    label: '错误 ', color: C.red,      patterns: [/error/i, /fail/i, /TypeError/i, /SyntaxError/i, /ReferenceError/i] },
  { key: 'warn',     label: '警告 ', color: C.yellow,   patterns: [/warn/i, /deprecated/i] },
  { key: 'vite',     label: 'Vite ', color: C.yellow,   patterns: [/vite/i, /ready/i, /Local:/i, /Network:/i] },
  { key: '_default', label: '前端 ', color: C.yellow,   patterns: [/.*/] },
]

const PG_SCOPES = [
  { key: 'startup',  label: 'PG启 ', color: C.magenta, patterns: [/starting/i, /ready/i, /listening/i, /database system/i] },
  { key: 'query',    label: 'PG查 ', color: C.cyan,    patterns: [/statement:/i, /execute/i, /query/i] },
  { key: 'error',    label: 'PG错 ', color: C.red,     patterns: [/error/i, /fatal/i, /panic/i] },
  { key: 'warn',     label: 'PG警 ', color: C.yellow,  patterns: [/warn/i, /notice/i] },
  { key: 'checkpoint',label:'PG存 ',color: C.green,    patterns: [/checkpoint/i, /checkpoint complete/i] },
  { key: '_default', label: ' PG  ', color: C.magenta, patterns: [/.*/] },
]

function stripAnsi(s) { return s.replace(/\x1b\[[0-9;]*m/g, '') }

// Strip Rust tracing-subscriber fmt prefix: "YYYY-MM-DDTHH:MM:SS.μμμμμμZ  LEVEL target: "
// Backend produces one per line; start.js supplies its own unified ISO timestamp.
const RE_TRACING_PREFIX = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z\s+\w+\s+(?:\S+:\s+)?/
function stripTracingPrefix(line) {
  const m = line.match(RE_TRACING_PREFIX)
  return m ? line.slice(m[0].length) : line
}

function createScopeTagger(scopes) {
  return (data) => {
    const lines = String(data).split('\n')
    for (const line of lines) {
      if (!line.trim()) continue
      const clean = stripAnsi(line)
      // Match scope against the FULL line (tracing prefix still present for patterns)
      let scope = null
      for (const s of scopes) {
        if (s.key === '_default') continue
        if (s.patterns.some(p => p.test(clean))) { scope = s; break }
      }
      if (!scope) scope = scopes.find(s => s.key === '_default') || scopes[scopes.length - 1]
      // Strip native tracing timestamp so only start.js's unified ISO ts remains
      const message = stripTracingPrefix(clean)
      if (process.stdin.isTTY) { readline.clearLine(process.stdout, 0); readline.cursorTo(process.stdout, 0) }
      console.log(`${C.grey}${ts()}${C.reset} ${scope.color}${scope.label}${C.reset} ${message}`)
      if (process.stdin.isTTY) drawPrompt()
    }
  }
}

// ── Prompt ──
let promptShown = false
function drawPrompt() {
  if (!process.stdin.isTTY) return
  process.stdout.write(`${C.green}> ${C.reset}`)
  promptShown = true
}
function clearPrompt() {
  if (!process.stdin.isTTY) return
  if (promptShown) {
    readline.cursorTo(process.stdout, 0)
    readline.clearLine(process.stdout, 0)
    promptShown = false
  }
}

// ── Banner ──
console.log('')
console.log(`${C.white}Talos 租赁管理系统${C.reset}`)
console.log(`${C.grey}──────────────────────────────────────────${C.reset}`)
console.log(`  ${C.cyan}服务${C.reset}    ${protocol}://localhost:${backendPort}`)
console.log(`  ${C.yellow}Vite${C.reset}    ${protocol}://localhost:${frontendPort}`)
if (pgPort) console.log(`  ${C.magenta}PG${C.reset}       localhost:${pgPort}`)
console.log(`${C.grey}──────────────────────────────────────────${C.reset}`)
console.log(`  输入命令后按 Enter 执行，输入 ${C.green}help${C.reset} 查看所有命令`)
console.log('')

// ── Environment ──
syncPortEnv()
process.env.HTTP_FRONTEND_PORT = '0'
process.env.HTTP_PORT = '0'

// ═══════════════════════════════════════════════════════════════════════════
// Process management
// ═══════════════════════════════════════════════════════════════════════════

let backend = null
let frontend = null
let pgProcess = null
let backendStartedAt = null
let frontendStartedAt = null
let pgStartedAt = null
let backendPid = null
let frontendPid = null
let pgPid = null

function startBackend() {
  const cmd = isWin ? 'cargo run' : 'cargo run'
  const backendCwd = path.join(__dirname, 'backend')
  const tagger = createScopeTagger(BACKEND_SCOPES)
  const p = spawn(cmd, {
    cwd: backendCwd,
    stdio: 'pipe',
    shell: isWin,
    env: process.env,
  })
  p.stdout.on('data', tagger)
  p.stderr.on('data', tagger)
  p.on('exit', (code) => {
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.cyan}服务${C.reset}${C.grey} 已退出 (code ${code})${C.reset}`)
    drawPrompt()
    if (backend === p) { backend = null; backendStartedAt = null; backendPid = null }
  })
  backend = p
  backendStartedAt = Date.now()
  backendPid = p.pid
  return p
}

function startFrontend() {
  const frontendCwd = `${__dirname}/frontend`.replace(/\\/g, '/')
  const cmd = isWin
    ? `cd /d "${frontendCwd}" && npm run dev -- --port ${frontendPort} --strictPort`
    : `cd "${frontendCwd}" && npm run dev -- --port ${frontendPort} --strictPort`
  const tagger = createScopeTagger(FRONTEND_SCOPES)
  const p = spawn(cmd, {
    cwd: __dirname,
    stdio: 'pipe',
    shell: true,
    env: process.env,
  })
  p.stdout.on('data', tagger)
  p.stderr.on('data', tagger)
  p.on('exit', (code) => {
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.yellow}Vite${C.reset}${C.grey} 已退出 (code ${code})${C.reset}`)
    drawPrompt()
    if (frontend === p) { frontend = null; frontendStartedAt = null; frontendPid = null }
  })
  frontend = p
  frontendStartedAt = Date.now()
  frontendPid = p.pid
  return p
}

function startPostgres() {
  const pgDataDir = process.env.PG_DATA_DIR
  if (!pgDataDir) {
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.magenta} PG ${C.reset}${C.red}  未配置 PG_DATA_DIR 环境变量，跳过启动${C.reset}`)
    drawPrompt()
    return null
  }
  if (!existsSync(pgDataDir)) {
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.magenta} PG ${C.reset}${C.red}  PG_DATA_DIR 路径不存在: ${pgDataDir}${C.reset}`)
    drawPrompt()
    return null
  }
  const tagger = createScopeTagger(PG_SCOPES)
  const p = spawn('pg_ctl', ['start', '-D', pgDataDir, '-l', path.join(pgDataDir, 'pg.log')], {
    stdio: 'pipe',
    shell: isWin,
    env: process.env,
  })
  p.stdout.on('data', tagger)
  p.stderr.on('data', tagger)
  p.on('exit', (code) => {
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.magenta} PG ${C.reset}${C.grey} 已退出 (code ${code})${C.reset}`)
    drawPrompt()
    if (pgProcess === p) { pgProcess = null; pgStartedAt = null; pgPid = null }
  })
  pgProcess = p
  pgStartedAt = Date.now()
  pgPid = p.pid
  console.log(`${C.grey}[${ts()}]${C.reset} ${C.magenta} PG ${C.reset}${C.grey} 正在启动 (数据目录: ${pgDataDir})...${C.reset}`)
  return p
}

function killProcess(p, name) {
  if (!p || p.killed) return
  try {
    if (isWin) {
      try { execSync(`taskkill /F /T /PID ${p.pid} 2>nul`) } catch {}
    } else {
      p.kill('SIGTERM')
    }
  } catch {}
  clearPrompt()
  console.log(`${C.grey}[${ts()}] 已发送终止信号 → ${name}${C.reset}`)
}

function killPort(port, name) {
  if (!port) return
  let killed = 0
  try {
    if (isWin) {
      const out = execSync(`netstat -ano | findstr ":${port} "`, { encoding: 'utf8' })
      const pids = new Set()
      for (const line of out.split('\n')) {
        const m = line.trim().match(/(\d+)\s*$/)
        if (m && m[1] !== '0' && m[1] !== '4') pids.add(m[1])
      }
      for (const pid of pids) {
        try { execSync(`taskkill /F /PID ${pid} 2>nul`); killed++ } catch {}
      }
    } else {
      try { execSync(`lsof -ti:${port} | xargs kill -9 2>/dev/null`); killed++ } catch {}
    }
  } catch {}
  if (killed > 0) {
    console.log(`${C.grey}[${ts()}] 已释放端口 ${port} → ${name}${C.reset}`)
  }
}

function restartBackend() {
  clearPrompt()
  console.log(`${C.grey}[${ts()}] 正在重启服务...${C.reset}`)
  if (backend) killProcess(backend, '服务')
  setTimeout(() => { startBackend() }, 800)
}

function restartFrontend() {
  clearPrompt()
  console.log(`${C.grey}[${ts()}] 正在重启 Vite...${C.reset}`)
  if (frontend) killProcess(frontend, 'Vite')
  setTimeout(() => { startFrontend() }, 800)
}

function restartAll() {
  clearPrompt()
  console.log(`${C.grey}[${ts()}] 正在重启全部...${C.reset}`)
  if (backend) killProcess(backend, '服务')
  if (frontend) killProcess(frontend, 'Vite')
  if (pgProcess) killProcess(pgProcess, 'PG')
  setTimeout(() => { startBackend(); setTimeout(() => startFrontend(), 1500) }, 800)
}

function shutdown() {
  clearPrompt()
  console.log(`${C.grey}[${ts()}] 正在停止所有服务...${C.reset}`)
  if (backend) killProcess(backend, '服务')
  if (frontend) killProcess(frontend, 'Vite')
  if (pgProcess) killProcess(pgProcess, 'PG')
  killPort(backendPort, '服务端口')
  killPort(frontendPort, 'Vite 端口')
  if (pgPort) killPort(pgPort, 'PG 端口')
  setTimeout(() => { console.log(`${C.grey}[${ts()}] 再见${C.reset}`); process.exit(0) }, 2000)
}

function killSingle(target) {
  clearPrompt()
  if (target === 'backend' && backend) {
    console.log(`${C.grey}[${ts()}] 正在停止服务 (PID ${backendPid || backend.pid})...${C.reset}`)
    killProcess(backend, '服务')
    console.log(`${C.grey}[${ts()}] 服务已停止 — ${C.green}start backend${C.reset}${C.grey} 重新启动${C.reset}`)
  } else if (target === 'frontend' && frontend) {
    console.log(`${C.grey}[${ts()}] 正在停止 Vite (PID ${frontendPid || frontend.pid})...${C.reset}`)
    killProcess(frontend, 'Vite')
    console.log(`${C.grey}[${ts()}] Vite 已停止 — ${C.green}start frontend${C.reset}${C.grey} 重新启动${C.reset}`)
  } else if (target === 'pg' && pgProcess) {
    console.log(`${C.grey}[${ts()}] 正在停止 PG (PID ${pgPid || pgProcess.pid})...${C.reset}`)
    killProcess(pgProcess, 'PG')
    killPort(pgPort, 'PG')
    console.log(`${C.grey}[${ts()}] PG 已停止 — ${C.green}start pg${C.reset}${C.grey} 重新启动${C.reset}`)
  } else {
    console.log(`${C.red}  ${target} 未在运行${C.reset}`)
  }
  drawPrompt()
}

function startSingle(target) {
  clearPrompt()
  if (target === 'backend' && !backend) {
    console.log(`${C.grey}[${ts()}] 正在启动服务...${C.reset}`)
    startBackend()
  } else if (target === 'frontend' && !frontend) {
    console.log(`${C.grey}[${ts()}] 正在启动 Vite...${C.reset}`)
    startFrontend()
  } else if (target === 'pg' && !pgProcess) {
    console.log(`${C.grey}[${ts()}] 正在启动 PG...${C.reset}`)
    startPostgres()
  } else if (target === 'backend' || target === 'frontend' || target === 'pg') {
    console.log(`${C.red}  ${target} 已在运行中${C.reset}`)
  } else {
    console.log(`${C.red}  用法: start <backend|frontend|pg>${C.reset}`)
  }
  drawPrompt()
}

function showStatus() {
  clearPrompt()
  function uptime(started) { return started ? `${Math.floor((Date.now() - started) / 1000)}s` : '—' }
  function icon(p) { return p && !p.killed ? `${C.green}●${C.reset}` : `${C.red}○${C.reset}` }
  console.log(`  ${C.white}┌─── 服务状态 ────────────────────────────────────────────┐${C.reset}`)
  console.log(`  ${C.white}│${C.reset}  ${icon(backend)} ${C.cyan}服务${C.reset}   PID ${String(backendPid || '—').padEnd(7)} 端口 ${String(backendPort).padEnd(5)} 运行 ${uptime(backendStartedAt)}`)
  console.log(`  ${C.white}│${C.reset}  ${icon(frontend)} ${C.yellow}Vite${C.reset}   PID ${String(frontendPid || '—').padEnd(7)} 端口 ${String(frontendPort).padEnd(5)} 运行 ${uptime(frontendStartedAt)}`)
  console.log(`  ${C.white}│${C.reset}  ${icon(pgProcess)} ${C.magenta} PG${C.reset}     PID ${String(pgPid || '—').padEnd(7)} 端口 ${String(pgPort || '—').padEnd(5)} 运行 ${uptime(pgStartedAt)}`)
  console.log(`  ${C.white}└─────────────────────────────────────────────────────────┘${C.reset}`)
  drawPrompt()
}

// ═══════════════════════════════════════════════════════════════════════════
// Port inspection / mutation
// ═══════════════════════════════════════════════════════════════════════════

function showPorts() {
  clearPrompt()
  console.log(`  ${C.white}┌─── 端口 ────────────────────────────────────────┐${C.reset}`)
  console.log(`  ${C.white}│${C.reset}  ${C.cyan}服务${C.reset}  ${protocol}://localhost:${String(backendPort).padEnd(5)}                     ${C.white}│${C.reset}`)
  console.log(`  ${C.white}│${C.reset}  ${C.yellow}Vite${C.reset}  ${protocol}://localhost:${String(frontendPort).padEnd(5)}                     ${C.white}│${C.reset}`)
  if (pgPort) console.log(`  ${C.white}│${C.reset}  ${C.magenta} PG${C.reset}    postgresql://localhost:${String(pgPort).padEnd(5)}                ${C.white}│${C.reset}`)
  console.log(`  ${C.white}│${C.reset}                                             ${C.white}│${C.reset}`)
  console.log(`  ${C.white}│${C.reset}  ${C.grey}修改:${C.reset} ${C.green}port set <服务|Vite|pg> <端口>${C.reset}              ${C.white}│${C.reset}`)
  console.log(`  ${C.white}│${C.reset}  ${C.grey}扫描:${C.reset} ${C.green}port scan [起始端口] [数量]${C.reset}                ${C.white}│${C.reset}`)
  console.log(`  ${C.white}└───────────────────────────────────────────────────────┘${C.reset}`)
  drawPrompt()
}

/** port set <target> <newPort> — 运行时修改端口并自动重启对应服务 */
function portSet(target, newPort) {
  const p = parseInt(newPort)
  if (isNaN(p) || p < 1024 || p > 65535) {
    console.log(`${C.red}  无效端口: ${newPort}  — 端口范围 1024–65535${C.reset}`)
    drawPrompt()
    return
  }
  if (knownPortsReason(p)) {
    console.log(`${C.red}  端口 ${p} 属于常用端口 (${knownPortsReason(p)})，拒绝修改${C.reset}`)
    drawPrompt()
    return
  }
  if (!portFree(p)) {
    console.log(`${C.red}  端口 ${p} 已被占用${C.reset}`)
    drawPrompt()
    return
  }

  if (target === 'backend') {
    const wasRunning = backend !== null
    if (wasRunning) killProcess(backend, '服务')
    const old = backendPort
    backendPort = p
    syncPortEnv()
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.cyan}服务${C.reset}${C.grey} 端口 ${old} → ${C.green}${p}${C.reset}`)
    if (wasRunning) {
      console.log(`${C.grey}[${ts()}] 正在用新端口重启服务...${C.reset}`)
      setTimeout(() => { startBackend() }, 800)
    } else {
      drawPrompt()
    }
  } else if (target === 'frontend') {
    const wasRunning = frontend !== null
    if (wasRunning) killProcess(frontend, 'Vite')
    const old = frontendPort
    frontendPort = p
    syncPortEnv()
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.yellow}Vite${C.reset}${C.grey} 端口 ${old} → ${C.green}${p}${C.reset}`)
    if (wasRunning) {
      console.log(`${C.grey}[${ts()}] 正在用新端口重启 Vite...${C.reset}`)
      setTimeout(() => { startFrontend() }, 800)
    } else {
      drawPrompt()
    }
  } else if (target === 'pg') {
    const old = pgPort
    pgPort = p
    if (pgProcess) killProcess(pgProcess, 'PG')
    console.log(`${C.grey}[${ts()}]${C.reset} ${C.magenta} PG${C.reset}${C.grey} 端口 ${old} → ${C.green}${p}${C.reset}`)
    drawPrompt()
  } else {
    console.log(`${C.red}  用法: port set <backend|frontend|pg> <端口>${C.reset}`)
    drawPrompt()
  }
}

/** Return a human-readable reason when a port is in or near the well-known set, or null. */
function knownPortsReason(p) {
  const nearby = [
    [20, 23, 'FTP/SSH/Telnet'],
    [25, 25, 'SMTP'],
    [53, 53, 'DNS'],
    [67, 69, 'DHCP/TFTP'],
    [80, 80, 'HTTP'],
    [110, 110, 'POP3'],
    [135, 139, 'NetBIOS'],
    [143, 143, 'IMAP'],
    [389, 389, 'LDAP'],
    [443, 443, 'HTTPS'],
    [445, 445, 'SMB'],
    [465, 587, 'SMTP(S)'],
    [993, 995, 'IMAP/POP3(S)'],
    [1080, 1080, 'SOCKS Proxy'],
    [1433, 1433, 'MSSQL'],
    [1521, 1521, 'Oracle'],
    [1723, 1723, 'PPTP VPN'],
    [3306, 3306, 'MySQL/MariaDB'],
    [3389, 3389, 'RDP'],
    [4444, 4444, 'Metasploit (常用攻击端口)'],
    [5432, 5432, 'PostgreSQL'],
    [5672, 5672, 'RabbitMQ'],
    [5900, 5900, 'VNC'],
    [6379, 6379, 'Redis'],
    [7687, 7687, 'Neo4j Bolt'],
    [8000, 8000, '常用开发端口'],
    [8080, 8080, 'HTTP 代理'],
    [8443, 8443, 'HTTPS 代理'],
    [8888, 8888, 'Jupyter'],
    [9000, 9000, 'PHP-FPM/MinIO'],
    [9092, 9092, 'Kafka'],
    [9200, 9300, 'Elasticsearch'],
    [11211, 11211, 'Memcached'],
    [27017, 28017, 'MongoDB'],
  ]
  // Also check exact match in KNOWN_PORTS
  if (KNOWN_PORTS.has(p)) {
    for (const [lo, hi, reason] of nearby) {
      if (p >= lo && p <= hi) return reason
    }
    return 'IANA 常规端口'
  }
  return null
}

function showPortScan(start, count) {
  clearPrompt()
  const from = parseInt(start) || backendPort + 1
  const n = Math.min(parseInt(count) || 10, 50)
  if (from < 1024) { console.log(`${C.red}  起始端口不能低于 1024（privileged ports）${C.reset}`); drawPrompt(); return }

  const results = portScan(from, n)
  if (results.length === 0) {
    console.log(`${C.red}  从 ${from} 开始未找到空闲端口${C.reset}`)
    drawPrompt()
    return
  }

  console.log(`  ${C.white}┌─── 空闲端口扫描 (起始: ${from}, 找到: ${results.length}) ────────────────────────┐${C.reset}`)
  // Print in columns of 5
  for (let i = 0; i < results.length; i += 5) {
    const row = results.slice(i, i + 5).map(p => {
      const stillFree = portFree(p)
      const color = stillFree ? C.green : C.red
      return `${color}${String(p).padStart(5)}${C.reset}`
    }).join('  ')
    console.log(`  ${C.white}│${C.reset}  ${row}  ${C.white}│${C.reset}`)
  }
  console.log(`  ${C.white}│${C.reset}  ${C.grey}${C.green}绿色${C.reset}${C.grey} = 空闲   ${C.red}红色${C.reset}${C.grey} = 已被占用 (扫描后变化)                                             ${C.white}│${C.reset}`)
  console.log(`  ${C.white}└──────────────────────────────────────────────────────────────────────────┘${C.reset}`)
  console.log(`  ${C.grey}  提示: 用 ${C.green}port set <服务|Vite|pg> <端口>${C.reset}${C.grey} 绑定到空闲端口${C.reset}`)
  drawPrompt()
}

function clearScreen() {
  process.stdout.write('\x1b[2J\x1b[H')
  drawPrompt()
}

function showHelp() {
  clearPrompt()
  const G = C.green, W = C.white, R = C.reset, g = C.grey

  // Visible length — ANSI codes don't take screen space
  function vlen(s) { return stripAnsi(String(s)).length }

  // Pad a string so its visible width equals `w`. Handles strings with ANSI codes.
  function vpad(s, w) {
    const need = w - vlen(s)
    return need > 0 ? s + ' '.repeat(need) : s
  }

  // Width constants for two-column layout
  const CMD_W = 31  // command column visible width
  const ALIAS_W = 16 // alias column visible width

  function row(cmd, alias, desc) {
    const left = `${G}${cmd}`
    const mid = alias ? `${g}${alias}` : ''
    console.log(`  ${W}│${R}  ${vpad(left, CMD_W)}${R}  ${vpad(mid, ALIAS_W)}${R}  ${desc}`)
  }
  function sep() { console.log(`  ${W}│${R}  ${g}${'─'.repeat(76)}${R}  ${W}│${R}`) }
  function section(title) { console.log(`  ${W}│${R}  ${W}▸ ${title}${R}`) }
  function blank() { console.log(`  ${W}│${R}`) }

  console.log(`  ${W}┌── 命令列表${' '.repeat(68)}┐${R}`)
  console.log(`  ${W}│${R}  ${g}语法: <命令> [参数]  或  <快捷别名>    (键入后 Enter 执行)${R}`)
  blank()

  // ── Process control ──
  section('▸ 进程管理')
  row('restart',                'r',           '重启全部 (服务 + Vite + PG)')
  row('restart backend',        'rb',          '只重启后端服务')
  row('restart frontend',       'rf',          '只重启 Vite 前端')
  blank()
  row('start  backend',         'sb',          '启动后端服务')
  row('start  frontend',        'sf',          '启动 Vite 前端')
  row('start  pg',              'sp',          '启动 PG (需 PG_DATA_DIR)')
  blank()
  row('kill   backend',         'kb',          '停止后端 (不自动重启)')
  row('kill   frontend',        'kf',          '停止 Vite (不自动重启)')
  row('kill   pg',              'kp',          '停止 PG  (不自动重启)')
  blank()
  row('status',                 'st / s',      '进程 PID · 端口 · 运行时长')

  blank(); sep(); blank()

  // ── Port management ──
  section('▸ 端口管理')
  row('port',                   '',            '查看当前端口')
  row('port set  服务 <N>',     '',            '修改后端端口并自动重启 (也接受 backend)')
  row('port set  Vite <N>',     '',            '修改前端端口并自动重启 (也接受 frontend)')
  row('port set  pg   <N>',     '',            '修改 PG 端口')
  row('port scan [起始] [数量]','',            '扫描空闲端口 (跳过 IANA 知名 + Hyper-V 排除)')

  blank(); sep(); blank()

  // ── Misc ──
  section('▸ 其他')
  row('clear',                   'cls',        '清屏')
  row('help',                    'h / ?',      '显示此帮助')
  row('quit',                    'q / exit / Ctrl+C',  '退出全部服务')

  blank(); sep(); blank()

  // ── Scope legend ──
  section('日志作用域图例')
  blank()
  console.log(`  ${W}│${R}  ${g}后端${R}   ${C.cyan}服务${R} │ ${C.yellow}HTTP${R} │ ${C.magenta}数据${R} │ ${C.white}迁移${R} │ ${C.green}认证${R} │ ${C.cyan}订单${R} │ ${C.white}任务${R} │ ${C.red}警告${R} │ ${C.red}错误${R}`)
  console.log(`  ${W}│${R}  ${g}Vite${R}   ${C.yellow}Vite${R} │ ${C.green}HMR${R} │ ${C.red}错误${R} │ ${C.yellow}警告${R}`)
  console.log(`  ${W}│${R}  ${g}PG${R}     ${C.magenta}PG${R} │ ${C.magenta}PG启${R} │ ${C.cyan}PG查${R} │ ${C.red}PG错${R} │ ${C.yellow}PG警${R} │ ${C.green}PG存${R}`)

  console.log(`  ${W}└${'─'.repeat(78)}┘${R}`)
  drawPrompt()
}

// ── Start ──
startBackend()
startFrontend()
if (process.env.PG_DATA_DIR) startPostgres()

// ═══════════════════════════════════════════════════════════════════════════
// CLI command parser
// ═══════════════════════════════════════════════════════════════════════════

function execCommand(cmd) {
  const parts = cmd.trim().split(/\s+/)
  if (parts.length === 0 || parts[0] === '') return
  const c = parts[0].toLowerCase()

  // restart [all|backend|frontend|pg]
  if (c === 'restart' || c === 'r') {
    const target = (parts[1] || 'all').toLowerCase()
    if (target === 'all' || target === 'a') { restartAll(); return }
    if (target === 'backend' || target === 'b' || target === 'be') { restartBackend(); return }
    if (target === 'frontend' || target === 'f' || target === 'fe') { restartFrontend(); return }
    if (target === 'pg' || target === 'p') {
      clearPrompt()
      console.log(`${C.grey}[${ts()}] PG 重启: 仅支持 kill pg + start pg 手动重启${C.reset}`)
      drawPrompt()
      return
    }
    console.log(`${C.red}  用法: restart [all|backend|frontend]${C.reset}`)
    drawPrompt()
    return
  }

  if (c === 'rb') { restartBackend(); return }
  if (c === 'rf') { restartFrontend(); return }

  // status / st / s
  if (c === 'status' || c === 'st' || c === 's') { showStatus(); return }

  // kill <backend|frontend|pg>
  if (c === 'kill' || c === 'k') {
    const target = (parts[1] || '').toLowerCase()
    if (!target) { console.log(`${C.red}  用法: kill <backend|frontend|pg>${C.reset}`); drawPrompt(); return }
    if (target === 'backend' || target === 'b') { killSingle('backend'); return }
    if (target === 'frontend' || target === 'f') { killSingle('frontend'); return }
    if (target === 'pg' || target === 'p') { killSingle('pg'); return }
    console.log(`${C.red}  用法: kill <backend|frontend|pg>${C.reset}`)
    drawPrompt()
    return
  }

  if (c === 'kb') { killSingle('backend'); return }
  if (c === 'kf') { killSingle('frontend'); return }
  if (c === 'kp') { killSingle('pg'); return }

  // start <backend|frontend|pg>
  if (c === 'start') {
    const target = (parts[1] || '').toLowerCase()
    if (!target) { console.log(`${C.red}  用法: start <backend|frontend|pg>${C.reset}`); drawPrompt(); return }
    if (target === 'backend' || target === 'b') { startSingle('backend'); return }
    if (target === 'frontend' || target === 'f') { startSingle('frontend'); return }
    if (target === 'pg' || target === 'p') { startSingle('pg'); return }
    console.log(`${C.red}  用法: start <backend|frontend|pg>${C.reset}`)
    drawPrompt()
    return
  }

  if (c === 'sb') { startSingle('backend'); return }
  if (c === 'sf') { startSingle('frontend'); return }
  if (c === 'sp') { startSingle('pg'); return }

  // port [set|scan] ...
  if (c === 'port' || c === 'p') {
    const sub = (parts[1] || '').toLowerCase()
    if (!sub) { showPorts(); return }

    // port set <target> <number>
    if (sub === 'set') {
      const target = (parts[2] || '').toLowerCase()
      const val = parts[3]
      if (!target || !val) {
        console.log(`${C.red}  用法: port set <服务|Vite|pg> <端口>${C.reset}`)
        console.log(`${C.grey}  示例: port set 服务 3001${C.reset}`)
        console.log(`${C.grey}  示例: port set Vite 5174${C.reset}`)
        drawPrompt()
        return
      }
      // Normalize Chinese / alias names
      const normalized = target === '服务' || target === 'server' ? 'backend'
        : target === '前端' || target === 'vite' ? 'frontend'
        : target
      if (normalized !== 'backend' && normalized !== 'frontend' && normalized !== 'pg') {
        console.log(`${C.red}  目标: 服务|Vite|pg (也接受中文 服务/前端)${C.reset}`)
        drawPrompt()
        return
      }
      clearPrompt()
      portSet(normalized, val)
      return
    }

    // port scan [start] [count]
    if (sub === 'scan') {
      const start = parts[2] || String(backendPort + 1)
      const count = parts[3] || '10'
      showPortScan(start, count)
      return
    }

    console.log(`${C.red}  用法: port [set|scan] ...  — 无子命令则显示当前端口${C.reset}`)
    drawPrompt()
    return
  }

  // clear / cls
  if (c === 'clear' || c === 'cls') { clearScreen(); return }

  // quit / exit / q / x
  if (c === 'quit' || c === 'exit' || c === 'q' || c === 'x') { shutdown(); return }

  // help / h / ?
  if (c === 'help' || c === 'h' || c === '?') { showHelp(); return }

  // Unknown
  clearPrompt()
  console.log(`${C.red}  未知命令: ${cmd}  — 输入 ${C.green}help${C.reset}${C.red} 查看可用命令${C.reset}`)
  drawPrompt()
}

// ═══════════════════════════════════════════════════════════════════════════
// Interactive input (line-buffered raw mode)
// ═══════════════════════════════════════════════════════════════════════════

if (process.stdin.isTTY) {
  process.stdin.setRawMode(true)
  process.stdin.resume()
  process.stdin.setEncoding('utf8')

  let inputBuffer = ''
  let cursorPos = 0

  function redrawLine() {
    readline.cursorTo(process.stdout, 0)
    readline.clearLine(process.stdout, 0)
    process.stdout.write(`${C.green}> ${C.reset}${inputBuffer}`)
    readline.cursorTo(process.stdout, 2 + cursorPos)
    promptShown = false
  }

  process.stdin.on('data', (key) => {
    if (key === '\x03') { process.stdout.write('\n'); shutdown(); return }

    if (key === '\r' || key === '\n') {
      process.stdout.write('\n')
      const cmd = inputBuffer.trim()
      inputBuffer = ''
      cursorPos = 0
      promptShown = false
      if (cmd) execCommand(cmd)
      else drawPrompt()
      return
    }

    if (key === '\x7f' || key === '\x08') {
      if (cursorPos > 0) {
        inputBuffer = inputBuffer.slice(0, cursorPos - 1) + inputBuffer.slice(cursorPos)
        cursorPos--
        redrawLine()
      }
      return
    }

    if (key === '\x1b') return

    if (key.length === 1 && key.charCodeAt(0) >= 32) {
      inputBuffer = inputBuffer.slice(0, cursorPos) + key + inputBuffer.slice(cursorPos)
      cursorPos++
      redrawLine()
      return
    }
  })
} else {
  console.log(`${C.grey}  提示: 在终端中直接运行可启用交互式命令行${C.reset}`)
  drawPrompt()
}

// ── Cleanup on exit ──
function cleanup() {
  if (process.stdin.isTTY) { try { process.stdin.setRawMode(false) } catch {} }
  try { if (backend) killProcess(backend, '服务') } catch {}
  try { if (frontend) killProcess(frontend, 'Vite') } catch {}
  try { if (pgProcess) killProcess(pgProcess, 'PG') } catch {}
}
process.on('exit', cleanup)
process.on('SIGTERM', () => { shutdown() })

process.on('uncaughtException', (err) => {
  if (err.code === 'EPIPE' || err.code === 'ECONNRESET') return
  console.error(`${C.red}[${ts()}] 致命错误: ${err.message}${C.reset}`)
  shutdown()
})

drawPrompt()
