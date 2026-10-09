#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p10-sp04"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
STATUS_FILE="$EVIDENCE_DIR/qualification-status.txt"
printf 'status=started\n' > "$STATUS_FILE"
qualification_status="fail"

RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p10-sp04.XXXXXX")"
chmod 700 "$RUNTIME_DIR"
mkdir -p "$RUNTIME_DIR/tls" "$RUNTIME_DIR/backup"
chmod 700 "$RUNTIME_DIR/tls" "$RUNTIME_DIR/backup"

run_id="${GITHUB_RUN_ID:-local}"
safe_run_id="$(printf '%s' "$run_id" | tr -cd 'A-Za-z0-9_.-')"
test -n "$safe_run_id"
PROJECT="talos-p10-sp04-${safe_run_id}"
PROJECT="$(printf '%s' "$PROJECT" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

PG_IMAGE="postgres:18-alpine@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2"
export DB_PASSWORD="$(openssl rand -hex 18)"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"
export P9_SP03_BOOTSTRAP_USERNAME="p10sp04-${safe_run_id}"
export P9_SP03_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"
export CORS_ALLOWED_ORIGIN="https://p10-sp04.invalid"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER="p10sp04-admin"
export GRAFANA_PASSWORD="$(openssl rand -hex 18)"
export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"
export TALOS_HTTP_PORT="18080"
export TALOS_HTTPS_PORT="18443"
export TALOS_PROMETHEUS_PORT="19090"
export TALOS_GRAFANA_PORT="13000"

openssl rand -hex 32 > "$METRICS_SCRAPE_TOKEN_FILE"
chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"

PLATFORM_HOST="p10-sp04.invalid"
PLATFORM_ORIGIN="https://${PLATFORM_HOST}"
PLATFORM_CONNECT="${PLATFORM_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
TENANT_A_SLUG="p10sp04-a-${safe_run_id}"
TENANT_B_SLUG="p10sp04-b-${safe_run_id}"
TENANT_A_HOST="${TENANT_A_SLUG}.talos.invalid"
TENANT_B_HOST="${TENANT_B_SLUG}.talos.invalid"
TENANT_A_ORIGIN="https://${TENANT_A_HOST}"
TENANT_B_ORIGIN="https://${TENANT_B_HOST}"
TENANT_A_CONNECT="${TENANT_A_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
TENANT_B_CONNECT="${TENANT_B_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"

PGPASS_FILE="$RUNTIME_DIR/pgpass"
BACKUP_KEY_FILE="$RUNTIME_DIR/backup-key"
printf 'db:5432:talos:talos:%s\n' "$DB_PASSWORD" > "$PGPASS_FILE"
chmod 600 "$PGPASS_FILE"
openssl rand -base64 48 > "$BACKUP_KEY_FILE"
chmod 600 "$BACKUP_KEY_FILE"

compose=(
  docker compose
  -p "$PROJECT"
  -f deploy/compose.production.yml
  -f deploy/compose.p9-sp03.yml
)

RESTORE_CONTAINER=""
RESTORE_NETWORK=""

scan_retained_evidence() {
  local leak=0
  local secret
  local backup_key_runtime=""

  if test -n "${BACKUP_KEY_FILE:-}" && test -f "$BACKUP_KEY_FILE"; then
    backup_key_runtime="$(cat "$BACKUP_KEY_FILE")"
  fi

  for secret in     "${DB_PASSWORD:-}"     "${RESTORE_PASSWORD:-}"     "${P9_SP03_BOOTSTRAP_PASSWORD:-}"     "${MACHINE_SECRET:-}"     "${backup_key_value:-}"     "$backup_key_runtime"
  do
    if test -n "$secret" && grep -R -F -l --binary-files=without-match "$secret" "$EVIDENCE_DIR" >/dev/null 2>&1; then
      echo "runtime secret leaked into retained SP04 evidence" >&2
      leak=1
    fi
  done

  if test -n "$(find "$EVIDENCE_DIR" -type f \( -name '*.dump' -o -name 'pgpass' -o -name 'restore-pgpass' -o -name 'backup-key' -o -name '*.key' -o -name '*.cookies' \) -print -quit)"; then
    echo "forbidden runtime material retained in SP04 evidence" >&2
    leak=1
  fi

  test "$leak" = "0"
}

cleanup() {
  exit_code=$?
  set +e
  set +u

  "${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-final.txt" 2>&1 || true
  "${compose[@]}" logs --no-color > "$EVIDENCE_DIR/compose.log" 2>&1 || true
  if test -n "$RESTORE_CONTAINER"; then
    docker logs "$RESTORE_CONTAINER" > "$EVIDENCE_DIR/restore-postgres.log" 2>&1 || true
    docker rm -f "$RESTORE_CONTAINER" >/dev/null 2>&1 || true
  fi
  "${compose[@]}" down -v --remove-orphans > "$EVIDENCE_DIR/compose-down.log" 2>&1 || true
  if test -n "$RESTORE_NETWORK"; then
    docker network rm "$RESTORE_NETWORK" >/dev/null 2>&1 || true
  fi

  if ! scan_retained_evidence; then
    exit_code=1
    qualification_status="fail"
  fi

  if test "$qualification_status" = "pass" && test "$exit_code" -eq 0; then
    printf 'status=pass\nexit_code=0\nevidence_scan=pass\n' > "$STATUS_FILE"
  else
    printf 'status=fail\nexit_code=%s\nevidence_scan=%s\n'       "$exit_code"       "$(if scan_retained_evidence >/dev/null 2>&1; then printf pass; else printf fail; fi)"       > "$STATUS_FILE"
  fi

  rm -rf "$RUNTIME_DIR"
  trap - EXIT
  exit "$exit_code"
}
trap cleanup EXIT

for command in docker curl openssl python3 sha256sum stat date cargo; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required qualification command unavailable: $command" >&2
    exit 1
  }
done

openssl req -x509 -newkey rsa:2048 -sha256 -days 1 -nodes \
  -keyout "$TLS_KEY_FILE" \
  -out "$TLS_CERT_FILE" \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
  >/dev/null 2>&1

docker version > "$EVIDENCE_DIR/docker-version.txt"
docker compose version > "$EVIDENCE_DIR/docker-compose-version.txt"
: "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"
: "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"
[[ "$TALOS_QUALIFIED_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$TALOS_QUALIFIED_SOURCE_TREE_SHA" =~ ^[0-9a-f]{40}$ ]]
git rev-parse HEAD > "$EVIDENCE_DIR/transport-sha.txt"
printf '%s\n' "$TALOS_QUALIFIED_SOURCE_SHA" > "$EVIDENCE_DIR/source-sha.txt"
printf '%s\n' "$TALOS_QUALIFIED_SOURCE_TREE_SHA" > "$EVIDENCE_DIR/source-tree-sha.txt"

mapfile -t migration_files < <(find backend/src/db/migrations/postgres -maxdepth 1 -type f -name '*.sql' -print | LC_ALL=C sort)
test "${#migration_files[@]}" -gt 0
migration_head="$(basename "${migration_files[${#migration_files[@]}-1]}")"
migration_registry_head="${migration_head%.sql}"
migration_manifest_sha256="$(
  for file in "${migration_files[@]}"; do
    printf '%s  %s\n' "$(sha256sum "$file" | awk '{print $1}')" "$file"
  done | sha256sum | awk '{print $1}'
)"
[[ "$migration_manifest_sha256" =~ ^[0-9a-f]{64}$ ]]
printf 'head=%s\nregistry_head=%s\nsha256=%s\n' "$migration_head" "$migration_registry_head" "$migration_manifest_sha256" > "$EVIDENCE_DIR/migration-manifest.txt"

"${compose[@]}" config >/dev/null
"${compose[@]}" config --images > "$EVIDENCE_DIR/compose-images.txt"
"${compose[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
"${compose[@]}" pull db > "$EVIDENCE_DIR/pull.log"
"${compose[@]}" build --pull app nginx > "$EVIDENCE_DIR/build.log"
"${compose[@]}" up -d --no-build db app nginx
"${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-started.txt"

wait_ready() {
  local output="$1"
  for _ in $(seq 1 90); do
    if curl --silent --show-error --fail --insecure --noproxy '*' \
      --connect-to "$PLATFORM_CONNECT" \
      "$PLATFORM_ORIGIN/ready" \
      -o "$output"
    then
      grep -F '"status":"ready"' "$output" >/dev/null &&
      grep -F '"database":"postgres"' "$output" >/dev/null &&
      return 0
    fi
    sleep 2
  done
  return 1
}
wait_ready "$EVIDENCE_DIR/ready-before-backup.json"

PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"
platform_login_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie-jar "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/platform-login.json" \
  --write-out '%{http_code}' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")" \
  "$PLATFORM_ORIGIN/auth/platform/login")"
test "$platform_login_status" = "200"
SOURCE_IDENTITY_ID="$(python3 - "$RUNTIME_DIR/platform-login.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("ok") is True
assert data["user"]["authority"]["kind"] == "platform"
print(data["user"]["id"])
PY
)"
test -n "$SOURCE_IDENTITY_ID"

