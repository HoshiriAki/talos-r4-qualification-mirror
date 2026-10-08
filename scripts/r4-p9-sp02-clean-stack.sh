#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p9-sp02"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p9-sp02.XXXXXX")"
mkdir -p "$RUNTIME_DIR/tls"

run_id="${GITHUB_RUN_ID:-local}"
PROJECT="talos-p9-sp02-${run_id}"
PROJECT="$(printf '%s' "$PROJECT" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

export DB_PASSWORD="p9sp02-ci-only"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"
export CORS_ALLOWED_ORIGIN="https://127.0.0.1:18443"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER="p9sp02-admin"
export GRAFANA_PASSWORD="p9sp02-ci-only"
export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"
# Docker Compose file-backed secrets are read by non-root service UIDs.
# Keep the host parent private while making the mounted source read-only/readable.
test "$(stat -c '%a' "$RUNTIME_DIR")" = "700"
openssl rand -hex 32 > "$METRICS_SCRAPE_TOKEN_FILE"
chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"
export TALOS_HTTP_PORT="18080"
export TALOS_HTTPS_PORT="18443"
export TALOS_PROMETHEUS_PORT="19090"
export TALOS_GRAFANA_PORT="13000"

compose=(docker compose -p "$PROJECT" -f deploy/compose.production.yml)

cleanup() {
  set +e
  "${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-final.txt" 2>&1
  "${compose[@]}" logs --no-color > "$EVIDENCE_DIR/compose.log" 2>&1
  "${compose[@]}" down -v --remove-orphans > "$EVIDENCE_DIR/compose-down.log" 2>&1
  rm -rf "$RUNTIME_DIR"
}
trap cleanup EXIT

openssl req -x509 -newkey rsa:2048 -sha256 -days 1 -nodes \
  -keyout "$TLS_KEY_FILE" \
  -out "$TLS_CERT_FILE" \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
  >/dev/null 2>&1

docker version > "$EVIDENCE_DIR/docker-version.txt"
docker compose version > "$EVIDENCE_DIR/docker-compose-version.txt"
git rev-parse HEAD > "$EVIDENCE_DIR/source-sha.txt"

"${compose[@]}" config >/dev/null
"${compose[@]}" config --images > "$EVIDENCE_DIR/compose-images.txt"
"${compose[@]}" down -v --remove-orphans >/dev/null 2>&1 || true

"${compose[@]}" pull db prometheus grafana | tee "$EVIDENCE_DIR/pull.log"
"${compose[@]}" build --pull app nginx | tee "$EVIDENCE_DIR/build.log"

{
  echo "source_sha=$(git rev-parse HEAD)"
  for image in \
    postgres:18.6-alpine3.24 \
    prom/prometheus:v3.14.0 \
    grafana/grafana:13.2.2
  do
    docker image inspect "$image" \
      --format 'image={{index .RepoTags 0}} id={{.Id}} repo_digests={{json .RepoDigests}}'
  done
} > "$EVIDENCE_DIR/image-identities.txt"

"${compose[@]}" up -d --no-build db app nginx
"${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-started.txt"

for _ in $(seq 1 90); do
  if curl --silent --show-error --fail --insecure \
    "https://127.0.0.1:${TALOS_HTTPS_PORT}/ready" \
    -o "$EVIDENCE_DIR/ready.json"
  then
    break
  fi
  sleep 2
done

curl --silent --show-error --fail --insecure \
  "https://127.0.0.1:${TALOS_HTTPS_PORT}/health" \
  -o "$EVIDENCE_DIR/health.json"
curl --silent --show-error --fail --insecure \
  "https://127.0.0.1:${TALOS_HTTPS_PORT}/ready" \
  -o "$EVIDENCE_DIR/ready.json"
curl --silent --show-error --fail --insecure \
  "https://127.0.0.1:${TALOS_HTTPS_PORT}/" \
  -o "$EVIDENCE_DIR/index.html"

grep -F '"status":"ok"' "$EVIDENCE_DIR/health.json" >/dev/null
grep -F '"status":"ready"' "$EVIDENCE_DIR/ready.json" >/dev/null
grep -F '"database":"postgres"' "$EVIDENCE_DIR/ready.json" >/dev/null
grep -F '<div id="app"></div>' "$EVIDENCE_DIR/index.html" >/dev/null

