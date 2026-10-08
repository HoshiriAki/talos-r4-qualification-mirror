#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p9-sp03"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p9-sp03.XXXXXX")"
mkdir -p "$RUNTIME_DIR/tls"

run_id="${GITHUB_RUN_ID:-local}"
PROJECT="talos-p9-sp03-${run_id}"
PROJECT="$(printf '%s' "$PROJECT" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

export DB_PASSWORD="$(openssl rand -hex 18)"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"
export P9_SP03_BOOTSTRAP_USERNAME="p9sp03-${run_id}"
export P9_SP03_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"
export CORS_ALLOWED_ORIGIN="https://p9-sp03.invalid"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER="p9sp03-admin"
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

PLATFORM_HOST="p9-sp03.invalid"
PLATFORM_ORIGIN="https://${PLATFORM_HOST}"
TENANT_SLUG="p9sp03-${run_id}"
TENANT_HOST="${TENANT_SLUG}.talos.invalid"
TENANT_ORIGIN="https://${TENANT_HOST}"
PLATFORM_CONNECT="${PLATFORM_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
TENANT_CONNECT="${TENANT_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
SPOOFED_IP="203.0.113.99"

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

"${compose[@]}" pull db prometheus grafana > "$EVIDENCE_DIR/pull.log"
"${compose[@]}" build --pull app nginx > "$EVIDENCE_DIR/build.log"

"${compose[@]}" up -d --no-build db app nginx
"${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-started.txt"

for _ in $(seq 1 90); do
  if curl --silent --show-error --fail --insecure --noproxy '*' \
    --connect-to "$PLATFORM_CONNECT" \
    "$PLATFORM_ORIGIN/ready" \
    -o "$RUNTIME_DIR/ready.json"
  then
    break
  fi
  sleep 2
done
curl --silent --show-error --fail --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  "$PLATFORM_ORIGIN/ready" -o "$RUNTIME_DIR/ready.json"
grep -F '"status":"ready"' "$RUNTIME_DIR/ready.json" >/dev/null
grep -F '"database":"postgres"' "$RUNTIME_DIR/ready.json" >/dev/null

PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"
PLATFORM_LOGIN_HEADERS="$RUNTIME_DIR/platform-login.headers"
PLATFORM_LOGIN_BODY="$RUNTIME_DIR/platform-login.json"

invalid_platform_status="$(curl --silent --show-error --insecure \
  --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --output "$RUNTIME_DIR/platform-invalid.json" \
  --write-out '%{http_code}' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"wrong-password"}' "$P9_SP03_BOOTSTRAP_USERNAME")" \
  "$PLATFORM_ORIGIN/auth/platform/login")"
test "$invalid_platform_status" = "401"
grep -F 'AUTH_INVALID_CREDENTIALS' "$RUNTIME_DIR/platform-invalid.json" >/dev/null

platform_login_status="$(curl --silent --show-error --insecure \
  --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --dump-header "$PLATFORM_LOGIN_HEADERS" \
  --cookie-jar "$PLATFORM_COOKIE" \
  --output "$PLATFORM_LOGIN_BODY" \
  --write-out '%{http_code}' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")" \
  "$PLATFORM_ORIGIN/auth/platform/login")"
test "$platform_login_status" = "200"
grep -qi '^set-cookie:.*Secure' "$PLATFORM_LOGIN_HEADERS"
grep -qi '^set-cookie:.*HttpOnly' "$PLATFORM_LOGIN_HEADERS"
grep -qi '^set-cookie:.*SameSite=Lax' "$PLATFORM_LOGIN_HEADERS"

IDENTITY_ID="$(python3 - "$PLATFORM_LOGIN_BODY" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("ok") is True
assert data["user"]["authority"]["kind"] == "platform"
print(data["user"]["id"])
PY
)"
test -n "$IDENTITY_ID"

platform_me_status="$(curl --silent --show-error --insecure \
  --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/platform-me.json" \
  --write-out '%{http_code}' \
  -H 'X-Talos-Authority: platform' \
  "$PLATFORM_ORIGIN/auth/me")"
test "$platform_me_status" = "200"
python3 - "$RUNTIME_DIR/platform-me.json" "$IDENTITY_ID" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["user"]["id"] == sys.argv[2]
assert data["user"]["authority"]["kind"] == "platform"
PY