create_tenant() {
  local slug="$1"
  local label="$2"
  local out="$3"
  local status
  status="$(curl --silent --show-error --insecure --noproxy '*' \
    --connect-to "$PLATFORM_CONNECT" \
    --cookie "$PLATFORM_COOKIE" \
    --output "$out" \
    --write-out '%{http_code}' \
    -H 'X-Talos-Authority: platform' \
    -H "Origin: $PLATFORM_ORIGIN" \
    -H 'Content-Type: application/json' \
    --data "$(printf '{"name":"%s","slug":"%s"}' "$label" "$slug")" \
    "$PLATFORM_ORIGIN/api/tenants")"
  test "$status" = "200"
  python3 - "$out" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["status"] == "active"
print(data["id"])
PY
}

TENANT_A_ID="$(create_tenant "$TENANT_A_SLUG" "P10 SP04 Tenant A" "$RUNTIME_DIR/tenant-a-create.json")"
TENANT_B_ID="$(create_tenant "$TENANT_B_SLUG" "P10 SP04 Tenant B" "$RUNTIME_DIR/tenant-b-create.json")"
test -n "$TENANT_A_ID"
test -n "$TENANT_B_ID"
test "$TENANT_A_ID" != "$TENANT_B_ID"

login_tenant() {
  local origin="$1"
  local connect="$2"
  local cookie="$3"
  local out="$4"
  local status
  status="$(curl --silent --show-error --insecure --noproxy '*' \
    --connect-to "$connect" \
    --cookie-jar "$cookie" \
    --output "$out" \
    --write-out '%{http_code}' \
    -H "Origin: $origin" \
    -H 'Content-Type: application/json' \
    --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")" \
    "$origin/auth/login")"
  test "$status" = "200"
}

TENANT_A_COOKIE="$RUNTIME_DIR/tenant-a.cookies"
TENANT_B_COOKIE="$RUNTIME_DIR/tenant-b.cookies"
login_tenant "$TENANT_A_ORIGIN" "$TENANT_A_CONNECT" "$TENANT_A_COOKIE" "$RUNTIME_DIR/tenant-a-login.json"
login_tenant "$TENANT_B_ORIGIN" "$TENANT_B_CONNECT" "$TENANT_B_COOKIE" "$RUNTIME_DIR/tenant-b-login.json"

MODEL_BODY="$(printf '{"name":"P10 SP04 Model %s","category":"qualification","prefix":"P10Q","enabled":true}' "$safe_run_id")"
model_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/model-create.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_A_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$MODEL_BODY" \
  "$TENANT_A_ORIGIN/api/device-models")"
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

MACHINE_PROVISION_BODY='{"name":"P10 SP04 backup writer","scopes":[{"module":"device","command":"create_device"}],"rate_limit_rpm":60,"role":"admin"}'
machine_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/machine-issued.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_A_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$MACHINE_PROVISION_BODY" \
  "$TENANT_A_ORIGIN/api/machine-clients")"
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

DEVICE_SERIAL="P10SP04${safe_run_id//[^0-9A-Za-z]/}A"
DEVICE_BODY="$(printf '{"module":"device","command":"create_device","payload":{"serialNo":"%s","modelId":"%s","warehouseId":"","status":"available"}}' "$DEVICE_SERIAL" "$MODEL_ID")"
MACHINE_HEADERS="$RUNTIME_DIR/machine-create.headers"
write_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --dump-header "$MACHINE_HEADERS" \
  --output "$RUNTIME_DIR/machine-create.json" \
  --write-out '%{http_code}' \
  -H "Authorization: Bearer $MACHINE_SECRET" \
  -H 'Content-Type: application/json' \
  --data "$DEVICE_BODY" \
  "$TENANT_A_ORIGIN/api/machine/v1/tenants/${TENANT_A_ID}/execute")"
test "$write_status" = "200"
CORRELATION_ID="$(awk 'tolower($1)=="x-correlation-id:" {gsub("\r","",$2); print $2}' "$MACHINE_HEADERS" | tail -n1)"
test -n "$CORRELATION_ID"

read_a_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/device-a.json" \
  --write-out '%{http_code}' \
  "$TENANT_A_ORIGIN/devices/${DEVICE_SERIAL}")"
test "$read_a_status" = "200"
read_b_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_B_CONNECT" \
  --cookie "$TENANT_B_COOKIE" \
  --output "$RUNTIME_DIR/device-b.json" \
  --write-out '%{http_code}' \
  "$TENANT_B_ORIGIN/devices/${DEVICE_SERIAL}")"
test "$read_b_status" = "200"
python3 - "$RUNTIME_DIR/device-a.json" "$RUNTIME_DIR/device-b.json" "$DEVICE_SERIAL" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    a=json.load(f)
with open(sys.argv[2], encoding='utf-8') as f:
    b=json.load(f)
assert a.get("serialNo") == sys.argv[3]
assert b is None
PY

MANIFEST_BODY='{"providerId":"fixture-p10-backup","version":"1.0.0","capabilities":["qualification.backup"],"configSchema":[{"name":"endpoint","required":true,"valueType":"https_origin"}],"secretSchema":[],"apiVersions":{"qualification.backup":"v1"},"webhookTypes":[],"simulationCapabilities":[],"readiness":"fixture","compatibility":{"qualification.backup":"backward_compatible"}}'
manifest_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/manifest.json" \
  --write-out '%{http_code}' \
  -H 'X-Talos-Authority: platform' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$MANIFEST_BODY" \
  "$PLATFORM_ORIGIN/api/integrations/manifests")"
test "$manifest_status" = "200"

INSTANCE_ID="p10-sp03-instance-${safe_run_id}"
BINDING_ID="p10-sp03-binding-${safe_run_id}"
INSTANCE_BODY="$(printf '{"id":"%s","providerId":"fixture-p10-backup","manifestVersion":"1.0.0","configRevision":"revision-1","config":{"endpoint":"https://fixture.invalid"},"secretRefs":{},"lifecycle":"active","health":"ready","readiness":"fixture"}' "$INSTANCE_ID")"
instance_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/instance.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_A_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$INSTANCE_BODY" \
  "$TENANT_A_ORIGIN/api/integrations/instances")"
test "$instance_status" = "200"

BINDING_BODY="$(printf '{"id":"%s","providerInstanceId":"%s","capability":"qualification.backup","configRevision":"revision-1","enabled":true}' "$BINDING_ID" "$INSTANCE_ID")"
binding_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/binding.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_A_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$BINDING_BODY" \
  "$TENANT_A_ORIGIN/api/integrations/bindings")"
test "$binding_status" = "200"

"${compose[@]}" exec -T db psql -U talos -d talos \
  -v tenant_id="$TENANT_A_ID" \
  -v binding_id="$BINDING_ID" <<'SQL'
\set ON_ERROR_STOP on
INSERT INTO integration_circuit_state
    (tenant_id,binding_id,state,failure_count,opened_until,updated_at)
VALUES
    (:'tenant_id',:'binding_id','open',1,'9999-12-31T23:59:59Z',CURRENT_TIMESTAMP::text)
ON CONFLICT (tenant_id,binding_id) DO UPDATE
SET state='open',
    failure_count=GREATEST(integration_circuit_state.failure_count,1),
    opened_until='9999-12-31T23:59:59Z',
    updated_at=CURRENT_TIMESTAMP::text;
SQL

OPERATION_BODY="$(printf '{"capability":"qualification.backup","operationType":"qualification","idempotencyKey":"p10-sp03-%s","requestHash":"p10-sp03-request-%s"}' "$safe_run_id" "$safe_run_id")"
operation_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/operation.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_A_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$OPERATION_BODY" \
  "$TENANT_A_ORIGIN/api/integrations/operations")"
test "$operation_status" = "200"
OPERATION_ID="$(python3 - "$RUNTIME_DIR/operation.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("state") == "ready"
value=data.get("externalOperationId")
assert isinstance(value,str) and value
print(value)
PY
)"

