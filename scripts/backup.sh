#!/bin/bash
# Talos 数据库备份脚本
# 用法: ./backup.sh [db_path] [backup_dir]

set -euo pipefail

DB_PATH="${1:-data/talos.db}"
BACKUP_DIR="${2:-backups}"
RETENTION_DAYS=30

mkdir -p "$BACKUP_DIR"

TIMESTAMP=$(date -u +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/talos_$TIMESTAMP.db.gz"

# Backup SQLite
sqlite3 "$DB_PATH" ".backup /tmp/talos_backup.db" 2>/dev/null || sqlite3 "$DB_PATH" ".dump" | gzip > "$BACKUP_FILE"
if [ ! -f "$BACKUP_FILE" ]; then
    sqlite3 "$DB_PATH" ".backup /tmp/talos_backup_tmp.db"
    gzip -c /tmp/talos_backup_tmp.db > "$BACKUP_FILE"
    rm -f /tmp/talos_backup_tmp.db
fi

# Encrypt with AES-256-GCM (openssl)
ENCRYPTED_FILE="$BACKUP_FILE.enc"
openssl enc -aes-256-gcm -salt -pbkdf2 -pass "pass:${BACKUP_PASSPHRASE:?BACKUP_PASSPHRASE is required}" -in "$BACKUP_FILE" -out "$ENCRYPTED_FILE" 2>/dev/null && {
    rm "$BACKUP_FILE"
    echo "Backup: $ENCRYPTED_FILE ($(du -h "$ENCRYPTED_FILE" | cut -f1))"
} || {
    echo "Backup (unencrypted): $BACKUP_FILE ($(du -h "$BACKUP_FILE" | cut -f1))"
}

# Cleanup old backups
find "$BACKUP_DIR" -name "*.enc" -mtime +$RETENTION_DAYS -delete 2>/dev/null || true
find "$BACKUP_DIR" -name "*.gz" -mtime +$RETENTION_DAYS -delete 2>/dev/null || true

echo "Retention: $RETENTION_DAYS days, kept $(find "$BACKUP_DIR" -type f | wc -l) files"
