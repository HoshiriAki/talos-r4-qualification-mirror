#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p9-sp04"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p9-sp04.XXXXXX")"
mkdir -p "$RUNTIME_DIR/tls"

run_id="${GITHUB_RUN_ID:-local}"
PROJECT="talos-p9-sp04-${run_id}"
PROJECT="$(printf '%s' "$PROJECT" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

export DB_PASSWORD="$(openssl rand -hex 18)"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"
export P9_SP03_BOOTSTRAP_USERNAME="p9sp04-${run_id}"
export P9_SP03_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"
export CORS_ALLOWED_ORIGIN="https://p9-sp04.invalid"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER="p9sp04-admin"
export GRAFANA_PASSWORD="$(openssl rand -hex 18)"
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

PLATFORM_HOST="p9-sp04.invalid"
PLATFORM_ORIGIN="https://${PLATFORM_HOST}"
PLATFORM_CONNECT="${PLATFORM_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
TENANT_A_SLUG="p9sp04-a-${run_id}"
TENANT_B_SLUG="p9sp04-b-${run_id}"
TENANT_A_HOST="${TENANT_A_SLUG}.talos.invalid"
TENANT_B_HOST="${TENANT_B_SLUG}.talos.invalid"
TENANT_A_ORIGIN="https://${TENANT_A_HOST}"
TENANT_B_ORIGIN="https://${TENANT_B_HOST}"
TENANT_A_CONNECT="${TENANT_A_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
TENANT_B_CONNECT="${TENANT_B_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"

compose=(
  docker compose
  -p "$PROJECT"
  -f deploy/compose.production.yml
  -f deploy/compose.p9-sp03.yml
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
"${compose[@]}" pull db prometheus grafana > "$EVIDENCE_DIR/pull.log"
"${compose[@]}" build --pull app nginx > "$EVIDENCE_DIR/build.log"
"${compose[@]}" up -d --no-build db app nginx
"${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-started.txt"

for _ in $(seq 1 90); do
  if curl --silent --show-error --fail --insecure --noproxy '*'     --connect-to "$PLATFORM_CONNECT"     "$PLATFORM_ORIGIN/ready"     -o "$RUNTIME_DIR/ready.json"
  then
    break
  fi
  sleep 2
done
curl --silent --show-error --fail --insecure --noproxy '*'   --connect-to "$PLATFORM_CONNECT"   "$PLATFORM_ORIGIN/ready" -o "$RUNTIME_DIR/ready.json"
grep -F '"status":"ready"' "$RUNTIME_DIR/ready.json" >/dev/null
grep -F '"database":"postgres"' "$RUNTIME_DIR/ready.json" >/dev/null

PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"
platform_login_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$PLATFORM_CONNECT"   --cookie-jar "$PLATFORM_COOKIE"   --output "$RUNTIME_DIR/platform-login.json"   --write-out '%{http_code}'   -H "Origin: $PLATFORM_ORIGIN"   -H 'Content-Type: application/json'   --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")"   "$PLATFORM_ORIGIN/auth/platform/login")"
test "$platform_login_status" = "200"

create_tenant() {
  local slug="$1"
  local label="$2"
  local out="$3"
  local status
  status="$(curl --silent --show-error --insecure --noproxy '*'     --connect-to "$PLATFORM_CONNECT"     --cookie "$PLATFORM_COOKIE"     --output "$out"     --write-out '%{http_code}'     -H 'X-Talos-Authority: platform'     -H "Origin: $PLATFORM_ORIGIN"     -H 'Content-Type: application/json'     --data "$(printf '{"name":"%s","slug":"%s"}' "$label" "$slug")"     "$PLATFORM_ORIGIN/api/tenants")"
  test "$status" = "200"
  python3 - "$out" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["status"] == "active"
print(data["id"])
PY
}

TENANT_A_ID="$(create_tenant "$TENANT_A_SLUG" "P9 SP04 Tenant A" "$RUNTIME_DIR/tenant-a-create.json")"
TENANT_B_ID="$(create_tenant "$TENANT_B_SLUG" "P9 SP04 Tenant B" "$RUNTIME_DIR/tenant-b-create.json")"
test -n "$TENANT_A_ID"
test -n "$TENANT_B_ID"
test "$TENANT_A_ID" != "$TENANT_B_ID"

login_tenant() {
  local origin="$1"
  local connect="$2"
  local cookie="$3"
  local out="$4"
  local status
  status="$(curl --silent --show-error --insecure --noproxy '*'     --connect-to "$connect"     --cookie-jar "$cookie"     --output "$out"     --write-out '%{http_code}'     -H "Origin: $origin"     -H 'Content-Type: application/json'     --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")"     "$origin/auth/login")"
  test "$status" = "200"
}

TENANT_A_COOKIE="$RUNTIME_DIR/tenant-a.cookies"
TENANT_B_COOKIE="$RUNTIME_DIR/tenant-b.cookies"
login_tenant "$TENANT_A_ORIGIN" "$TENANT_A_CONNECT" "$TENANT_A_COOKIE" "$RUNTIME_DIR/tenant-a-login.json"
login_tenant "$TENANT_B_ORIGIN" "$TENANT_B_CONNECT" "$TENANT_B_COOKIE" "$RUNTIME_DIR/tenant-b-login.json"