http_status="$(curl --silent --output /dev/null --write-out '%{http_code}' \
  "http://127.0.0.1:${TALOS_HTTP_PORT}/health")"
test "$http_status" = "308"
echo "$http_status" > "$EVIDENCE_DIR/http-redirect-status.txt"

migration_count="$("${compose[@]}" exec -T db \
  psql -U talos -d talos -Atc 'SELECT COUNT(*) FROM schema_migrations;')"
latest_required="$("${compose[@]}" exec -T db \
  psql -U talos -d talos -Atc "SELECT COUNT(*) FROM schema_migrations WHERE id='083_r4_reservation_rule_sequence_invariant';")"
server_version="$("${compose[@]}" exec -T db \
  psql -U talos -d talos -Atc 'SHOW server_version;')"

test "$migration_count" = "80"
test "$latest_required" = "1"
case "$server_version" in
  18.*) ;;
  *)
    echo "expected PostgreSQL 18, got $server_version" >&2
    exit 1
    ;;
esac

{
  echo "schema_migration_count=$migration_count"
  echo "required_083_r4_reservation_rule_sequence_invariant=$latest_required"
  echo "server_version=$server_version"
} > "$EVIDENCE_DIR/migration-evidence.txt"

"${compose[@]}" exec -T app sh -ec \
  'test -z "$(find /app -type f \( -name "*.db" -o -name "*.db-wal" -o -name "*.db-shm" \) -print -quit)"'

app_image="$("${compose[@]}" images -q app | head -n1)"
nginx_image="$("${compose[@]}" images -q nginx | head -n1)"
{
  docker image inspect "$app_image" --format 'service=app id={{.Id}} repo_digests={{json .RepoDigests}}'
  docker image inspect "$nginx_image" --format 'service=nginx id={{.Id}} repo_digests={{json .RepoDigests}}'
} >> "$EVIDENCE_DIR/image-identities.txt"

negative_env=(
  -e NODE_ENV=production
  -e PUBLIC_HTTPS=true
  -e CORS_ALLOWED_ORIGIN="$CORS_ALLOWED_ORIGIN"
  -e TRUSTED_PROXY_COUNT=1
  -e TRUSTED_PROXY_CIDRS=172.29.0.0/24
  -e PORT=8080
)

set +e
timeout 20s docker run --rm --network none \
  "${negative_env[@]}" \
  -e DB_BACKEND=sqlite \
  -e DATABASE_URL="$TALOS_PRODUCTION_DATABASE_URL" \
  "$app_image" > "$EVIDENCE_DIR/invalid-profile.log" 2>&1
invalid_profile_status=$?
set -e
if [[ "$invalid_profile_status" -eq 0 || "$invalid_profile_status" -eq 124 ]]; then
  echo "invalid production SQLite profile did not fail closed" >&2
  exit 1
fi
grep -F 'production requires the PostgreSQL 18 database profile' \
  "$EVIDENCE_DIR/invalid-profile.log" >/dev/null

set +e
timeout 20s docker run --rm --network none \
  "${negative_env[@]}" \
  -e DB_BACKEND=postgres \
  -e DATABASE_URL=postgresql://talos:wrong@127.0.0.1:5432/talos \
  "$app_image" > "$EVIDENCE_DIR/invalid-database.log" 2>&1
invalid_database_status=$?
set -e
if [[ "$invalid_database_status" -eq 0 || "$invalid_database_status" -eq 124 ]]; then
  echo "invalid production PostgreSQL authority did not fail closed" >&2
  exit 1
fi
grep -F 'Failed to connect to PostgreSQL' "$EVIDENCE_DIR/invalid-database.log" >/dev/null

echo "P9_SP02_CLEAN_STACK source_sha=$(git rev-parse HEAD)"
echo "P9_SP02_CLEAN_STACK postgres_version=$server_version migrations=$migration_count"
echo "P9_SP02_CLEAN_STACK health=pass readiness=pass frontend=pass invalid_profile=fail_closed invalid_database=fail_closed"