TENANT_CREATE_BODY="$(printf '{"name":"P9 SP03 Tenant","slug":"%s"}' "$TENANT_SLUG")"
csrf_tenant_status="$(curl --silent --show-error --insecure \
  --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/tenant-create-csrf.json" \
  --write-out '%{http_code}' \
  -H 'X-Talos-Authority: platform' \
  -H 'Content-Type: application/json' \
  --data "$TENANT_CREATE_BODY" \
  "$PLATFORM_ORIGIN/api/tenants")"
test "$csrf_tenant_status" = "403"
grep -F 'AUTH_CSRF_REJECTED' "$RUNTIME_DIR/tenant-create-csrf.json" >/dev/null

tenant_create_status="$(curl --silent --show-error --insecure \
  --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/tenant-create.json" \
  --write-out '%{http_code}' \
  -H 'X-Talos-Authority: platform' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$TENANT_CREATE_BODY" \
  "$PLATFORM_ORIGIN/api/tenants")"
test "$tenant_create_status" = "200"
TENANT_ID="$(python3 - "$RUNTIME_DIR/tenant-create.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["status"] == "active"
print(data["id"])
PY
)"
test -n "$TENANT_ID"

platform_cookie_on_tenant_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/platform-cookie-tenant-host.json" \
  --write-out '%{http_code}' \
  "$TENANT_ORIGIN/auth/me")"
test "$platform_cookie_on_tenant_status" = "401"

TENANT_COOKIE="$RUNTIME_DIR/tenant.cookies"
TENANT_LOGIN_HEADERS="$RUNTIME_DIR/tenant-login.headers"
TENANT_LOGIN_BODY="$RUNTIME_DIR/tenant-login.json"

tenant_login_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --dump-header "$TENANT_LOGIN_HEADERS" \
  --cookie-jar "$TENANT_COOKIE" \
  --output "$TENANT_LOGIN_BODY" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H "X-Forwarded-For: $SPOOFED_IP" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")" \
  "$TENANT_ORIGIN/auth/login")"
test "$tenant_login_status" = "200"
grep -qi '^set-cookie:.*Secure' "$TENANT_LOGIN_HEADERS"
grep -qi '^set-cookie:.*HttpOnly' "$TENANT_LOGIN_HEADERS"
grep -qi '^set-cookie:.*SameSite=Lax' "$TENANT_LOGIN_HEADERS"

tenant_me_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/tenant-me.json" \
  --write-out '%{http_code}' \
  "$TENANT_ORIGIN/auth/me")"
test "$tenant_me_status" = "200"
python3 - "$RUNTIME_DIR/tenant-me.json" "$IDENTITY_ID" "$TENANT_ID" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["user"]["id"] == sys.argv[2]
authority=data["user"]["authority"]
assert authority["kind"] == "tenant"
assert authority["tenant_id"] == sys.argv[3]
assert authority["role"] == "owner"
PY

AUDIT_IP="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT detail_json #>> '{detail,ip}' FROM audit_events WHERE action='auth_login' AND actor_identity_id='${IDENTITY_ID}' AND tenant_id='${TENANT_ID}' ORDER BY occurred_at DESC LIMIT 1;")"
test -n "$AUDIT_IP"
test "$AUDIT_IP" != "$SPOOFED_IP"

MACHINE_PROVISION_BODY='{"name":"P9 SP03 machine","scopes":[{"module":"device","command":"list_devices"}],"rate_limit_rpm":60,"role":"staff"}'
machine_csrf_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/machine-provision-csrf.json" \
  --write-out '%{http_code}' \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_PROVISION_BODY" \
  "$TENANT_ORIGIN/api/machine-clients")"
test "$machine_csrf_status" = "403"
grep -F 'AUTH_CSRF_REJECTED' "$RUNTIME_DIR/machine-provision-csrf.json" >/dev/null

MACHINE_RESPONSE="$RUNTIME_DIR/machine-issued.json"
MACHINE_HEADERS="$RUNTIME_DIR/machine-issued.headers"
machine_provision_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --dump-header "$MACHINE_HEADERS" \
  --output "$MACHINE_RESPONSE" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_PROVISION_BODY" \
  "$TENANT_ORIGIN/api/machine-clients")"
test "$machine_provision_status" = "200"
grep -qi '^cache-control:.*no-store' "$MACHINE_HEADERS"