DB_CONTAINER="$("${compose[@]}" ps -q db)"
APP_CONTAINER="$("${compose[@]}" ps -q app)"
test -n "$DB_CONTAINER"
test -n "$APP_CONTAINER"
PG_SYSTEM_ID="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT system_identifier FROM pg_control_system();")"
SERVER_VERSION="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SHOW server_version_num;")"
MIGRATION_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM schema_migrations;")"
LATEST_MIGRATION="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1;")"
test -n "$PG_SYSTEM_ID"
test -n "$SERVER_VERSION"
test -n "$MIGRATION_COUNT"
test -n "$LATEST_MIGRATION"

"${compose[@]}" stop -t 20 nginx app
read -r APP_EXIT APP_OOM < <(docker inspect -f '{{.State.ExitCode}} {{.State.OOMKilled}}' "$APP_CONTAINER")
test "$APP_EXIT" = "0"
test "$APP_OOM" = "false"
docker logs "$APP_CONTAINER" > "$EVIDENCE_DIR/app-quiesce.log" 2>&1
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/app-quiesce.log" >/dev/null

WORKFLOW_ID="p10-sp04-workflow-${safe_run_id}"
WORKFLOW_SOURCE_ID="p10-sp04-recovery-order-${safe_run_id}"
OUTBOX_ID="p10-sp04-outbox-${safe_run_id}"
INBOX_ID="p10-sp04-inbox-${safe_run_id}"
PRE_MARKER="p10-sp04-pre-${safe_run_id}"
POST_MARKER="p10-sp04-post-${safe_run_id}"

"${compose[@]}" exec -T db psql -U talos -d talos \
  -v tenant_id="$TENANT_A_ID" \
  -v workflow_id="$WORKFLOW_ID" \
  -v workflow_source_id="$WORKFLOW_SOURCE_ID" \
  -v outbox_id="$OUTBOX_ID" \
  -v inbox_id="$INBOX_ID" \
  -v source_sha="$TALOS_QUALIFIED_SOURCE_SHA" \
  -v pre_marker="$PRE_MARKER" <<'SQL'
\set ON_ERROR_STOP on
BEGIN;
INSERT INTO workflow_instances
    (id,tenant_id,source_kind,source_id,definition_id,definition_version,definition_hash,status,created_at,updated_at)
VALUES
    (:'workflow_id',:'tenant_id','order',:'workflow_source_id','maxwell.rental.v1',1,
     'sha256:31baf0a8f2737e54c9bcce31801bdc416cb2c2f06921ef8da874b52383cf5f34',
     'active',CURRENT_TIMESTAMP::text,CURRENT_TIMESTAMP::text);
INSERT INTO workflow_steps
    (id,tenant_id,workflow_instance_id,step_key,sequence_no,state,attempt_count,next_eligible_at,lease_owner,lease_expires_at,last_error,idempotency_key,created_at,updated_at)
SELECT
    :'workflow_id' || '-step-' || sequence_no::text,
    :'tenant_id',
    :'workflow_id',
    step_key,
    sequence_no,
    CASE WHEN sequence_no = 0 THEN 'succeeded' ELSE 'pending' END,
    CASE WHEN sequence_no = 0 THEN 1 ELSE 0 END,
    NULL,NULL,NULL,NULL,
    'workflow:' || :'workflow_id' || ':' || step_key,
    CURRENT_TIMESTAMP::text,CURRENT_TIMESTAMP::text
FROM (VALUES
    (0,'order_confirmed'),
    (1,'contract_required'),
    (2,'payment_required'),
    (3,'reservation_confirmed'),
    (4,'allocation_complete'),
    (5,'shipment_create_requested'),
    (6,'shipment_delivered'),
    (7,'return_received'),
    (8,'inspection_complete'),
    (9,'risk_cases_resolved'),
    (10,'settlement_complete'),
    (11,'close_order')
) AS steps(sequence_no,step_key);
INSERT INTO domain_outbox
    (id,tenant_id,source_kind,source_id,message_type,idempotency_key,payload_json,payload_version,state,attempt_count,available_at,created_at,delivered_at,last_error)
VALUES
    (:'outbox_id',:'tenant_id','order',:'workflow_source_id','OrderConfirmed',
     'p10-sp04-outbox:' || :'workflow_source_id',
     json_build_object('orderId',:'workflow_source_id')::text,1,'delivered',1,
     CURRENT_TIMESTAMP::text,CURRENT_TIMESTAMP::text,CURRENT_TIMESTAMP::text,NULL);
INSERT INTO domain_inbox
    (id,tenant_id,message_id,message_type,payload_version,received_at,processed_at,state,last_error)
VALUES
    (:'inbox_id',:'tenant_id',:'outbox_id','OrderConfirmed',1,
     CURRENT_TIMESTAMP::text,CURRENT_TIMESTAMP::text,'processed',NULL);
CREATE TABLE IF NOT EXISTS p10_recovery_markers (
    marker_id TEXT PRIMARY KEY,
    marker_kind TEXT NOT NULL CHECK (marker_kind IN ('pre_backup','post_backup')),
    source_sha TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);
INSERT INTO p10_recovery_markers(marker_id,marker_kind,source_sha,tenant_id,created_at)
VALUES (:'pre_marker','pre_backup',:'source_sha',:'tenant_id',CURRENT_TIMESTAMP::text);
COMMIT;
SQL

CANONICAL_A_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${DEVICE_SERIAL}';")"
CANONICAL_B_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${DEVICE_SERIAL}';")"
MACHINE_AUDIT_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM audit_events WHERE action='machine.access' AND tenant_id='${TENANT_A_ID}' AND resource_id='${MACHINE_CLIENT_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'outcome'='admitted';")"
COMMAND_AUDIT_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM audit_events WHERE action='device.create_device' AND resource_type='command' AND tenant_id='${TENANT_A_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'result'='succeeded';")"
CROSS_TENANT_AUDIT_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM audit_events WHERE correlation_id='${CORRELATION_ID}' AND tenant_id='${TENANT_B_ID}';")"
WORKFLOW_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM workflow_instances WHERE tenant_id='${TENANT_A_ID}' AND id='${WORKFLOW_ID}' AND definition_id='maxwell.rental.v1';")"
WORKFLOW_STEP_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM workflow_steps WHERE tenant_id='${TENANT_A_ID}' AND workflow_instance_id='${WORKFLOW_ID}';")"
OUTBOX_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM domain_outbox WHERE tenant_id='${TENANT_A_ID}' AND id='${OUTBOX_ID}' AND state='delivered';")"
INBOX_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM domain_inbox WHERE tenant_id='${TENANT_A_ID}' AND id='${INBOX_ID}' AND state='processed';")"
OPERATION_STATE="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='${TENANT_A_ID}' AND id='${OPERATION_ID}';")"
CIRCUIT_STATE="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT state FROM integration_circuit_state WHERE tenant_id='${TENANT_A_ID}' AND binding_id='${BINDING_ID}';")"
DISPATCHING_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operations WHERE state='dispatching';")"
RUNNING_WORKFLOW_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM workflow_steps WHERE state='running';")"
PROCESSING_OUTBOX_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM domain_outbox WHERE state='processing';")"
PRE_MARKER_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_id='${PRE_MARKER}' AND marker_kind='pre_backup';")"
POST_MARKER_BEFORE_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_kind='post_backup';")"

test "$CANONICAL_A_COUNT" = "1"
test "$CANONICAL_B_COUNT" = "0"
test "$MACHINE_AUDIT_COUNT" = "1"
test "$COMMAND_AUDIT_COUNT" = "1"
test "$CROSS_TENANT_AUDIT_COUNT" = "0"
test "$WORKFLOW_COUNT" = "1"
test "$WORKFLOW_STEP_COUNT" = "12"
test "$OUTBOX_COUNT" = "1"
test "$INBOX_COUNT" = "1"
test "$OPERATION_STATE" = "ready|0"
test "$CIRCUIT_STATE" = "open"
test "$DISPATCHING_COUNT" = "0"
test "$RUNNING_WORKFLOW_COUNT" = "0"
test "$PROCESSING_OUTBOX_COUNT" = "0"
test "$PRE_MARKER_COUNT" = "1"
test "$POST_MARKER_BEFORE_COUNT" = "0"

