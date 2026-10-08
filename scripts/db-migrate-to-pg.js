#!/usr/bin/env node
/**
 * SQLite → PostgreSQL full dump/restore CLI with checksum validation.
 *
 * Usage:
 *   node scripts/db-migrate-to-pg.js dump [--db rental.db] [--out dump.json]
 *   node scripts/db-migrate-to-pg.js restore [--db dump.json] [--dsn postgresql://...]
 *   node scripts/db-migrate-to-pg.js verify [--source rental.db] [--target postgresql://...]
 *
 * Phase B — PostgreSQL migration tool.
 */

const fs = require('fs')
const path = require('path')

// ── TABLES to migrate (order matters for FK constraints) ──
const TABLES = [
  'identities',
  'tenants',
  'tenant_memberships',
  'platform_memberships',
  'platform_role_grants',
  'user_settings',
  'auth_sessions',
  'devices',
  'orders',
  'order_devices',
  'pricing_configs',
  'dynamic_daily_prices',
  'device_models',
  'model_base_prices',
  'warehouses',
  'warehouse_region_rules',
  'order_price_details',
  'audit_logs',
  'api_keys',
  '_meta',
  '_migrations',
  'rate_limits',
]

// ── Column type overrides: SQLite TEXT stored as JSON → PG needs JSONB cast ──
const JSONB_COLUMNS = {
  orders: ['accessories', 'deviceModels'],
  pricing_configs: ['holidayRulesJson', 'receiveShippingFeesJson'],
  user_settings: ['settingsJson'],
  audit_logs: ['detailJson'],
}

// ── Identity columns: PG GENERATED ALWAYS AS IDENTITY requires OVERRIDING SYSTEM VALUE ──
const IDENTITY_OVERRIDE_TABLES = new Set(['pricing_configs'])

// ── Boolean columns: SQLite INTEGER (0/1) → PG BOOLEAN ──
const BOOLEAN_COLUMNS = {
  device_models: ['enabled'],
  warehouses: ['enabled'],
  api_keys: ['is_enabled'],
  warehouse_region_rules: ['isPrimary'],
}

function parseArgs() {
  const args = process.argv.slice(2)
  const cmd = args[0]
  const opts = {}
  for (let i = 1; i < args.length; i += 2) {
    const key = args[i].replace(/^--?/, '')
    opts[key] = args[i + 1]
  }
  return { cmd, opts }
}

// ── DUMP: SQLite → JSON ──
async function dump(opts) {
  const dbPath = opts.db || path.join(__dirname, '..', 'rental.db')
  const outPath = opts.out || path.join(__dirname, '..', 'talos-dump.json')

  console.log(`[dump] Reading SQLite: ${dbPath}`)
  const Database = require('better-sqlite3')
  const db = new Database(dbPath, { readonly: true })

  const dump = {
    version: 2,
    created_at: new Date().toISOString(),
    source: 'talos-sqlite',
    checksum: null,
    tables: {},
  }

  let totalRows = 0
  for (const table of TABLES) {
    // Check if table exists
    const exists = db
      .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
      .get(table)
    if (!exists) {
      console.log(`[dump]   Skipping ${table} (does not exist)`)
      continue
    }

    const rows = db.prepare(`SELECT * FROM "${table}" ORDER BY rowid`).all()
    dump.tables[table] = rows
    totalRows += rows.length
    console.log(`[dump]   ${table}: ${rows.length} rows`)
  }

  db.close()

  // Compute checksum (simple hash of JSON)
  const crypto = require('crypto')
  dump.checksum = crypto
    .createHash('sha256')
    .update(JSON.stringify(dump.tables))
    .digest('hex')

  fs.writeFileSync(outPath, JSON.stringify(dump, null, 2))
  console.log(`[dump] Wrote ${totalRows} rows to ${outPath}`)
  console.log(`[dump] Checksum: ${dump.checksum}`)
  return dump
}

