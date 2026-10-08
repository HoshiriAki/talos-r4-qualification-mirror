#!/bin/bash
# Talos 灾备恢复脚本
# 用法: ./dr-restore.sh <backup_file> <target_db>

set -euo pipefail

BACKUP_FILE="$1"
TARGET_DB="${2:-data/talos_restored.db}"

echo "=== Talos DR Restore ==="
echo "Source: $BACKUP_FILE"
echo "Target: $TARGET_DB"

# Decrypt if needed
if [[ "$BACKUP_FILE" == *.enc ]]; then
    DECRYPTED="/tmp/talos_restore_decrypted.db.gz"
    openssl enc -d -aes-256-gcm -pbkdf2 -pass "pass:${BACKUP_PASSPHRASE:?BACKUP_PASSPHRASE is required}" -in "$BACKUP_FILE" -out "$DECRYPTED" 2>/dev/null || {
        echo "ERROR: Decryption failed"
        exit 1
    }
    BACKUP_FILE="$DECRYPTED"
fi

# Restore
gunzip -c "$BACKUP_FILE" > "$TARGET_DB" 2>/dev/null || cp "$BACKUP_FILE" "$TARGET_DB"

# Verify
if sqlite3 "$TARGET_DB" "SELECT COUNT(*) FROM orders;" > /dev/null 2>&1; then
    ORDER_COUNT=$(sqlite3 "$TARGET_DB" "SELECT COUNT(*) FROM orders;")
    echo "Restore OK: $ORDER_COUNT orders in target DB"
    echo "Checksum: $(sha256sum "$TARGET_DB" | cut -d' ' -f1)"
else
    echo "ERROR: Restored DB verification failed"
    exit 1
fi

# Cleanup
rm -f /tmp/talos_restore_decrypted.db.gz
echo "=== Restore Complete ==="