DB_NETWORK="$(docker inspect -f '{{range $name, $_ := .NetworkSettings.Networks}}{{println $name}}{{end}}' "$DB_CONTAINER" | head -n1)"
test -n "$DB_NETWORK"
export P10_PG_NETWORK="$DB_NETWORK"
export P10_PG_HOST="db"
export P10_PG_PORT="5432"
export P10_PG_DATABASE="talos"
export P10_PG_USER="talos"
export P10_PGPASS_FILE="$PGPASS_FILE"
export P10_BACKUP_KEY_FILE="$BACKUP_KEY_FILE"
export P10_BACKUP_OUTPUT_DIR="$RUNTIME_DIR/backup"
export P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"
export P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"
export P10_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"
export P10_MIGRATION_HEAD="$migration_head"
export P10_OPERATOR_ID="${GITHUB_ACTOR:-local-operator}"
export P10_RUN_ID="sp04-source-${safe_run_id}"
export P10_PG_CLIENT_IMAGE="$PG_IMAGE"

backup_start_ns="$(date +%s%N)"
bash scripts/r4-p10-pg-backup.sh > "$EVIDENCE_DIR/protected-backup.log" 2>&1
backup_end_ns="$(date +%s%N)"
backup_duration_ms="$(( (backup_end_ns - backup_start_ns) / 1000000 ))"

readarray -t artifacts < <(find "$RUNTIME_DIR/backup" -maxdepth 1 -type f -name '*.dump.gpg' -print)
readarray -t metadata_files < <(find "$RUNTIME_DIR/backup" -maxdepth 1 -type f -name '*.metadata.json' -print)
test "${#artifacts[@]}" = "1"
test "${#metadata_files[@]}" = "1"
BACKUP_ARTIFACT="${artifacts[0]}"
BACKUP_METADATA="${metadata_files[0]}"
test -z "$(find "$RUNTIME_DIR/backup" -maxdepth 1 -type f -name '*.dump' -print -quit)"

export P10_BACKUP_ARTIFACT="$BACKUP_ARTIFACT"
export P10_BACKUP_METADATA="$BACKUP_METADATA"
verify_start_ns="$(date +%s%N)"
bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/protected-verify-before-post-marker.log" 2>&1
verify_end_ns="$(date +%s%N)"
verify_duration_ms="$(( (verify_end_ns - verify_start_ns) / 1000000 ))"

CIPHERTEXT_SHA_BEFORE="$(sha256sum "$BACKUP_ARTIFACT" | awk '{print $1}')"
"${compose[@]}" exec -T db psql -U talos -d talos \
  -v tenant_id="$TENANT_A_ID" \
  -v source_sha="$TALOS_QUALIFIED_SOURCE_SHA" \
  -v post_marker="$POST_MARKER" <<'SQL'
\set ON_ERROR_STOP on
INSERT INTO p10_recovery_markers(marker_id,marker_kind,source_sha,tenant_id,created_at)
VALUES (:'post_marker','post_backup',:'source_sha',:'tenant_id',CURRENT_TIMESTAMP::text);
SQL
POST_MARKER_AFTER_COUNT="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_id='${POST_MARKER}' AND marker_kind='post_backup';")"
test "$POST_MARKER_AFTER_COUNT" = "1"

CIPHERTEXT_SHA_AFTER="$(sha256sum "$BACKUP_ARTIFACT" | awk '{print $1}')"
test "$CIPHERTEXT_SHA_AFTER" = "$CIPHERTEXT_SHA_BEFORE"
bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/protected-verify-after-post-marker.log" 2>&1

cp "$BACKUP_ARTIFACT" "$EVIDENCE_DIR/"
cp "$BACKUP_METADATA" "$EVIDENCE_DIR/"
cp "$RUNTIME_DIR/tenant-a-create.json" "$EVIDENCE_DIR/tenant-a-create.json"
cp "$RUNTIME_DIR/tenant-b-create.json" "$EVIDENCE_DIR/tenant-b-create.json"

python3 - "$BACKUP_METADATA" "$MIGRATION_COUNT" "$LATEST_MIGRATION" "$P10_SOURCE_SHA" > "$EVIDENCE_DIR/backup-metadata-summary.txt" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["source_sha"] == sys.argv[4]
assert str(data["migration"]["count"]) == sys.argv[2]
assert data["migration"]["latest_id"] == sys.argv[3]
print("schema="+data["schema"])
print("source_sha="+data["source_sha"])
print("source_database="+data["source"]["database"])
print("server_version_num="+data["source"]["server_version_num"])
print("migration_count="+str(data["migration"]["count"]))
print("latest_migration="+data["migration"]["latest_id"])
print("protected_sha256="+data["protected_artifact"]["sha256"])
print("encryption_policy="+data["protected_artifact"]["encryption_policy"])
PY

cat > "$EVIDENCE_DIR/source-rehearsal-summary.txt" <<EOF
source_sha=$TALOS_QUALIFIED_SOURCE_SHA
source_tree=$TALOS_QUALIFIED_SOURCE_TREE_SHA
transport_sha=$(git rev-parse HEAD)
repository_migration_head=$migration_head
repository_migration_manifest_sha256=$migration_manifest_sha256
postgres_container=$DB_CONTAINER
postgres_system_identifier=$PG_SYSTEM_ID
postgres_server_version_num=$SERVER_VERSION
migration_count=$MIGRATION_COUNT
latest_migration=$LATEST_MIGRATION
tenant_a_id=$TENANT_A_ID
tenant_b_id=$TENANT_B_ID
tenant_a_device_rows=$CANONICAL_A_COUNT
tenant_b_same_device_rows=$CANONICAL_B_COUNT
machine_admission_audit_rows=$MACHINE_AUDIT_COUNT
command_success_audit_rows=$COMMAND_AUDIT_COUNT
cross_tenant_audit_rows=$CROSS_TENANT_AUDIT_COUNT
workflow_instances=$WORKFLOW_COUNT
workflow_steps=$WORKFLOW_STEP_COUNT
domain_outbox_delivered=$OUTBOX_COUNT
domain_inbox_processed=$INBOX_COUNT
integration_operation=$OPERATION_STATE
integration_circuit=$CIRCUIT_STATE
quiescence_external_dispatching=$DISPATCHING_COUNT
quiescence_workflow_running=$RUNNING_WORKFLOW_COUNT
quiescence_outbox_processing=$PROCESSING_OUTBOX_COUNT
pre_backup_marker=$PRE_MARKER_COUNT
post_backup_marker_before_cut=$POST_MARKER_BEFORE_COUNT
post_backup_marker_after_cut=$POST_MARKER_AFTER_COUNT
backup_duration_ms=$backup_duration_ms
verify_duration_ms=$verify_duration_ms
ciphertext_sha256_before_post_marker=$CIPHERTEXT_SHA_BEFORE
ciphertext_sha256_after_post_marker=$CIPHERTEXT_SHA_AFTER
plaintext_retained=false
EOF

SOURCE_SNAPSHOT_BEFORE="$CANONICAL_A_COUNT|$CANONICAL_B_COUNT|$MACHINE_AUDIT_COUNT|$COMMAND_AUDIT_COUNT|$CROSS_TENANT_AUDIT_COUNT|$WORKFLOW_COUNT|$WORKFLOW_STEP_COUNT|$OUTBOX_COUNT|$INBOX_COUNT|$OPERATION_STATE|$CIRCUIT_STATE|$POST_MARKER_AFTER_COUNT|$PG_SYSTEM_ID"

RESTORE_NETWORK="talos-p10-sp04-restore-${safe_run_id}"
RESTORE_NETWORK="$(printf '%s' "$RESTORE_NETWORK" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"
RESTORE_CONTAINER="talos-p10-sp04-restore-db-${safe_run_id}"
RESTORE_CONTAINER="$(printf '%s' "$RESTORE_CONTAINER" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_.-')"
RESTORE_PASSWORD="$(openssl rand -hex 18)"
RESTORE_PGPASS_FILE="$RUNTIME_DIR/restore-pgpass"
printf '%s:5432:talos:talos:%s\n' "$RESTORE_CONTAINER" "$RESTORE_PASSWORD" > "$RESTORE_PGPASS_FILE"
chmod 600 "$RESTORE_PGPASS_FILE"

docker network create "$RESTORE_NETWORK" > "$EVIDENCE_DIR/restore-network.txt"
docker run -d \
  --name "$RESTORE_CONTAINER" \
  --network "$RESTORE_NETWORK" \
  -p 127.0.0.1::5432 \
  -e POSTGRES_USER=talos \
  -e POSTGRES_PASSWORD="$RESTORE_PASSWORD" \
  -e POSTGRES_DB=talos \
  "$PG_IMAGE" > "$EVIDENCE_DIR/restore-container-id.txt"