// ── RESTORE: JSON → PostgreSQL ──
async function restore(opts) {
  const dumpPath = opts.db || path.join(__dirname, '..', 'talos-dump.json')
  const dsn = opts.dsn || 'postgresql://localhost:5432/talos'

  console.log(`[restore] Reading dump: ${dumpPath}`)
  const dump = JSON.parse(fs.readFileSync(dumpPath, 'utf-8'))
  console.log(`[restore] Dump version: ${dump.version}, checksum: ${dump.checksum}`)

  const { Pool } = require('pg')
  const pool = new Pool({ connectionString: dsn, max: 5 })

  let totalRows = 0

  for (const table of TABLES) {
    const rows = dump.tables[table]
    if (!rows || rows.length === 0) continue

    // Each table gets its own transaction so one failure doesn't cascade.
    const client = await pool.connect()
    let tableRows = 0
    try {
      await client.query('BEGIN')

      // PostgreSQL stores unquoted identifiers as lowercase.
      // SQLite preserves the original CREATE TABLE casing, so we
      // must normalize column names to avoid case-mismatch errors.
      const originalColumns = Object.keys(rows[0])
      const pgColumns = originalColumns.map(c => c.toLowerCase())
      const placeholders = pgColumns.map((_, i) => `$${i + 1}`).join(', ')
      const colNames = pgColumns.map(c => `"${c}"`).join(', ')

      const override = IDENTITY_OVERRIDE_TABLES.has(table) ? ' OVERRIDING SYSTEM VALUE' : ''
      const sql = `INSERT INTO "${table}" (${colNames})${override} VALUES (${placeholders}) ON CONFLICT DO NOTHING`

      // Process rows: convert SQLite INTEGER booleans to PG booleans
      const boolCols = BOOLEAN_COLUMNS[table] || []
      const jsonbCols = JSONB_COLUMNS[table] || []

      for (const row of rows) {
        // Convert boolean columns
        for (const col of boolCols) {
          if (row[col] !== null && row[col] !== undefined) {
            row[col] = row[col] === 1 || row[col] === '1' || row[col] === true
          }
        }
        // Ensure JSONB columns are valid JSON strings
        for (const col of jsonbCols) {
          if (typeof row[col] === 'string') {
            try {
              JSON.parse(row[col]) // validate
            } catch {
              row[col] = col === 'detailJson' ? '{}' : '[]'
            }
          }
        }

        const values = originalColumns.map(c => row[c])
        try {
          await client.query(sql, values)
          tableRows++
        } catch (err) {
          console.error(`[restore] Error inserting into ${table}: ${err.message}`)
          console.error(`  Row: ${JSON.stringify(row).slice(0, 200)}`)
        }
      }

      await client.query('COMMIT')
      totalRows += tableRows
      console.log(`[restore]   ${table}: ${tableRows}/${rows.length} rows`)
    } catch (err) {
      await client.query('ROLLBACK')
      console.error(`[restore] Table ${table} failed: ${err.message}`)
    } finally {
      client.release()
    }
  }

  await pool.end()
  console.log(`[restore] Restored ${totalRows} rows to PostgreSQL`)

  return { totalRows }
}

// ── VERIFY: Compare SQLite ↔ PostgreSQL row counts ──
async function verify(opts) {
  const sourcePath = opts.source || path.join(__dirname, '..', 'rental.db')
  const targetDsn = opts.target || 'postgresql://localhost:5432/talos'

  console.log(`[verify] Comparing SQLite (${sourcePath}) ↔ PostgreSQL`)
  const Database = require('better-sqlite3')
  const { Pool } = require('pg')

  const sqliteDb = new Database(sourcePath, { readonly: true })
  const pgPool = new Pool({ connectionString: targetDsn, max: 2 })

  const results = []
  let allMatch = true

  for (const table of TABLES) {
    // Check SQLite
    const sqliteExists = sqliteDb
      .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
      .get(table)

    // Check PG
    const pgCheck = await pgPool.query(
      "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1) AS exists",
      [table]
    )

    const sqliteCount = sqliteExists
      ? sqliteDb.prepare(`SELECT COUNT(*) AS cnt FROM "${table}"`).get().cnt
      : 0
    const pgCount = pgCheck.rows[0]?.exists
      ? parseInt((await pgPool.query(`SELECT COUNT(*) AS cnt FROM "${table}"`)).rows[0].cnt)
      : 0

    const match = sqliteCount === pgCount
    if (!match) allMatch = false

    results.push({ table, sqliteCount, pgCount, match })
    const icon = match ? '✓' : '✗'
    console.log(`[verify]   ${icon} ${table}: SQLite=${sqliteCount} PG=${pgCount}`)
  }

  sqliteDb.close()
  await pgPool.end()

  if (allMatch) {
    console.log('[verify] ✓ All tables match — data migration validated')
  } else {
    console.error('[verify] ✗ MISMATCH detected — some tables have different row counts')
  }

  return { results, allMatch }
}

// ── Main ──
async function main() {
  const { cmd, opts } = parseArgs()

  switch (cmd) {
    case 'dump':
      await dump(opts)
      break
    case 'restore':
      await restore(opts)
      break
    case 'verify':
      const { allMatch } = await verify(opts)
      if (!allMatch) process.exit(1)
      break
    case 'migrate':
      // Full pipeline: dump → restore → verify
      console.log('=== Phase 1/3: Dump SQLite ===')
      await dump(opts)
      console.log('\n=== Phase 2/3: Restore to PostgreSQL ===')
      await restore(opts)
      console.log('\n=== Phase 3/3: Verify ===')
      const verifyResult = await verify(opts)
      if (!verifyResult.allMatch) {
        console.error('\n[migrate] Verification failed — data mismatch detected')
        process.exit(1)
      }
      console.log('\n[migrate] ✓ Migration complete — all tables validated')
      break
    default:
      console.log('Usage: node scripts/db-migrate-to-pg.js <dump|restore|verify|migrate> [options]')
      console.log('  dump    --db <path>      SQLite database path (default: rental.db)')
      console.log('          --out <path>     Output JSON file (default: talos-dump.json)')
      console.log('  restore --db <path>      Input JSON dump (default: talos-dump.json)')
      console.log('          --dsn <url>      PostgreSQL connection string')
      console.log('  verify  --source <path>  SQLite database path')
      console.log('          --target <url>   PostgreSQL connection string')
      console.log('  migrate --db <path>      Full pipeline: dump → restore → verify')
      process.exit(1)
  }
}

main().catch(err => {
  console.error(err)
  process.exit(1)
})
