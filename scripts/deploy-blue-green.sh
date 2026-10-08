#!/bin/bash
# Blue-Green deployment script
set -euo pipefail

COMPOSE_FILE="docker-compose.yml"
NEW_COLOR="${1:-green}"
OLD_COLOR=$([ "$NEW_COLOR" = "green" ] && echo "blue" || echo "green")
NEW_PORT=$([ "$NEW_COLOR" = "green" ] && echo "8081" || echo "8082")

echo "=== Talos Blue-Green Deploy: $OLD_COLOR to $NEW_COLOR ==="

# Build and start new instance
docker compose -f "$COMPOSE_FILE" up -d --scale "app_${NEW_COLOR}=1" "app_${NEW_COLOR}"

# Health check
echo "Health check $NEW_COLOR on port $NEW_PORT..."
for i in $(seq 1 30); do
    if curl -sf "http://localhost:$NEW_PORT/health" > /dev/null 2>&1; then
        echo "OK: $NEW_COLOR healthy"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "FAIL: $NEW_COLOR health check timed out - rolling back"
        docker compose -f "$COMPOSE_FILE" stop "app_${NEW_COLOR}"
        exit 1
    fi
    sleep 2
done

# Swap traffic
echo "Switching traffic to $NEW_COLOR..."
docker compose -f "$COMPOSE_FILE" exec nginx nginx -s reload

# Stop old instance
echo "Stopping $OLD_COLOR..."
docker compose -f "$COMPOSE_FILE" stop "app_${OLD_COLOR}"

echo "=== Deploy complete: $NEW_COLOR active ==="