readarray -t MACHINE_VALUES < <(python3 - "$MACHINE_RESPONSE" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
for key in ("client_id","credential_id","secret"):
    value=data[key]
    assert isinstance(value,str) and value
    print(value)
PY
)
MACHINE_CLIENT_ID="${MACHINE_VALUES[0]}"
MACHINE_CREDENTIAL_ID="${MACHINE_VALUES[1]}"
MACHINE_SECRET="${MACHINE_VALUES[2]}"

MACHINE_EXEC_BODY='{"module":"device","command":"list_devices","payload":{}}'
MACHINE_EXEC_HEADERS="$RUNTIME_DIR/machine-exec.headers"
valid_machine_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --dump-header "$MACHINE_EXEC_HEADERS" \
  --output "$RUNTIME_DIR/machine-exec.json" \
  --write-out '%{http_code}' \
  -H "Authorization: Bearer $MACHINE_SECRET" \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_EXEC_BODY" \
  "$TENANT_ORIGIN/api/machine/v1/tenants/${TENANT_ID}/execute")"
test "$valid_machine_status" = "200"
grep -qi '^cache-control:.*no-store' "$MACHINE_EXEC_HEADERS"
grep -qi '^x-correlation-id:' "$MACHINE_EXEC_HEADERS"
python3 - "$RUNTIME_DIR/machine-exec.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("ok") is True
assert isinstance(data.get("result"), list)
PY

invalid_machine_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --output "$RUNTIME_DIR/machine-invalid.json" \
  --write-out '%{http_code}' \
  -H 'Authorization: Bearer invalid-machine-secret' \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_EXEC_BODY" \
  "$TENANT_ORIGIN/api/machine/v1/tenants/${TENANT_ID}/execute")"
test "$invalid_machine_status" = "401"

machine_cookie_lane_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --output "$RUNTIME_DIR/machine-cookie-lane.json" \
  --write-out '%{http_code}' \
  -H "Authorization: Bearer $MACHINE_SECRET" \
  -H 'Cookie: talos_session=browser-lane-forbidden' \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_EXEC_BODY" \
  "$TENANT_ORIGIN/api/machine/v1/tenants/${TENANT_ID}/execute")"
test "$machine_cookie_lane_status" = "403"

revoke_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/machine-revoke.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data '{}' \
  "$TENANT_ORIGIN/api/machine-clients/${MACHINE_CLIENT_ID}/credentials/${MACHINE_CREDENTIAL_ID}/revoke")"
test "$revoke_status" = "200"

revoked_machine_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --output "$RUNTIME_DIR/machine-revoked.json" \
  --write-out '%{http_code}' \
  -H "Authorization: Bearer $MACHINE_SECRET" \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_EXEC_BODY" \
  "$TENANT_ORIGIN/api/machine/v1/tenants/${TENANT_ID}/execute")"
test "$revoked_machine_status" = "401"

machine_admitted_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM audit_events WHERE action='machine.access' AND resource_id='${MACHINE_CLIENT_ID}' AND detail_json->>'outcome'='admitted';")"
test "$machine_admitted_count" -ge 1

cat > "$EVIDENCE_DIR/auth-summary.txt" <<EOF
source_sha=$(git rev-parse HEAD)
platform_invalid_login=401
platform_login=200
platform_cookie_secure=true
platform_cookie_http_only=true
platform_cookie_same_site_lax=true
platform_authority=platform
tenant_create_csrf=403
tenant_create=200
platform_cookie_tenant_host=401
tenant_login=200
tenant_cookie_secure=true
tenant_cookie_http_only=true
tenant_cookie_same_site_lax=true
tenant_authority=tenant_owner
trusted_proxy_spoof_rejected=true
machine_provision_csrf=403
machine_provision=200
machine_execute=200
machine_invalid_bearer=401
machine_cookie_lane=403
machine_revoke=200
machine_revoked_bearer=401
machine_admitted_audit_count=$machine_admitted_count
EOF

cat > "$EVIDENCE_DIR/authority-identities.txt" <<EOF
tenant_id=$TENANT_ID
machine_client_id=$MACHINE_CLIENT_ID
machine_credential_id=$MACHINE_CREDENTIAL_ID
EOF

echo "P9_SP03_DEPLOYED_AUTH source_sha=$(git rev-parse HEAD)"
echo "P9_SP03_DEPLOYED_AUTH human=pass csrf=pass cookie_scope=pass trusted_proxy=pass"
echo "P9_SP03_DEPLOYED_AUTH machine=pass invalid_bearer=fail_closed browser_lane=fail_closed revoked_bearer=fail_closed"
