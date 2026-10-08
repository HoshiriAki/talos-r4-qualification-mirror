#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p9-sp05"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p9-sp05.XXXXXX")"
mkdir -p "$RUNTIME_DIR/tls"

run_id="${GITHUB_RUN_ID:-local}"
PROJECT="talos-p9-sp05-${run_id}"
PROJECT="$(printf '%s' "$PROJECT" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

export DB_PASSWORD="$(openssl rand -hex 18)"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"
export CORS_ALLOWED_ORIGIN="https://p9-sp05.invalid"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER="p9sp05-admin"
export GRAFANA_PASSWORD="$(openssl rand -hex 18)"
export P9_SP05_BOOTSTRAP_USERNAME="p9sp05-${run_id}"
export P9_SP05_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"
export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"
export TALOS_HTTP_PORT="18080"
export TALOS_HTTPS_PORT="18443"
export TALOS_PROMETHEUS_PORT="19090"
export TALOS_GRAFANA_PORT="13000"

# Docker Compose file-backed secrets are read by non-root service UIDs.
# Keep the host parent private while making the mounted source read-only/readable.
test "$(stat -c '%a' "$RUNTIME_DIR")" = "700"
openssl rand -hex 32 > "$METRICS_SCRAPE_TOKEN_FILE"
chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"

PLATFORM_HOST="p9-sp05.invalid"
PLATFORM_ORIGIN="https://${PLATFORM_HOST}"
PLATFORM_CONNECT="${PLATFORM_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"

compose=(
  docker compose
  -p "$PROJECT"
  -f deploy/compose.production.yml
  -f deploy/compose.p9-sp05.yml
)

cleanup() {
  set +e
  "${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-final.txt" 2>&1
  "${compose[@]}" logs --no-color > "$EVIDENCE_DIR/compose.log" 2>&1
  "${compose[@]}" down -v --remove-orphans > "$EVIDENCE_DIR/compose-down.log" 2>&1
  rm -rf "$RUNTIME_DIR"
}
trap cleanup EXIT

openssl req -x509 -newkey rsa:2048 -sha256 -days 1 -nodes   -keyout "$TLS_KEY_FILE"   -out "$TLS_CERT_FILE"   -subj "/CN=localhost"   -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"   >/dev/null 2>&1

docker version > "$EVIDENCE_DIR/docker-version.txt"
docker compose version > "$EVIDENCE_DIR/docker-compose-version.txt"
git rev-parse HEAD > "$EVIDENCE_DIR/source-sha.txt"

"${compose[@]}" config >/dev/null
"${compose[@]}" config --images > "$EVIDENCE_DIR/compose-images.txt"
"${compose[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
"${compose[@]}" pull db prometheus > "$EVIDENCE_DIR/pull.log"
"${compose[@]}" build --pull app nginx > "$EVIDENCE_DIR/build.log"
"${compose[@]}" up -d --no-build db app nginx prometheus
"${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-started.txt"

for _ in $(seq 1 90); do
  if curl --silent --show-error --fail --insecure     "https://127.0.0.1:${TALOS_HTTPS_PORT}/ready"     -o "$RUNTIME_DIR/ready.json"
  then
    break
  fi
  sleep 2
done
curl --silent --show-error --fail --insecure   "https://127.0.0.1:${TALOS_HTTPS_PORT}/ready"   -o "$RUNTIME_DIR/ready.json"
grep -F '"status":"ready"' "$RUNTIME_DIR/ready.json" >/dev/null
grep -F '"database":"postgres"' "$RUNTIME_DIR/ready.json" >/dev/null

# Generate one real Registry event through the deployed HTTPS boundary. RuntimeMetrics
# intentionally omits zero-valued series, so a live Registry counter must be driven
# before Prometheus can prove that metric family is exported.
PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"
platform_login_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie-jar "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/platform-login.json" \
  --write-out '%{http_code}' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP05_BOOTSTRAP_USERNAME" "$P9_SP05_BOOTSTRAP_PASSWORD")" \
  "$PLATFORM_ORIGIN/auth/platform/login")"
test "$platform_login_status" = "200"

governance_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/tenant-governance-list.json" \
  --write-out '%{http_code}' \
  -H 'X-Talos-Authority: platform' \
  "$PLATFORM_ORIGIN/api/tenant-governance/tenants")"
test "$governance_status" = "200"