for _ in $(seq 1 60); do
  if docker exec "$RESTORE_CONTAINER" pg_isready -U talos -d talos >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$RESTORE_CONTAINER" pg_isready -U talos -d talos >/dev/null

RESTORE_CONTAINER_ID="$(docker inspect -f '{{.Id}}' "$RESTORE_CONTAINER")"
RESTORE_SYSTEM_ID="$(docker exec "$RESTORE_CONTAINER" psql -U talos -d talos -Atc "SELECT system_identifier FROM pg_control_system();")"
test -n "$RESTORE_CONTAINER_ID"
test -n "$RESTORE_SYSTEM_ID"
test "$RESTORE_CONTAINER_ID" != "$DB_CONTAINER"
test "$RESTORE_SYSTEM_ID" != "$PG_SYSTEM_ID"

RESTORE_HOST_PORT="$(docker port "$RESTORE_CONTAINER" 5432/tcp | awk -F: '/127\.0\.0\.1/ {print $NF; exit}')"
test -n "$RESTORE_HOST_PORT"

RESTORE_PUBLIC_TABLES_BEFORE="$(docker exec "$RESTORE_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM pg_catalog.pg_tables WHERE schemaname='public';")"
RESTORE_SCHEMA_MIGRATIONS_ABSENT_BEFORE="$(docker exec "$RESTORE_CONTAINER" psql -U talos -d talos -Atc "SELECT to_regclass('public.schema_migrations') IS NULL;")"
test "$RESTORE_PUBLIC_TABLES_BEFORE" = "0"
test "$RESTORE_SCHEMA_MIGRATIONS_ABSENT_BEFORE" = "t"

export P10_BACKUP_ARTIFACT="$BACKUP_ARTIFACT"
export P10_BACKUP_METADATA="$BACKUP_METADATA"
export P10_BACKUP_KEY_FILE="$BACKUP_KEY_FILE"
export P10_PG_CLIENT_IMAGE="$PG_IMAGE"

# The protected artifact is independently reverified immediately before any
# restore-target database mutation.
bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/protected-verify-before-restore.log" 2>&1

export P10_RESTORE_NETWORK="$RESTORE_NETWORK"
export P10_RESTORE_HOST="$RESTORE_CONTAINER"
export P10_RESTORE_PORT="5432"
export P10_RESTORE_DATABASE="talos"
export P10_RESTORE_USER="talos"
export P10_RESTORE_PGPASS_FILE="$RESTORE_PGPASS_FILE"
export P10_EXPECTED_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"
export P10_EXPECTED_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"
export P10_EXPECTED_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"
export P10_EXPECTED_MIGRATION_HEAD="$migration_head"

restore_start_ns="$(date +%s%N)"
bash scripts/r4-p10-pg-restore.sh > "$EVIDENCE_DIR/restore.log" 2>&1
restore_end_ns="$(date +%s%N)"
restore_duration_ms="$(( (restore_end_ns - restore_start_ns) / 1000000))"

restore_sql() {
  docker exec "$RESTORE_CONTAINER" psql -U talos -d talos -Atc "$1"
}

RESTORE_MIGRATION_COUNT_BEFORE="$(restore_sql "SELECT COUNT(*) FROM schema_migrations;")"
RESTORE_LATEST_MIGRATION_BEFORE="$(restore_sql "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1;")"
test "$RESTORE_MIGRATION_COUNT_BEFORE" = "$MIGRATION_COUNT"
test "$LATEST_MIGRATION" = "$migration_registry_head"
test "$RESTORE_LATEST_MIGRATION_BEFORE" = "$migration_registry_head"

RESTORE_TENANT_A_COUNT="$(restore_sql "SELECT COUNT(*) FROM tenants WHERE id='${TENANT_A_ID}';")"
RESTORE_TENANT_B_COUNT="$(restore_sql "SELECT COUNT(*) FROM tenants WHERE id='${TENANT_B_ID}';")"
RESTORE_DEVICE_A_COUNT="$(restore_sql "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${DEVICE_SERIAL}';")"
RESTORE_DEVICE_B_COUNT="$(restore_sql "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${DEVICE_SERIAL}';")"
RESTORE_MACHINE_AUDIT_COUNT="$(restore_sql "SELECT COUNT(*) FROM audit_events WHERE action='machine.access' AND tenant_id='${TENANT_A_ID}' AND resource_id='${MACHINE_CLIENT_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'outcome'='admitted';")"
RESTORE_COMMAND_AUDIT_COUNT="$(restore_sql "SELECT COUNT(*) FROM audit_events WHERE action='device.create_device' AND resource_type='command' AND tenant_id='${TENANT_A_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'result'='succeeded';")"
RESTORE_CROSS_TENANT_AUDIT_COUNT="$(restore_sql "SELECT COUNT(*) FROM audit_events WHERE correlation_id='${CORRELATION_ID}' AND tenant_id='${TENANT_B_ID}';")"
RESTORE_WORKFLOW_COUNT="$(restore_sql "SELECT COUNT(*) FROM workflow_instances WHERE tenant_id='${TENANT_A_ID}' AND id='${WORKFLOW_ID}' AND definition_id='maxwell.rental.v1';")"
RESTORE_WORKFLOW_STEP_COUNT="$(restore_sql "SELECT COUNT(*) FROM workflow_steps WHERE tenant_id='${TENANT_A_ID}' AND workflow_instance_id='${WORKFLOW_ID}';")"
RESTORE_OUTBOX_COUNT="$(restore_sql "SELECT COUNT(*) FROM domain_outbox WHERE tenant_id='${TENANT_A_ID}' AND id='${OUTBOX_ID}' AND state='delivered';")"
RESTORE_INBOX_COUNT="$(restore_sql "SELECT COUNT(*) FROM domain_inbox WHERE tenant_id='${TENANT_A_ID}' AND id='${INBOX_ID}' AND state='processed';")"
RESTORE_OPERATION_STATE="$(restore_sql "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='${TENANT_A_ID}' AND id='${OPERATION_ID}';")"
RESTORE_CIRCUIT_STATE="$(restore_sql "SELECT state FROM integration_circuit_state WHERE tenant_id='${TENANT_A_ID}' AND binding_id='${BINDING_ID}';")"
RESTORE_PRE_MARKER_COUNT="$(restore_sql "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_id='${PRE_MARKER}' AND marker_kind='pre_backup';")"
RESTORE_POST_MARKER_COUNT="$(restore_sql "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_kind='post_backup';")"

test "$RESTORE_TENANT_A_COUNT" = "1"
test "$RESTORE_TENANT_B_COUNT" = "1"
test "$RESTORE_DEVICE_A_COUNT" = "1"
test "$RESTORE_DEVICE_B_COUNT" = "0"
test "$RESTORE_MACHINE_AUDIT_COUNT" = "1"
test "$RESTORE_COMMAND_AUDIT_COUNT" = "1"
test "$RESTORE_CROSS_TENANT_AUDIT_COUNT" = "0"
test "$RESTORE_WORKFLOW_COUNT" = "1"
test "$RESTORE_WORKFLOW_STEP_COUNT" = "12"
test "$RESTORE_OUTBOX_COUNT" = "1"
test "$RESTORE_INBOX_COUNT" = "1"
test "$RESTORE_OPERATION_STATE" = "ready|0"
test "$RESTORE_CIRCUIT_STATE" = "open"
test "$RESTORE_PRE_MARKER_COUNT" = "1"
test "$RESTORE_POST_MARKER_COUNT" = "0"

export TALOS_P10_RESTORE_DATABASE_URL="postgresql://talos:${RESTORE_PASSWORD}@127.0.0.1:${RESTORE_HOST_PORT}/talos"
export TALOS_P10_EXPECTED_MIGRATION_ID="$migration_registry_head"
cargo test \
  --manifest-path backend/Cargo.toml \
  --features postgres \
  --bin talos-backend \
  restored_pg18_migration_chain_is_idempotent \
  --locked \
  -- --ignored --nocapture > "$EVIDENCE_DIR/migration-idempotency.log" 2>&1
grep -F 'P10_RESTORE_MIGRATION_IDEMPOTENT' "$EVIDENCE_DIR/migration-idempotency.log" >/dev/null
unset TALOS_P10_RESTORE_DATABASE_URL TALOS_P10_EXPECTED_MIGRATION_ID

RESTORE_MIGRATION_COUNT_AFTER="$(restore_sql "SELECT COUNT(*) FROM schema_migrations;")"
RESTORE_LATEST_MIGRATION_AFTER="$(restore_sql "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1;")"
test "$RESTORE_MIGRATION_COUNT_AFTER" = "$RESTORE_MIGRATION_COUNT_BEFORE"
test "$RESTORE_LATEST_MIGRATION_AFTER" = "$RESTORE_LATEST_MIGRATION_BEFORE"

