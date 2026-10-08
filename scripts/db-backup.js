const path = require('path');
const fs = require('fs');
const Database = require('better-sqlite3');
const { DB_PATH } = require('../src/config/env');
const { getShanghaiDateTimeParts, pad2 } = require('../src/utils/time');

const BACKUP_DIR = path.join(process.cwd(), 'backups');
const MAX_BACKUPS = 7;

function ensureBackupDir() {
  if (!fs.existsSync(BACKUP_DIR)) {
    fs.mkdirSync(BACKUP_DIR, { recursive: true });
  }
}

function formatTimestamp() {
  const now = getShanghaiDateTimeParts(new Date());
  return `${now.year}-${pad2(now.month)}-${pad2(now.day)}-${pad2(now.hour)}${pad2(now.minute)}${pad2(now.second)}`;
}

function rotateBackups() {
  const files = fs.readdirSync(BACKUP_DIR)
    .filter(f => f.startsWith('rental-') && f.endsWith('.db'))
    .map(f => ({ name: f, path: path.join(BACKUP_DIR, f), mtime: fs.statSync(path.join(BACKUP_DIR, f)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime);

  for (let i = MAX_BACKUPS; i < files.length; i++) {
    fs.unlinkSync(files[i].path);
    console.log(`[db-backup] rotated: ${files[i].name}`);
  }
}

async function runBackup() {
  ensureBackupDir();
  const fileName = `rental-${formatTimestamp()}.db`;
  const destPath = path.join(BACKUP_DIR, fileName);

  try {
    const src = new Database(DB_PATH, { readonly: true });
    await src.backup(destPath);
    src.close();
    console.log(`[db-backup] saved: ${fileName}`);
    rotateBackups();
    return { ok: true, file: fileName };
  } catch (err) {
    console.error(`[db-backup] failed: ${err.message}`);
    return { ok: false, error: err.message };
  }
}

// Run directly
if (require.main === module) {
  runBackup().then(result => process.exit(result.ok ? 0 : 1));
}

module.exports = { runBackup };
