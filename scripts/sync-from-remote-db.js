/**
 * sync-from-remote-db.js
 * Incremental upsert: remote-db/rental.db → local rental.db
 * Reads every business table from the remote SQLite file and upserts rows
 * into the local database in FK-safe order. Skips sessions & migrations.
 *
 * Usage: node scripts/sync-from-remote-db.js [--dry-run]
 */

if (process.env.TALOS_ALLOW_LEGACY_LOCAL_DB_SYNC !== '1') {
  console.error('Legacy local database synchronization is restricted. Set TALOS_ALLOW_LEGACY_LOCAL_DB_SYNC=1 only for an owner-approved local recovery task.');
  process.exit(1);
}

const path = require('path');
const crypto = require('crypto');
const Database = require('better-sqlite3');

const DRY_RUN = process.argv.includes('--dry-run');

const REMOTE_PATH = path.join(__dirname, '..', 'remote-db', 'rental.db');
const LOCAL_PATH = path.join(__dirname, '..', 'rental.db');

// Legacy local-recovery FK-safe order. No Axum sync route exists after SP-27.
const SYNC_ORDER = [
  'device_models',
  'warehouses',
  'devices',
  'pricing_configs',
  'model_base_prices',
  'warehouse_region_rules',
  'dynamic_daily_prices',
  'holiday_rules',
  'orders',
  'order_devices',
  'order_price_details',
  'audit_logs',
];

const SKIP_TABLES = new Set([
  'identities', 'tenant_memberships', 'platform_memberships',
  'platform_role_grants', 'auth_sessions', 'schema_migrations',
]);

function getTableColumns(db, table) {
  const rows = db.pragma(`table_info("${table}")`);
  return rows.map(r => r.name);
}

function getRowCount(db, table) {
  return db.prepare(`SELECT COUNT(*) FROM "${table}"`).pluck().get();
}