SOURCE_CANONICAL_A_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${DEVICE_SERIAL}';")"
SOURCE_CANONICAL_B_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${DEVICE_SERIAL}';")"
SOURCE_MACHINE_AUDIT_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM audit_events WHERE action='machine.access' AND tenant_id='${TENANT_A_ID}' AND resource_id='${MACHINE_CLIENT_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'outcome'='admitted';")"
SOURCE_COMMAND_AUDIT_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM audit_events WHERE action='device.create_device' AND resource_type='command' AND tenant_id='${TENANT_A_ID}' AND correlation_id='${CORRELATION_ID}' AND detail_json->>'result'='succeeded';")"
SOURCE_CROSS_AUDIT_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM audit_events WHERE correlation_id='${CORRELATION_ID}' AND tenant_id='${TENANT_B_ID}';")"
SOURCE_WORKFLOW_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM workflow_instances WHERE tenant_id='${TENANT_A_ID}' AND id='${WORKFLOW_ID}' AND definition_id='maxwell.rental.v1';")"
SOURCE_WORKFLOW_STEPS_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM workflow_steps WHERE tenant_id='${TENANT_A_ID}' AND workflow_instance_id='${WORKFLOW_ID}';")"
SOURCE_OUTBOX_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM domain_outbox WHERE tenant_id='${TENANT_A_ID}' AND id='${OUTBOX_ID}' AND state='delivered';")"
SOURCE_INBOX_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM domain_inbox WHERE tenant_id='${TENANT_A_ID}' AND id='${INBOX_ID}' AND state='processed';")"
SOURCE_OPERATION_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='${TENANT_A_ID}' AND id='${OPERATION_ID}';")"
SOURCE_CIRCUIT_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT state FROM integration_circuit_state WHERE tenant_id='${TENANT_A_ID}' AND binding_id='${BINDING_ID}';")"
SOURCE_POST_MARKER_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_id='${POST_MARKER}' AND marker_kind='post_backup';")"
SOURCE_SYSTEM_ID_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT system_identifier FROM pg_control_system();")"

SOURCE_SNAPSHOT_AFTER="$SOURCE_CANONICAL_A_AFTER|$SOURCE_CANONICAL_B_AFTER|$SOURCE_MACHINE_AUDIT_AFTER|$SOURCE_COMMAND_AUDIT_AFTER|$SOURCE_CROSS_AUDIT_AFTER|$SOURCE_WORKFLOW_AFTER|$SOURCE_WORKFLOW_STEPS_AFTER|$SOURCE_OUTBOX_AFTER|$SOURCE_INBOX_AFTER|$SOURCE_OPERATION_AFTER|$SOURCE_CIRCUIT_AFTER|$SOURCE_POST_MARKER_AFTER|$SOURCE_SYSTEM_ID_AFTER"
test "$SOURCE_SNAPSHOT_AFTER" = "$SOURCE_SNAPSHOT_BEFORE"

# Bind the exact production app/nginx profile to the restored PostgreSQL authority.
# Source PostgreSQL remains running for local docker-exec comparison but is removed
# from the production backend network so "db" can only resolve to the restored authority.
docker network disconnect "$DB_NETWORK" "$DB_CONTAINER"
SOURCE_DB_BACKEND_ATTACHED="$(docker inspect -f '{{if index .NetworkSettings.Networks "'"$DB_NETWORK"'"}}yes{{else}}no{{end}}' "$DB_CONTAINER")"
test "$SOURCE_DB_BACKEND_ATTACHED" = "no"

docker network connect --alias db --ip 172.29.0.10 "$DB_NETWORK" "$RESTORE_CONTAINER"
RESTORE_DB_BACKEND_ATTACHED="$(docker inspect -f '{{if index .NetworkSettings.Networks "'"$DB_NETWORK"'"}}yes{{else}}no{{end}}' "$RESTORE_CONTAINER")"
test "$RESTORE_DB_BACKEND_ATTACHED" = "yes"

export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${RESTORE_PASSWORD}@db:5432/talos"
RESTORE_IDENTITY_COUNT_BEFORE_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM identities;")"
RESTORE_MACHINE_CLIENT_COUNT_BEFORE_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM machine_clients WHERE tenant_id='${TENANT_A_ID}' AND id='${MACHINE_CLIENT_ID}';")"
test "$RESTORE_IDENTITY_COUNT_BEFORE_RUNTIME" -ge 1
test "$RESTORE_MACHINE_CLIENT_COUNT_BEFORE_RUNTIME" = "1"

"${compose[@]}" up -d --no-deps --force-recreate app nginx
RESTORED_APP_CONTAINER="$("${compose[@]}" ps -q app)"
test -n "$RESTORED_APP_CONTAINER"
test "$RESTORED_APP_CONTAINER" != "$APP_CONTAINER"

wait_ready "$EVIDENCE_DIR/ready-restored.json"
grep -F '"status":"ready"' "$EVIDENCE_DIR/ready-restored.json" >/dev/null
grep -F '"database":"postgres"' "$EVIDENCE_DIR/ready-restored.json" >/dev/null

docker logs "$RESTORED_APP_CONTAINER" > "$EVIDENCE_DIR/restored-app-startup.log" 2>&1
grep -F 'Database profile: Postgres18' "$EVIDENCE_DIR/restored-app-startup.log" >/dev/null
grep -F 'PostgreSQL connection established' "$EVIDENCE_DIR/restored-app-startup.log" >/dev/null
grep -F 'No pending PG migrations' "$EVIDENCE_DIR/restored-app-startup.log" >/dev/null
! grep -F 'SQLite local database path' "$EVIDENCE_DIR/restored-app-startup.log" >/dev/null

RESTORED_PLATFORM_COOKIE="$RUNTIME_DIR/restored-platform.cookies"
restored_platform_login_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie-jar "$RESTORED_PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/restored-platform-login.json" \
  --write-out '%{http_code}' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")" \
  "$PLATFORM_ORIGIN/auth/platform/login")"
test "$restored_platform_login_status" = "200"
RESTORED_IDENTITY_ID="$(python3 - "$RUNTIME_DIR/restored-platform-login.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data.get("ok") is True
assert data["user"]["authority"]["kind"] == "platform"
print(data["user"]["id"])
PY
)"
test -n "$RESTORED_IDENTITY_ID"
test "$RESTORED_IDENTITY_ID" = "$SOURCE_IDENTITY_ID"

RESTORED_TENANT_A_COOKIE="$RUNTIME_DIR/restored-tenant-a.cookies"
RESTORED_TENANT_B_COOKIE="$RUNTIME_DIR/restored-tenant-b.cookies"
login_tenant "$TENANT_A_ORIGIN" "$TENANT_A_CONNECT" "$RESTORED_TENANT_A_COOKIE" "$RUNTIME_DIR/restored-tenant-a-login.json"
login_tenant "$TENANT_B_ORIGIN" "$TENANT_B_CONNECT" "$RESTORED_TENANT_B_COOKIE" "$RUNTIME_DIR/restored-tenant-b-login.json"

restored_tenant_me_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$RESTORED_TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/restored-tenant-a-me.json" \
  --write-out '%{http_code}' \
  "$TENANT_A_ORIGIN/auth/me")"
test "$restored_tenant_me_status" = "200"
python3 - "$RUNTIME_DIR/restored-tenant-a-me.json" "$RESTORED_IDENTITY_ID" "$TENANT_A_ID" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
assert data["user"]["id"] == sys.argv[2]
authority=data["user"]["authority"]
assert authority["kind"] == "tenant"
assert authority["tenant_id"] == sys.argv[3]
assert authority["role"] == "owner"
PY

# Read pre-backup business state through the restored application.
restored_existing_a_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$RESTORED_TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/restored-existing-a.json" \
  --write-out '%{http_code}' \
  "$TENANT_A_ORIGIN/devices/${DEVICE_SERIAL}")"
test "$restored_existing_a_status" = "200"
restored_existing_b_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_B_CONNECT" \
  --cookie "$RESTORED_TENANT_B_COOKIE" \
  --output "$RUNTIME_DIR/restored-existing-b.json" \
  --write-out '%{http_code}' \
  "$TENANT_B_ORIGIN/devices/${DEVICE_SERIAL}")"
