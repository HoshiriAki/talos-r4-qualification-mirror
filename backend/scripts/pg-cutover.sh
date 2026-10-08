#!/usr/bin/env bash
# =============================================================================
# Talos — PostgreSQL Production Cutover Script
# =============================================================================
# Performs a controlled SQLite → PostgreSQL cutover with:
#  1. Stop writes (set READ_ONLY flag)
#  2. Final data sync via data-tool
#  3. Switch DB_BACKEND=postgres
#  4. Start Talos on PostgreSQL
#  5. Health-check + verify
#  6. Log total downtime
#
# Usage:
#   ./scripts/pg-cutover.sh [--dry-run] [--health-url http://localhost:8080/health]
#
# Prerequisites:
#   - PostgreSQL running and DATABASE_URL set
#   - cargo build --features postgres completed
#   - data-tool binary available (or built via cargo run --features postgres --bin data-tool)
# =============================================================================

set -euo pipefail

# --- Configuration -----------------------------------------------------------
DRY_RUN=false
HEALTH_URL="${HEALTH_URL:-http://localhost:8080/health}"
HEALTH_RETRIES=30
HEALTH_DELAY=2
READ_ONLY_FILE="${READ_ONLY_FILE:-/tmp/talos-readonly.flag}"
LOG_FILE="${LOG_FILE:-/tmp/talos-cutover-$(date +%Y%m%d-%H%M%S).log}"
BACKEND_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$(cd "$BACKEND_DIR/.." && pwd)"

# --- Argument parsing --------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)      DRY_RUN=true; shift ;;
        --health-url)   HEALTH_URL="$2"; shift 2 ;;
        --log-file)     LOG_FILE="$2"; shift 2 ;;
        *)              echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# --- Logging -----------------------------------------------------------------
log() {
    local timestamp
    timestamp="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "[$timestamp] $*" | tee -a "$LOG_FILE"
}

log "=== Talos PostgreSQL Cutover ==="
log "Start time (Unix): $(date +%s)"

# Helper: strip password from connection URL for safe logging
sanitize_url() { echo "$1" | sed 's/:\\([^:@]*\\)@/:****@/'; }

# --- Phase 1: Pre-flight checks ----------------------------------------------
log "[phase:preflight] Running pre-flight checks..."

# Check PostgreSQL is reachable
if command -v psql &> /dev/null; then
    if ! psql "$DATABASE_URL" -c "SELECT 1;" > /dev/null 2>&1; then
        log "[ERROR] Cannot connect to PostgreSQL at $(sanitize_url "$DATABASE_URL")"
        exit 1
    fi
    log "[phase:preflight] PostgreSQL reachable"
else
    log "[phase:preflight] psql not found — skipping direct PG connection check"
fi

# Verify cargo build succeeded with postgres feature
if [ -f "$BACKEND_DIR/target/debug/talos-backend" ]; then
    log "[phase:preflight] Talos binary found"
else
    log "[WARN] Talos binary not found at expected path — attempting build..."
    if [ "$DRY_RUN" = false ]; then
        (cd "$BACKEND_DIR" && cargo build --features postgres) || {
            log "[ERROR] cargo build --features postgres failed"
            exit 1
        }
    fi
fi

if [ "$DRY_RUN" = true ]; then
    log "[phase:preflight] DRY RUN — no actual changes will be made"
fi

# --- Phase 2: Stop writes (set read-only mode) -------------------------------
DOWNTIME_START=$(date +%s)

log "[phase:stop-writes] Setting read-only mode..."
if [ "$DRY_RUN" = false ]; then
    touch "$READ_ONLY_FILE"
    log "[phase:stop-writes] Read-only flag set at $READ_ONLY_FILE"
    log "[phase:stop-writes] Downtime started at $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
else
    log "[phase:stop-writes] (DRY RUN) Would set $READ_ONLY_FILE"
fi

# Wait for in-flight writes to drain (grace period)
log "[phase:drain] Waiting 5s for in-flight writes to drain..."
sleep 5

# --- Phase 3: Final data sync ------------------------------------------------
log "[phase:sync] Running final SQLite → PostgreSQL data sync..."