for _ in $(seq 1 60); do
  if curl --silent --show-error --fail     "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/-/ready"     -o "$RUNTIME_DIR/prometheus-ready.txt"
  then
    break
  fi
  sleep 2
done
curl --silent --show-error --fail   "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/-/ready"   -o "$RUNTIME_DIR/prometheus-ready.txt"

for _ in $(seq 1 20); do
  curl --silent --show-error --fail     --get     --data-urlencode 'query=up{job="talos-backend"}'     "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/api/v1/query"     -o "$RUNTIME_DIR/prometheus-up.json"
  if python3 - "$RUNTIME_DIR/prometheus-up.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
results=data.get("data",{}).get("result",[])
raise SystemExit(0 if any(item.get("value", [None,"0"])[1] == "1" for item in results) else 1)
PY
  then
    break
  fi
  sleep 3
done

curl --silent --show-error --fail   --get   --data-urlencode 'query=up{job="talos-backend"}'   "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/api/v1/query"   -o "$RUNTIME_DIR/prometheus-up.json"
curl --silent --show-error --fail   --get   --data-urlencode 'query=talos_uptime_seconds'   "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/api/v1/query"   -o "$RUNTIME_DIR/prometheus-uptime.json"
curl --silent --show-error --fail   --get   --data-urlencode 'query=count(talos_http_requests_total)'   "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/api/v1/query"   -o "$RUNTIME_DIR/prometheus-http-count.json"
for _ in $(seq 1 20); do
  curl --silent --show-error --fail \
    --get \
    --data-urlencode 'query=sum(talos_registry_commands_total{phase="attempt",module="tenant_governance"})' \
    "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/api/v1/query" \
    -o "$RUNTIME_DIR/prometheus-registry-count.json"
  if python3 - "$RUNTIME_DIR/prometheus-registry-count.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
results=data.get("data",{}).get("result",[])
raise SystemExit(0 if results and float(results[0].get("value", [None,"0"])[1]) >= 1.0 else 1)
PY
  then
    break
  fi
  sleep 3
done

curl --silent --show-error --fail \
  --get \
  --data-urlencode 'query=sum(talos_registry_commands_total{phase="attempt",module="tenant_governance"})' \
  "http://127.0.0.1:${TALOS_PROMETHEUS_PORT}/api/v1/query" \
  -o "$RUNTIME_DIR/prometheus-registry-count.json"

python3 -   "$RUNTIME_DIR/prometheus-up.json"   "$RUNTIME_DIR/prometheus-uptime.json"   "$RUNTIME_DIR/prometheus-http-count.json"   "$RUNTIME_DIR/prometheus-registry-count.json" <<'PY'
import json,sys

def load(path):
    with open(path, encoding='utf-8') as f:
        return json.load(f)

up=load(sys.argv[1]).get("data",{}).get("result",[])
assert any(item.get("value", [None,"0"])[1] == "1" for item in up)

uptime=load(sys.argv[2]).get("data",{}).get("result",[])
assert uptime and float(uptime[0]["value"][1]) >= 0.0

http_count=load(sys.argv[3]).get("data",{}).get("result",[])
assert http_count and float(http_count[0]["value"][1]) > 0.0

registry_count=load(sys.argv[4]).get("data",{}).get("result",[])
assert registry_count and float(registry_count[0]["value"][1]) > 0.0
PY

SCRAPE_TOKEN="$(cat "$METRICS_SCRAPE_TOKEN_FILE")"
public_metrics_status="$(curl --silent --show-error --insecure   --output "$RUNTIME_DIR/public-metrics-token.json"   --write-out '%{http_code}'   -H "Authorization: Bearer $SCRAPE_TOKEN"   "https://127.0.0.1:${TALOS_HTTPS_PORT}/metrics")"
test "$public_metrics_status" = "401"

cat > "$EVIDENCE_DIR/observability-summary.txt" <<EOF
source_sha=$(git rev-parse HEAD)
prometheus_backend_up=1
talos_uptime_seconds=present
talos_http_requests_total=present
talos_registry_commands_total=tenant_governance_attempt_observed
public_nginx_scrape_token_status=$public_metrics_status
metrics_scrape_peer=172.29.0.40
production_plugin_runtime=disabled
qualification_plugin_fixture=exact_head_quality_job
EOF

echo "P9_SP05_OBSERVABILITY prometheus_scrape=pass selected_metrics=pass public_token_path=fail_closed"
