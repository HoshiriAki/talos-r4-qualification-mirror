#!/usr/bin/env node
/**
 * 从生产服务器（jiaozizulin.hk）爬取订单和设备数据到本地数据库
 *
 * 用法：
 *   node scripts/scrape-server-data.mjs
 *
 * 环境变量：
 *   SCRAPE_SERVER  — 服务器地址（默认 https://jiaozizulin.hk）
 *   SCRAPE_USERNAME — 登录用户名
 *   SCRAPE_PASSWORD — 登录密码
 *   SCRAPE_INSECURE — 设为 1 跳过 TLS 证书验证（仅本地开发用）
 *
 * 数据获取：登录 → 分页拉取 /users 和 /devices → upsert 到本地 rental.db
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import crypto from 'node:crypto'

const __dirname = dirname(fileURLToPath(import.meta.url))
const ROOT = resolve(__dirname, '..')

if (process.env.TALOS_ALLOW_LEGACY_SCRAPE !== '1') {
  console.error('Legacy server scraping is restricted. Set TALOS_ALLOW_LEGACY_SCRAPE=1 only for an owner-approved local recovery task.')
  process.exit(1)
}

// ── Config ──
const SERVER = process.env.SCRAPE_SERVER || 'https://jiaozizulin.hk'
const USERNAME = process.env.SCRAPE_USERNAME || process.env.SCRAPE_USER || ''
const PASSWORD = process.env.SCRAPE_PASSWORD || process.env.SCRAPE_PASS || ''
const INSECURE = process.env.SCRAPE_INSECURE === '1'
const PAGE_SIZE = 100

const DB_PATH = resolve(ROOT, 'rental.db')

if (!USERNAME || !PASSWORD) {
  console.error('请设置环境变量 SCRAPE_USERNAME 和 SCRAPE_PASSWORD')
  console.error('  $env:SCRAPE_USERNAME="xxx"; $env:SCRAPE_PASSWORD="xxx"; node scripts/scrape-server-data.mjs')
  process.exit(1)
}

// ── HTTP helpers ──
const agent = INSECURE
  ? new (await import('node:https')).Agent({ rejectUnauthorized: false })
  : undefined

function buildOpts(method, body, cookie) {
  const headers = {
    'Content-Type': 'application/json',
    Accept: 'application/json',
  }
  if (cookie) headers.Cookie = cookie
  return { method, headers, body: body ? JSON.stringify(body) : undefined, agent }
}

async function fetchJson(url, opts = {}) {
  const isAbsolute = url.startsWith('http')
  const fullUrl = isAbsolute ? url : `${SERVER}${url}`
  const resp = await fetch(fullUrl, { ...opts, redirect: 'manual' })

  // Follow same-origin redirects only — protect session cookie from exfiltration
  if (resp.status >= 300 && resp.status < 400) {
    const location = resp.headers.get('location')
    if (!location) throw new Error(`Redirect (${resp.status}) without Location header`)
    const redirectUrl = new URL(isAbsolute || !location.startsWith('http') ? location : location)
    if (!redirectUrl.host) {
      // Relative path — safe, same host
      return fetchJson(redirectUrl.pathname + redirectUrl.search, opts)
    }
    const serverHost = new URL(SERVER).hostname
    if (redirectUrl.hostname !== serverHost) {
      throw new Error(`Refusing to follow redirect to untrusted host: ${redirectUrl.hostname}`)
    }
    return fetchJson(redirectUrl.href, opts)
  }
  const text = await resp.text()
  let data
  try { data = JSON.parse(text) } catch {
    // Some endpoints return empty body on success
    data = {}
  }
  // Return raw headers for cookie extraction
  return { status: resp.status, data, headers: resp.headers, ok: resp.ok }
}

// ── 1. Login ──
console.log(`[scrape] 登录 ${SERVER}/auth/login ...`)
const loginResult = await fetchJson('/auth/login', buildOpts('POST', { username: USERNAME, password: PASSWORD }))

if (!loginResult.ok) {
  console.error(`[scrape] ✗ 登录失败 (${loginResult.status})`)
  process.exit(1)
}

const setCookie = loginResult.headers.get('set-cookie')
if (!setCookie) {
  console.error('[scrape] ✗ 服务器未返回 session cookie')
  process.exit(1)
}

const match = setCookie.match(/talos_session=([^;]+)/)
if (!match) {
  console.error('[scrape] ✗ 无法解析 session cookie')
  process.exit(1)
}

const cookie = `talos_session=${match[1]}`
console.log(`[scrape] ✓ 登录成功`)

// ── 2. Fetch all pages ──
async function fetchAllPages(path, label) {
  const rows = []
  let page = 1
  while (true) {
    process.stdout.write(`\r[scrape] ${label} 第 ${page} 页...`)
    const opts = buildOpts('GET', null, cookie)
    const { data, ok, status } = await fetchJson(`${path}?page=${page}&pageSize=${PAGE_SIZE}`, opts)
    if (!ok) {
      console.log(`\n[scrape] ⚠ ${label} 第 ${page} 页返回 ${status}`)
      break
    }
    const items = data.users ?? data.devices ?? []
    if (!Array.isArray(items) || items.length === 0) break
    rows.push(...items)
    const totalPages = data.pagination?.totalPages || 0
    if (page >= totalPages) break
    page++
  }
  console.log(`\r[scrape] ${label} 完成 — 共 ${rows.length} 行`)
  return rows
}

const orders = await fetchAllPages('/users', '订单')
const devices = await fetchAllPages('/devices', '设备')

// ── 3. Backup local DB ──
try {
  const src = readFileSync(DB_PATH)
  const backupPath = DB_PATH.replace('.db', `.backup-${Date.now()}.db`)
  writeFileSync(backupPath, src)
  console.log(`[scrape] 已备份本地数据库 → ${path.basename(backupPath)}`)
} catch (e) {
  console.log(`[scrape] 本地数据库不存在，跳过备份`)
}

// ── 4. Open local DB ──
const { default: Database } = await import('better-sqlite3')
const db = new Database(DB_PATH)
db.pragma('journal_mode = WAL')
db.pragma('foreign_keys = OFF')

// ── 5. Upsert helpers ──
function getColumns(table) {
  try {
    return db.prepare(`PRAGMA table_info("${table}")`).all().map(r => ({ name: r.name, notnull: r.notnull, dflt_value: r.dflt_value }))
  } catch { return [] }
}

function upsertTable(table, rows, idField = 'id') {
  if (!rows || rows.length === 0) return 0
  const cols = getColumns(table)
  if (cols.length === 0) return 0

  const first = rows[0]
  const sourceCols = Object.keys(first)
  const targetCols = cols.filter(c => sourceCols.includes(c.name))
  if (targetCols.length === 0) return 0

  const colNames = targetCols.map(c => `"${c.name}"`).join(', ')
  const placeholders = targetCols.map(() => '?').join(', ')

  // Use ON CONFLICT if the id column has a UNIQUE constraint
  const hasPk = cols.some(c => c.name === idField && c.notnull)
  const conflictClause = hasPk ? ` ON CONFLICT("${idField}") DO UPDATE SET ${targetCols.filter(c => c.name !== idField).map(c => `"${c.name}" = excluded."${c.name}"`).join(', ')}` : ''
  const sql = `INSERT INTO "${table}" (${colNames}) VALUES (${placeholders})${conflictClause}`

  const stmt = db.prepare(sql)
  const upsertMany = db.transaction((items) => {
    for (const row of items) {
      const values = targetCols.map(c => {
        const v = row[c.name]
        if (v === null || v === undefined) {
          if (c.notnull && c.dflt_value != null) return c.dflt_value
          return null
        }
        if (typeof v === 'object') return JSON.stringify(v)
        return v
      })
      try { stmt.run(...values) } catch { /* skip constraint violations */ }
    }
  })

  upsertMany(rows)
  return rows.length
}