MODEL_BODY="$(printf '{"name":"P9 SP04 Model %s","category":"qualification","prefix":"P9Q","enabled":true}' "$run_id")"
model_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT"   --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/model-create.json"   --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN"   -H 'Content-Type: application/json'   --data "$MODEL_BODY"   "$TENANT_A_ORIGIN/api/device-models")"
test "$model_status" = "200"
MODEL_ID="$(python3 - "$RUNTIME_DIR/model-create.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
value=data.get("id") or data.get("model",{}).get("id")
assert isinstance(value,str) and value
print(value)
PY
)"

MACHINE_PROVISION_BODY='{"name":"P9 SP04 business writer","scopes":[{"module":"device","command":"create_device"}],"rate_limit_rpm":60,"role":"admin"}'
machine_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT"   --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/machine-issued.json"   --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN"   -H 'Content-Type: application/json'   --data "$MACHINE_PROVISION_BODY"   "$TENANT_A_ORIGIN/api/machine-clients")"
test "$machine_status" = "200"

readarray -t MACHINE_VALUES < <(python3 - "$RUNTIME_DIR/machine-issued.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
for key in ("client_id","secret"):
    value=data[key]
    assert isinstance(value,str) and value
    print(value)
PY
)
MACHINE_CLIENT_ID="${MACHINE_VALUES[0]}"
MACHINE_SECRET="${MACHINE_VALUES[1]}"

DEVICE_SERIAL="P9SP04${run_id//[^0-9]/}A"
DEVICE_BODY="$(printf '{"module":"device","command":"create_device","payload":{"serialNo":"%s","modelId":"%s","warehouseId":"","status":"available"}}' "$DEVICE_SERIAL" "$MODEL_ID")"
MACHINE_HEADERS="$RUNTIME_DIR/machine-create.headers"
write_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT"   --dump-header "$MACHINE_HEADERS"   --output "$RUNTIME_DIR/machine-create.json"   --write-out '%{http_code}'   -H "Authorization: Bearer $MACHINE_SECRET"   -H 'Content-Type: application/json'   --data "$DEVICE_BODY"   "$TENANT_A_ORIGIN/api/machine/v1/tenants/${TENANT_A_ID}/execute")"
test "$write_status" = "200"
python3 - "$RUNTIME_DIR/machine-create.json" "$DEVICE_SERIAL" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("ok") is True
result=data.get("result")
assert isinstance(result,dict)
assert result.get("serialNo") == sys.argv[2]
PY

CORRELATION_ID="$(awk 'tolower($1)=="x-correlation-id:" {gsub("\r","",$2); print $2}' "$MACHINE_HEADERS" | tail -n1)"
test -n "$CORRELATION_ID"

read_a_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT"   --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/device-a.json"   --write-out '%{http_code}'   "$TENANT_A_ORIGIN/devices/${DEVICE_SERIAL}")"
test "$read_a_status" = "200"
python3 - "$RUNTIME_DIR/device-a.json" "$DEVICE_SERIAL" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("serialNo") == sys.argv[2]
PY

read_b_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_B_CONNECT"   --cookie "$TENANT_B_COOKIE"   --output "$RUNTIME_DIR/device-b.json"   --write-out '%{http_code}'   "$TENANT_B_ORIGIN/devices/${DEVICE_SERIAL}")"
test "$read_b_status" = "200"
python3 - "$RUNTIME_DIR/device-b.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data is None
PY

canonical_a_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc   "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${DEVICE_SERIAL}';")"
canonical_b_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc   "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${DEVICE_SERIAL}';")"
test "$canonical_a_count" = "1"
test "$canonical_b_count" = "0"

machine_admission_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc   "SELECT COUNT(*) FROM audit_events WHERE action='machine.access' AND tenant_id='${TENANT_A_ID}' AND resource_id='${MACHINE_CLIENT_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'outcome'='admitted';")"
command_audit_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc   "SELECT COUNT(*) FROM audit_events WHERE action='device.create_device' AND resource_type='command' AND tenant_id='${TENANT_A_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'result'='succeeded' AND detail_json->>'execution_mode'='Normal' AND detail_json #>> '{data_scope,namespace}'='Production' AND detail_json #>> '{payload,module}'='device' AND detail_json #>> '{payload,command}'='create_device';")"
cross_tenant_audit_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc   "SELECT COUNT(*) FROM audit_events WHERE correlation_id='${CORRELATION_ID}' AND tenant_id='${TENANT_B_ID}';")"
test "$machine_admission_count" = "1"
test "$command_audit_count" = "1"
test "$cross_tenant_audit_count" = "0"

cat > "$EVIDENCE_DIR/business-summary.txt" <<EOF
source_sha=$(git rev-parse HEAD)
tenant_a_write=200
tenant_a_read=200
tenant_b_same_device_read=200_null
canonical_tenant_a_device_rows=$canonical_a_count
canonical_tenant_b_device_rows=$canonical_b_count
canonical_machine_access_audit=$machine_admission_count
canonical_device_create_succeeded_audit=$command_audit_count
cross_tenant_audit_rows=$cross_tenant_audit_count
preview_simulation_execution_mode=Normal_Production
EOF

echo "P9_SP04_BUSINESS_EVIDENCE source_sha=$(git rev-parse HEAD)"
echo "P9_SP04_BUSINESS_EVIDENCE write=pass read=pass cross_tenant=isolated canonical_command_audit=pass"