test "$restored_existing_b_status" = "200"
python3 - "$RUNTIME_DIR/restored-existing-a.json" "$RUNTIME_DIR/restored-existing-b.json" "$DEVICE_SERIAL" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    a=json.load(f)
with open(sys.argv[2], encoding='utf-8') as f:
    b=json.load(f)
assert a.get("serialNo") == sys.argv[3]
assert b is None
PY

# Reuse the restored pre-backup machine credential. This proves credential state
# survived restore and that the new write is executed by the restored runtime.
RUNTIME_DEVICE_SERIAL="P10SP04RT${safe_run_id//[^0-9A-Za-z]/}A"
RUNTIME_DEVICE_BODY="$(printf '{"module":"device","command":"create_device","payload":{"serialNo":"%s","modelId":"%s","warehouseId":"","status":"available"}}' "$RUNTIME_DEVICE_SERIAL" "$MODEL_ID")"
RUNTIME_MACHINE_HEADERS="$RUNTIME_DIR/restored-machine-create.headers"
runtime_write_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --dump-header "$RUNTIME_MACHINE_HEADERS" \
  --output "$RUNTIME_DIR/restored-machine-create.json" \
  --write-out '%{http_code}' \
  -H "Authorization: Bearer $MACHINE_SECRET" \
  -H 'Content-Type: application/json' \
  --data "$RUNTIME_DEVICE_BODY" \
  "$TENANT_A_ORIGIN/api/machine/v1/tenants/${TENANT_A_ID}/execute")"
test "$runtime_write_status" = "200"
RUNTIME_CORRELATION_ID="$(awk 'tolower($1)=="x-correlation-id:" {gsub("\r","",$2); print $2}' "$RUNTIME_MACHINE_HEADERS" | tail -n1)"
test -n "$RUNTIME_CORRELATION_ID"

restored_new_a_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_A_CONNECT" \
  --cookie "$RESTORED_TENANT_A_COOKIE" \
  --output "$RUNTIME_DIR/restored-new-a.json" \
  --write-out '%{http_code}' \
  "$TENANT_A_ORIGIN/devices/${RUNTIME_DEVICE_SERIAL}")"
test "$restored_new_a_status" = "200"
restored_new_b_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_B_CONNECT" \
  --cookie "$RESTORED_TENANT_B_COOKIE" \
  --output "$RUNTIME_DIR/restored-new-b.json" \
  --write-out '%{http_code}' \
  "$TENANT_B_ORIGIN/devices/${RUNTIME_DEVICE_SERIAL}")"
test "$restored_new_b_status" = "200"
python3 - "$RUNTIME_DIR/restored-new-a.json" "$RUNTIME_DIR/restored-new-b.json" "$RUNTIME_DEVICE_SERIAL" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    a=json.load(f)
with open(sys.argv[2], encoding='utf-8') as f:
    b=json.load(f)
assert a.get("serialNo") == sys.argv[3]
assert b is None
PY

RESTORED_NEW_DEVICE_A_COUNT="$(restore_sql "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_A_ID}' AND serialNo='${RUNTIME_DEVICE_SERIAL}';")"
RESTORED_NEW_DEVICE_B_COUNT="$(restore_sql "SELECT COUNT(*) FROM devices WHERE tenant_id='${TENANT_B_ID}' AND serialNo='${RUNTIME_DEVICE_SERIAL}';")"
RESTORED_MACHINE_AUDIT_NEW="$(restore_sql "SELECT COUNT(*) FROM audit_events WHERE action='machine.access' AND tenant_id='${TENANT_A_ID}' AND resource_id='${MACHINE_CLIENT_ID}' AND correlation_id='${RUNTIME_CORRELATION_ID}' AND detail_json->>'outcome'='admitted';")"
RESTORED_COMMAND_AUDIT_NEW="$(restore_sql "SELECT COUNT(*) FROM audit_events WHERE action='device.create_device' AND resource_type='command' AND tenant_id='${TENANT_A_ID}' AND correlation_id='${RUNTIME_CORRELATION_ID}' AND detail_json->>'result'='succeeded' AND detail_json->>'execution_mode'='Normal' AND detail_json #>> '{data_scope,namespace}'='Production';")"
RESTORED_CROSS_AUDIT_NEW="$(restore_sql "SELECT COUNT(*) FROM audit_events WHERE correlation_id='${RUNTIME_CORRELATION_ID}' AND tenant_id='${TENANT_B_ID}';")"
SOURCE_NEW_DEVICE_COUNT="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM devices WHERE serialNo='${RUNTIME_DEVICE_SERIAL}';")"
SOURCE_RUNTIME_CORRELATION_COUNT="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM audit_events WHERE correlation_id='${RUNTIME_CORRELATION_ID}';")"

test "$RESTORED_NEW_DEVICE_A_COUNT" = "1"
test "$RESTORED_NEW_DEVICE_B_COUNT" = "0"
test "$RESTORED_MACHINE_AUDIT_NEW" = "1"
test "$RESTORED_COMMAND_AUDIT_NEW" = "1"
test "$RESTORED_CROSS_AUDIT_NEW" = "0"
test "$SOURCE_NEW_DEVICE_COUNT" = "0"
test "$SOURCE_RUNTIME_CORRELATION_COUNT" = "0"

# Give periodic workers a bounded observation window, then require no abandoned claims.
for _ in $(seq 1 30); do
  RESTORED_RUNNING_WORKFLOW="$(restore_sql "SELECT COUNT(*) FROM workflow_steps WHERE state='running';")"
  RESTORED_PROCESSING_OUTBOX="$(restore_sql "SELECT COUNT(*) FROM domain_outbox WHERE state='processing';")"
  if test "$RESTORED_RUNNING_WORKFLOW" = "0" && test "$RESTORED_PROCESSING_OUTBOX" = "0"; then
    break
  fi
  sleep 1
done
test "$RESTORED_RUNNING_WORKFLOW" = "0"
test "$RESTORED_PROCESSING_OUTBOX" = "0"

RESTORED_WORKFLOW_AFTER_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM workflow_instances WHERE tenant_id='${TENANT_A_ID}' AND id='${WORKFLOW_ID}';")"
RESTORED_WORKFLOW_STEPS_AFTER_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM workflow_steps WHERE tenant_id='${TENANT_A_ID}' AND workflow_instance_id='${WORKFLOW_ID}';")"
RESTORED_OUTBOX_AFTER_RUNTIME="$(restore_sql "SELECT state FROM domain_outbox WHERE tenant_id='${TENANT_A_ID}' AND id='${OUTBOX_ID}';")"
RESTORED_INBOX_AFTER_RUNTIME="$(restore_sql "SELECT state FROM domain_inbox WHERE tenant_id='${TENANT_A_ID}' AND id='${INBOX_ID}';")"
RESTORED_OPERATION_AFTER_RUNTIME="$(restore_sql "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='${TENANT_A_ID}' AND id='${OPERATION_ID}';")"
RESTORED_CIRCUIT_AFTER_RUNTIME="$(restore_sql "SELECT state FROM integration_circuit_state WHERE tenant_id='${TENANT_A_ID}' AND binding_id='${BINDING_ID}';")"
RESTORED_PRE_MARKER_AFTER_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_id='${PRE_MARKER}' AND marker_kind='pre_backup';")"
RESTORED_POST_MARKER_AFTER_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM p10_recovery_markers WHERE marker_kind='post_backup';")"
RESTORE_IDENTITY_COUNT_AFTER_RUNTIME="$(restore_sql "SELECT COUNT(*) FROM identities;")"
RESTORED_SQLITE_FILES="$(docker exec "$RESTORED_APP_CONTAINER" sh -lc "find /app -maxdepth 4 -type f \\( -name '*.db' -o -name '*.sqlite' -o -name '*.sqlite3' \\) -print")"

test "$RESTORED_WORKFLOW_AFTER_RUNTIME" = "1"
test "$RESTORED_WORKFLOW_STEPS_AFTER_RUNTIME" = "12"
test "$RESTORED_OUTBOX_AFTER_RUNTIME" = "delivered"
test "$RESTORED_INBOX_AFTER_RUNTIME" = "processed"
test "$RESTORED_OPERATION_AFTER_RUNTIME" = "ready|0"
test "$RESTORED_CIRCUIT_AFTER_RUNTIME" = "open"
test "$RESTORED_PRE_MARKER_AFTER_RUNTIME" = "1"
test "$RESTORED_POST_MARKER_AFTER_RUNTIME" = "0"
test "$RESTORE_IDENTITY_COUNT_AFTER_RUNTIME" = "$RESTORE_IDENTITY_COUNT_BEFORE_RUNTIME"
test -z "$RESTORED_SQLITE_FILES"

