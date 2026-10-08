use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::fs;
use std::path::Path;

/// Detect and handle replaced database files.
/// When rental.db is swapped but old -wal/-shm companion files remain,
/// SQLite may try to apply stale WAL entries → corruption or errors.
/// Probe the journal mode: if not WAL, delete orphaned companions.
/// If WAL, attempt checkpoint; on failure, companions will be recreated.
fn auto_detect_and_cleanup(db_path: &str) {
    let wal_path = format!("{}-wal", db_path);
    let shm_path = format!("{}-shm", db_path);

    if !Path::new(db_path).exists() {
        return;
    }

    let wal_exists = Path::new(&wal_path).exists();
    let shm_exists = Path::new(&shm_path).exists();
    if !wal_exists && !shm_exists {
        return;
    }

    // Phase 1: probe journal mode (read-only)
    let mode =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()
            .and_then(|conn| {
                conn.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                    .ok()
            })
            .unwrap_or_else(|| "unknown".to_string());

    if mode != "wal" {
        tracing::info!(
            "Database is in {} mode — removing orphaned WAL/SHM files",
            mode
        );
        let _ = fs::remove_file(&wal_path);
        let _ = fs::remove_file(&shm_path);
        return;
    }

    // Phase 2: DB is in WAL mode — attempt checkpoint
    match rusqlite::Connection::open(db_path) {
        Ok(conn) => {
            if conn
                .pragma_update(None, "wal_checkpoint", "TRUNCATE")
                .is_ok()
            {
                tracing::info!("WAL checkpoint complete");
            } else {
                tracing::warn!(
                    "WAL checkpoint failed — stale companions will be replaced on next write"
                );
            }
        }
        Err(e) => {
            tracing::warn!("Cannot open database for checkpoint: {}", e);
        }
    }
}

pub fn create_pool(db_path: &str) -> anyhow::Result<Pool<SqliteConnectionManager>> {
    auto_detect_and_cleanup(db_path);

    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder().max_size(8).build(manager)?;

    // Verify pool works and apply pragmas
    let conn = pool.get()?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;",
    )?;

    Ok(pool)
}