// ── 6. Upsert data ──
const rawOrders = orders.map(o => {
  // Map order fields — the API returns the same shape as the Order interface
  const mapped = { ...o }
  // Device list: API returns deviceSerialNos array or devices array
  if (mapped.deviceSerialNos && !mapped.devices) {
    mapped.devices = mapped.deviceSerialNos
  }
  // Map pickupMethods
  if (typeof mapped.pickupMethods === 'string') {
    mapped.pickupMethods = JSON.stringify(mapped.pickupMethods.split(',').map(s => s.trim()))
  } else if (Array.isArray(mapped.pickupMethods)) {
    mapped.pickupMethods = JSON.stringify(mapped.pickupMethods)
  }
  // Map deviceModels
  if (mapped.deviceModels && typeof mapped.deviceModels === 'object') {
    mapped.deviceModels = JSON.stringify(mapped.deviceModels)
  }
  // Map accessories
  if (Array.isArray(mapped.accessories)) {
    mapped.accessories = JSON.stringify(mapped.accessories)
  }
  return mapped
})

// Delete old data and insert (simpler than upsert for orders — avoid FK issues)
db.prepare('DELETE FROM order_devices').run()
db.prepare('DELETE FROM orders').run()

upsertTable('orders', rawOrders, 'id')
console.log(`[scrape] 订单写入: ${rawOrders.length} 行`)

// Devices
db.prepare('DELETE FROM devices').run()
const rawDevices = devices.map(d => {
  const mapped = { ...d }
  return mapped
})
upsertTable('devices', rawDevices, 'serialNo')
console.log(`[scrape] 设备写入: ${rawDevices.length} 行`)

db.pragma('foreign_keys = ON')
db.close()

console.log(`\n[scrape] ✓ 完成 — 拉取 ${orders.length} 条订单, ${devices.length} 条设备`)