"${compose[@]}" stop -t 20 nginx app
read -r RESTORED_APP_EXIT RESTORED_APP_OOM < <(docker inspect -f '{{.State.ExitCode}} {{.State.OOMKilled}}' "$RESTORED_APP_CONTAINER")
test "$RESTORED_APP_EXIT" = "0"
test "$RESTORED_APP_OOM" = "false"
docker logs "$RESTORED_APP_CONTAINER" > "$EVIDENCE_DIR/restored-app.log" 2>&1
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/restored-app.log" >/dev/null
! grep -F 'SQLite local database path' "$EVIDENCE_DIR/restored-app.log" >/dev/null

cat > "$EVIDENCE_DIR/restored-runtime-summary.txt" <<EOF
source_sha=$TALOS_QUALIFIED_SOURCE_SHA
source_tree=$TALOS_QUALIFIED_SOURCE_TREE_SHA
transport_sha=$(git rev-parse HEAD)
repository_migration_head=$migration_head
repository_migration_manifest_sha256=$migration_manifest_sha256
source_postgres_container=$DB_CONTAINER
source_postgres_system_identifier=$PG_SYSTEM_ID
restore_postgres_container=$RESTORE_CONTAINER_ID
restore_postgres_system_identifier=$RESTORE_SYSTEM_ID
source_db_backend_attached=$SOURCE_DB_BACKEND_ATTACHED
restore_db_backend_attached=$RESTORE_DB_BACKEND_ATTACHED
restored_app_container=$RESTORED_APP_CONTAINER
restored_app_replaced=true
restored_readiness=postgres_ready
restored_platform_login=$restored_platform_login_status
restored_tenant_login=200
source_identity_id=$SOURCE_IDENTITY_ID
restored_identity_id=$RESTORED_IDENTITY_ID
restored_identity_continuity=true
restored_identity_count_before_runtime=$RESTORE_IDENTITY_COUNT_BEFORE_RUNTIME
restored_identity_count_after_runtime=$RESTORE_IDENTITY_COUNT_AFTER_RUNTIME
restored_existing_tenant_a_read=$restored_existing_a_status
restored_existing_tenant_b_read=$restored_existing_b_status
restored_runtime_write=$runtime_write_status
restored_runtime_tenant_a_read=$restored_new_a_status
restored_runtime_tenant_b_read=$restored_new_b_status
restored_runtime_device_a_rows=$RESTORED_NEW_DEVICE_A_COUNT
restored_runtime_device_b_rows=$RESTORED_NEW_DEVICE_B_COUNT
restored_machine_admission_audit=$RESTORED_MACHINE_AUDIT_NEW
restored_command_success_audit=$RESTORED_COMMAND_AUDIT_NEW
restored_cross_tenant_audit=$RESTORED_CROSS_AUDIT_NEW
source_runtime_device_rows=$SOURCE_NEW_DEVICE_COUNT
source_runtime_correlation_rows=$SOURCE_RUNTIME_CORRELATION_COUNT
workflow_instances_after_runtime=$RESTORED_WORKFLOW_AFTER_RUNTIME
workflow_steps_after_runtime=$RESTORED_WORKFLOW_STEPS_AFTER_RUNTIME
workflow_running_after_settle=$RESTORED_RUNNING_WORKFLOW
outbox_state_after_runtime=$RESTORED_OUTBOX_AFTER_RUNTIME
outbox_processing_after_settle=$RESTORED_PROCESSING_OUTBOX
inbox_state_after_runtime=$RESTORED_INBOX_AFTER_RUNTIME
integration_operation_after_runtime=$RESTORED_OPERATION_AFTER_RUNTIME
integration_circuit_after_runtime=$RESTORED_CIRCUIT_AFTER_RUNTIME
pre_backup_marker_after_runtime=$RESTORED_PRE_MARKER_AFTER_RUNTIME
post_backup_marker_after_runtime=$RESTORED_POST_MARKER_AFTER_RUNTIME
sqlite_production_files=0
restored_app_exit=$RESTORED_APP_EXIT
restored_app_oom=$RESTORED_APP_OOM
EOF

cat > "$EVIDENCE_DIR/restore-summary.txt" <<EOF
source_sha=$TALOS_QUALIFIED_SOURCE_SHA
source_tree=$TALOS_QUALIFIED_SOURCE_TREE_SHA
transport_sha=$(git rev-parse HEAD)
repository_migration_head=$migration_head
repository_migration_manifest_sha256=$migration_manifest_sha256
source_postgres_container=$DB_CONTAINER
source_postgres_system_identifier=$PG_SYSTEM_ID
restore_postgres_container=$RESTORE_CONTAINER_ID
restore_postgres_system_identifier=$RESTORE_SYSTEM_ID
authority_container_distinct=true
authority_system_identifier_distinct=true
restore_public_tables_before=$RESTORE_PUBLIC_TABLES_BEFORE
restore_schema_migrations_absent_before=$RESTORE_SCHEMA_MIGRATIONS_ABSENT_BEFORE
restore_duration_ms=$restore_duration_ms
migration_count_before_probe=$RESTORE_MIGRATION_COUNT_BEFORE
migration_latest_before_probe=$RESTORE_LATEST_MIGRATION_BEFORE
migration_count_after_probe=$RESTORE_MIGRATION_COUNT_AFTER
migration_latest_after_probe=$RESTORE_LATEST_MIGRATION_AFTER
migration_idempotent=true
tenant_a_present=$RESTORE_TENANT_A_COUNT
tenant_b_present=$RESTORE_TENANT_B_COUNT
tenant_a_device_rows=$RESTORE_DEVICE_A_COUNT
tenant_b_same_device_rows=$RESTORE_DEVICE_B_COUNT
machine_admission_audit_rows=$RESTORE_MACHINE_AUDIT_COUNT
command_success_audit_rows=$RESTORE_COMMAND_AUDIT_COUNT
cross_tenant_audit_rows=$RESTORE_CROSS_TENANT_AUDIT_COUNT
workflow_instances=$RESTORE_WORKFLOW_COUNT
workflow_steps=$RESTORE_WORKFLOW_STEP_COUNT
domain_outbox_delivered=$RESTORE_OUTBOX_COUNT
domain_inbox_processed=$RESTORE_INBOX_COUNT
integration_operation=$RESTORE_OPERATION_STATE
integration_circuit=$RESTORE_CIRCUIT_STATE
pre_backup_marker=$RESTORE_PRE_MARKER_COUNT
post_backup_marker=$RESTORE_POST_MARKER_COUNT
source_unchanged_after_restore=true
plaintext_retained=false
EOF

"${compose[@]}" logs --no-color > "$EVIDENCE_DIR/compose.log" 2>&1 || true
docker logs "$RESTORE_CONTAINER" > "$EVIDENCE_DIR/restore-postgres.log" 2>&1 || true

backup_key_value="$(cat "$BACKUP_KEY_FILE")"
for secret in "$DB_PASSWORD" "$RESTORE_PASSWORD" "$P9_SP03_BOOTSTRAP_PASSWORD" "$MACHINE_SECRET" "$backup_key_value"; do
  if grep -R -F -l --binary-files=without-match "$secret" "$EVIDENCE_DIR" >/dev/null 2>&1; then
    echo "runtime secret leaked into retained SP04 evidence" >&2
    exit 1
  fi
done
test -z "$(find "$EVIDENCE_DIR" -type f \( -name '*.dump' -o -name 'pgpass' -o -name 'restore-pgpass' -o -name 'backup-key' -o -name '*.key' -o -name '*.cookies' \) -print -quit)"

rm -f "$BACKUP_KEY_FILE" "$PGPASS_FILE" "$RESTORE_PGPASS_FILE"
test ! -e "$BACKUP_KEY_FILE"
test ! -e "$PGPASS_FILE"
test ! -e "$RESTORE_PGPASS_FILE"

qualification_status="pass"
echo "P10_SP04_RESTORED_RUNTIME source_sha=$TALOS_QUALIFIED_SOURCE_SHA
source_tree=$TALOS_QUALIFIED_SOURCE_TREE_SHA
transport_sha=$(git rev-parse HEAD)
repository_migration_head=$migration_head
repository_migration_manifest_sha256=$migration_manifest_sha256 restore=fresh_pg18 app=exact_candidate readiness=pass auth=pass read_write=pass audit=canonical source_correlation_absent=pass recovery_state=governed sqlite_absent=pass secrets_retained=false"
echo "P10_RESTORED_RUNTIME_PASS"