function main() {
  console.log(`Remote: ${REMOTE_PATH}`);
  console.log(`Local:  ${LOCAL_PATH}`);
  if (DRY_RUN) console.log('[DRY RUN] No writes will be committed.\n');

  const remote = new Database(REMOTE_PATH, { readonly: true });
  const local = new Database(LOCAL_PATH);

  // Safety: enable WAL + foreign keys on local
  local.pragma('journal_mode = WAL');
  local.pragma('foreign_keys = ON');

  const stats = [];

  for (const table of SYNC_ORDER) {
    // Check table exists in remote
    const remoteExists = remote.pragma(`table_info("${table}")`).length > 0;
    if (!remoteExists) {
      console.log(`[skip] ${table} — not found in remote`);
      continue;
    }
    if (SKIP_TABLES.has(table)) {
      console.log(`[skip] ${table} — excluded table`);
      continue;
    }

    // Check table exists in local
    const localExists = local.pragma(`table_info("${table}")`).length > 0;
    if (!localExists) {
      console.log(`[skip] ${table} — not found in local`);
      continue;
    }

    const remoteColumns = getTableColumns(remote, table);
    const localColumns = new Set(getTableColumns(local, table));

    // Only sync columns that exist in both (handles schema drift)
    const commonCols = remoteColumns.filter(c => localColumns.has(c));
    if (commonCols.length === 0) {
      console.log(`[skip] ${table} — no common columns`);
      continue;
    }

    const remoteCount = getRowCount(remote, table);
    const localBefore = getRowCount(local, table);

    if (remoteCount === 0) {
      stats.push({ table, remote: 0, localBefore, localAfter: localBefore, inserted: 0, updated: 0 });
      continue;
    }

    // Build column lists
    const colList = commonCols.map(c => `"${c}"`).join(', ');
    const placeholderList = commonCols.map(() => '?').join(', ');

    // Detect conflict column(s) from table schema
    // PREFER explicit UNIQUE constraint (business key) over PK — remote & local may
    // have different surrogate ids for the same business row.
    const uniqueIndexCols = (() => {
      const idxs = remote.pragma(`index_list("${table}")`);
      // origin='u' = explicit UNIQUE constraint; origin='pk' = PRIMARY KEY
      const uniqueIdx = idxs.find(r => r.unique && r.origin === 'u');
      if (!uniqueIdx) return [];
      const info = remote.pragma(`index_info(${uniqueIdx.name})`);
      return info.map(r => r.name).filter(c => commonCols.includes(c));
    })();

    const pkCols = commonCols.filter(c => {
      const info = remote.pragma(`table_info("${table}")`).find(r => r.name === c);
      return info && info.pk > 0;
    });

    // Prefer explicit UNIQUE constraint as the conflict target (business key),
    // fall back to PK, then 'id', then INSERT OR IGNORE.
    const conflictCols = uniqueIndexCols.length > 0 ? uniqueIndexCols
      : pkCols.length > 0 ? pkCols
      : commonCols.includes('id') ? ['id']
      : [];

    // When the conflict detection uses a business key (not PK), exclude the
    // surrogate id from the SET clause — remote & local may assign different
    // UUIDs to the same business row. Updating id would collide with other rows.
    const isPkConflict = conflictCols.length === pkCols.length &&
      conflictCols.every(c => pkCols.includes(c));
    const nonConflictCols = isPkConflict
      ? commonCols.filter(c => !conflictCols.includes(c))
      : commonCols.filter(c => !conflictCols.includes(c) && c !== 'id');

    let sql;
    if (conflictCols.length > 0 && nonConflictCols.length > 0) {
      const conflictClause = conflictCols.map(c => `"${c}"`).join(', ');
      const setClause = nonConflictCols.map(c => `"${c}" = excluded."${c}"`).join(', ');
      sql = `INSERT INTO "${table}" (${colList}) VALUES (${placeholderList}) ON CONFLICT(${conflictClause}) DO UPDATE SET ${setClause}`;
    } else if (conflictCols.length > 0) {
      sql = `INSERT OR IGNORE INTO "${table}" (${colList}) VALUES (${placeholderList})`;
    } else {
      sql = `INSERT OR IGNORE INTO "${table}" (${colList}) VALUES (${placeholderList})`;
    }

    // Pre-remap surrogate ids that collide between remote & local when the
    // conflict detection uses a business key (not PK). ON CONFLICT on the
    // business key succeeds, but the subsequent id SET clause would collide
    // with a different local row that already owns that id.
    if (!isPkConflict && conflictCols.length > 0 && commonCols.includes('id')) {
      const localIds = new Set(local.prepare(`SELECT id FROM "${table}"`).pluck().all());
      const allLocalConflict = local.prepare(
        `SELECT ${conflictCols.map(c => `"${c}"`).join(', ')} FROM "${table}"`
      ).all();
      const conflictKeySet = new Set(
        allLocalConflict.map(r => JSON.stringify(conflictCols.map(c => r[c])))
      );
      const allRemote = remote.prepare(`SELECT * FROM "${table}"`).all();
      const collisionRows = allRemote.filter(
        r => !conflictKeySet.has(JSON.stringify(conflictCols.map(c => r[c]))) && localIds.has(r.id)
      );
      if (collisionRows.length > 0) {
        const insertPre = local.prepare(
          `INSERT OR IGNORE INTO "${table}" (${colList}) VALUES (${placeholderList})`
        );
        local.pragma('foreign_keys = OFF');
        const preTx = local.transaction(() => {
          for (const row of collisionRows) {
            const newRow = { ...row, id: crypto.randomUUID() };
            const vals = commonCols.map(c => newRow[c] === undefined || newRow[c] === null ? null : newRow[c]);
            insertPre.run(...vals);
          }
        });
        preTx();
        local.pragma('foreign_keys = ON');
      }
    }

    console.log(`[sync] ${table}: ${remoteCount} remote rows → upserting...`);

    if (DRY_RUN) {
      stats.push({ table, remote: remoteCount, localBefore, localAfter: localBefore, inserted: 0, updated: 0, dry: true });
      continue;
    }

    // Read + upsert in batches of 500
    const BATCH = 500;
    let processed = 0;

    const readStmt = remote.prepare(`SELECT ${colList} FROM "${table}"`);
    const rows = readStmt.all();

    const upsert = local.prepare(sql);
    const upsertMany = local.transaction((batch) => {
      for (const row of batch) {
        const values = commonCols.map(c => (row[c] === undefined || row[c] === null ? null : row[c]));
        upsert.run(...values);
      }
    });

    for (let i = 0; i < rows.length; i += BATCH) {
      const batch = rows.slice(i, i + BATCH);
      upsertMany(batch);
      processed += batch.length;
    }

    const localAfter = getRowCount(local, table);
    const inserted = Math.max(0, localAfter - localBefore);
    const updated = processed - inserted;

    stats.push({ table, remote: remoteCount, localBefore, localAfter, inserted, updated });
    console.log(`  → ${inserted} inserted, ${updated} updated, ${localAfter} total`);
  }

  // Summary
  console.log('\n' + '='.repeat(60));
  console.log('SYNC SUMMARY');
  console.log('='.repeat(60));
  console.log('Table'.padEnd(30) + 'Remote'.padStart(8) + 'Inserted'.padStart(10) + 'Updated'.padStart(10));
  console.log('-'.repeat(58));
  for (const s of stats) {
    console.log(
      s.table.padEnd(30) +
      String(s.remote).padStart(8) +
      String(s.inserted || 0).padStart(10) +
      String(s.updated || 0).padStart(10)
    );
  }
  console.log('-'.repeat(58));

  remote.close();
  local.close();

  if (DRY_RUN) console.log('\n[DRY RUN] No changes were committed.');
  else console.log('\nSync complete.');
}

main();
