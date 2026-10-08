import { spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const projectRoot = path.resolve(scriptDir, '..')
const stateRoot = path.join(projectRoot, '.talos-runtime', 'agent-test')
const stateFile = path.join(stateRoot, 'session.json')
const keepState = process.argv.includes('--keep')
const force = process.argv.includes('--force')

function fail(message) {
  console.error(`[agent-test] ${message}`)
  process.exit(1)
}

async function reserveFreePort() {
  return await new Promise((resolve, reject) => {
    const server = net.createServer()
    server.unref()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      if (!address || typeof address === 'string') {
        server.close(() => reject(new Error('unable to allocate a TCP port')))
        return
      }
      const port = address.port
      server.close((error) => error ? reject(error) : resolve(port))
    })
  })
}

if (existsSync(stateFile)) {
  if (!force) {
    fail(`an agent test session marker already exists at ${stateFile}; stop it or rerun with --force`)
  }
  rmSync(stateFile, { force: true })
}

mkdirSync(stateRoot, { recursive: true })

const sessionId = `${new Date().toISOString().replace(/[:.]/g, '-')}-${randomBytes(4).toString('hex')}`
const sessionDir = path.join(stateRoot, 'sessions', sessionId)
mkdirSync(sessionDir, { recursive: true })

const backendPort = await reserveFreePort()
let frontendPort = await reserveFreePort()
while (frontendPort === backendPort) frontendPort = await reserveFreePort()

const username = `agent_e2e_${randomBytes(4).toString('hex')}`
const password = randomBytes(24).toString('base64url')
const dbPath = path.join(sessionDir, 'rental.db')
const baseUrl = `http://localhost:${frontendPort}`
const createdAt = new Date().toISOString()

const session = {
  schemaVersion: '1.0.0',
  purpose: 'local-agent-playwright',
  createdAt,
  expiresWhenProcessStops: !keepState,
  authority: 'tenant',
  tenantSlug: 'default',
  baseUrl,
  loginUrl: `${baseUrl}/login`,
  backendUrl: `http://127.0.0.1:${backendPort}`,
  username,
  password,
  dbPath,
}

writeFileSync(stateFile, `${JSON.stringify(session, null, 2)}\n`, { encoding: 'utf8', mode: 0o600 })

console.log('[agent-test] disposable session prepared')
console.log(`[agent-test] credentials: ${path.relative(projectRoot, stateFile)}`)
console.log(`[agent-test] login URL: ${session.loginUrl}`)
console.log(`[agent-test] username: ${username}`)
console.log('[agent-test] password is intentionally available only in the ignored local state file')
console.log('[agent-test] press Ctrl+C or enter q in the TALOS launcher to stop and clean the session')

const child = spawn(process.execPath, [
  path.join(projectRoot, 'start.js'),
  '--port', String(backendPort),
  '--frontend-port', String(frontendPort),
], {
  cwd: projectRoot,
  env: {
    ...process.env,
    NODE_ENV: 'development',
    DB_PATH: dbPath,
    AUTH_COOKIE_SECURE: 'false',
    AUTH_BOOTSTRAP_ON_START: 'true',
    AUTH_BOOTSTRAP_ADMIN_USERNAME: username,
    AUTH_BOOTSTRAP_ADMIN_PASSWORD: password,
    TALOS_AGENT_TEST_SESSION: '1',
  },
  stdio: 'inherit',
})

let cleaned = false
function cleanup() {
  if (cleaned) return
  cleaned = true
  if (keepState) {
    console.log(`[agent-test] --keep specified; state retained at ${sessionDir}`)
    return
  }
  rmSync(stateFile, { force: true })
  rmSync(sessionDir, { recursive: true, force: true })
  console.log('[agent-test] temporary credentials and database removed')
}

child.once('error', (error) => {
  cleanup()
  fail(`failed to start TALOS: ${error.message}`)
})

child.once('exit', (code, signal) => {
  cleanup()
  if (signal) {
    console.log(`[agent-test] launcher exited via ${signal}`)
    process.exit(0)
  }
  process.exit(code ?? 0)
})

process.once('SIGTERM', () => {
  if (!child.killed) child.kill('SIGTERM')
})