if [ "$DRY_RUN" = false ]; then
    # Run the data-tool to do a full dump/restore from SQLite to Postgres
    (cd "$BACKEND_DIR" && cargo run --features postgres --bin data-tool 2>&1 | tee -a "$LOG_FILE") || {
        log "[ERROR] data-tool sync failed — check logs at $LOG_FILE"
        log "[phase:abort] Aborting cutover. Remove $READ_ONLY_FILE to resume writes."
        exit 1
    }
    log "[phase:sync] Data sync complete"
else
    log "[phase:sync] (DRY RUN) Would run: cargo run --features postgres --bin data-tool"
fi

# --- Phase 4: Switch to PostgreSQL -------------------------------------------
log "[phase:switch] Switching DB_BACKEND to postgres..."

if [ "$DRY_RUN" = false ]; then
    export DB_BACKEND=postgres
    log "[phase:switch] DB_BACKEND=postgres set"
else
    log "[phase:switch] (DRY RUN) Would set DB_BACKEND=postgres"
fi

# --- Phase 5: Start Talos on PostgreSQL ------------------------------------
log "[phase:start] Starting Talos backend on PostgreSQL..."

if [ "$DRY_RUN" = false ]; then
    # Start in background, capture PID
    (cd "$BACKEND_DIR" && cargo run --features postgres) &
    APP_PID=$!
    log "[phase:start] Talos started with PID=$APP_PID"
else
    log "[phase:start] (DRY RUN) Would start: cargo run --features postgres"
    APP_PID=0
fi

# --- Phase 6: Health-check ---------------------------------------------------
log "[phase:health] Waiting for health endpoint ($HEALTH_URL)..."

if [ "$DRY_RUN" = false ]; then
    for i in $(seq 1 "$HEALTH_RETRIES"); do
        if curl -sf --connect-timeout 3 "$HEALTH_URL" > /dev/null 2>&1; then
            log "[phase:health] Health endpoint OK (attempt $i)"
            break
        fi
        if [ "$i" -eq "$HEALTH_RETRIES" ]; then
            log "[ERROR] Health-check failed after $HEALTH_RETRIES attempts"
            log "[phase:abort] Cutover may have failed — check PID $APP_PID"
            DOWNTIME_END=$(date +%s)
            DOWNTIME_SEC=$((DOWNTIME_END - DOWNTIME_START))
            log "[downtime] $DOWNTIME_SEC seconds (unhealthy)"
            exit 1
        fi
        sleep "$HEALTH_DELAY"
    done
else
    log "[phase:health] (DRY RUN) Would poll $HEALTH_URL"
fi

# --- Phase 7: Cleanup & verification -----------------------------------------
DOWNTIME_END=$(date +%s)
DOWNTIME_SEC=$((DOWNTIME_END - DOWNTIME_START))
DOWNTIME_MIN=$(echo "scale=2; $DOWNTIME_SEC / 60" | bc 2>/dev/null || echo "$DOWNTIME_SEC")

log "[phase:verify] Cutover complete!"
log "[downtime] Total downtime: ${DOWNTIME_SEC}s (~${DOWNTIME_MIN} min)"

# Remove read-only flag
if [ "$DRY_RUN" = false ]; then
    rm -f "$READ_ONLY_FILE"
    log "[phase:cleanup] Read-only flag removed — writes re-enabled on PostgreSQL"
fi

# Verify migration status
if [ "$DRY_RUN" = false ] && command -v psql &> /dev/null; then
    MIGRATION_COUNT=$(psql "$DATABASE_URL" -t -c "SELECT COUNT(*) FROM schema_migrations;" 2>/dev/null | tr -d ' ' || echo "N/A")
    log "[phase:verify] PostgreSQL schema_migrations count: $MIGRATION_COUNT"
fi

log "=== Cutover finished successfully ==="
log "Log file: $LOG_FILE"

# Return 0 for success, exit code 2 for dry-run
if [ "$DRY_RUN" = true ]; then
    echo ""
    echo "DRY RUN complete — no changes were made."
    exit 0
fi
