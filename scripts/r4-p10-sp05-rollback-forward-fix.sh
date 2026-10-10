#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p10-sp05"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
STATUS_FILE="$EVIDENCE_DIR/qualification-status.txt"
printf 'status=started\n' > "$STATUS_FILE"
qualification_status=fail

RUNTIME_BASE="$(printenv RUNNER_TEMP 2>/dev/null || true)"
test -n "$RUNTIME_BASE" || RUNTIME_BASE=/tmp
RUNTIME_DIR="$(mktemp -d "$RUNTIME_BASE/talos-p10-sp05.XXXXXX")"
chmod 700 "$RUNTIME_DIR"
mkdir -p "$RUNTIME_DIR/tls"
chmod 700 "$RUNTIME_DIR/tls"

RUN_ID="$(printenv GITHUB_RUN_ID 2>/dev/null || true)"
test -n "$RUN_ID" || RUN_ID=local
SAFE_ID="$(printf '%s' "$RUN_ID" | tr -cd 'A-Za-z0-9_.-')"
test -n "$SAFE_ID"
PROJECT="talos-p10-sp05-$(printf '%s' "$SAFE_ID" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

P9_ROLLBACK_SOURCE_SHA=2f414df89769ceedce482c4c25a24f02a24344b5
P9_ROLLBACK_MAX_MIGRATION=083_r4_reservation_rule_sequence_invariant
P9_IMAGE="talos-p10-sp05-p9:$SAFE_ID"
P9_BUILD_CONTEXT="$RUNTIME_DIR/p9-build-projection"
P9_DB_MOD_FIXTURE="$ROOT/.talos/ci/fixtures/p10-sp05/p9-db-mod.rs"
P9_INTEGRATION_MOD_FIXTURE="$ROOT/.talos/ci/fixtures/p10-sp05/p9-integration-mod.rs"
ROLLBACK_APP="talos-p10-sp05-rollback-app-$SAFE_ID"

export DB_PASSWORD="$(openssl rand -hex 18)"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:$DB_PASSWORD@db:5432/talos"
export P9_SP03_BOOTSTRAP_USERNAME="p10sp05-$SAFE_ID"
export P9_SP03_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"
export CORS_ALLOWED_ORIGIN="https://p10-sp05.invalid"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER=p10sp05-admin
export GRAFANA_PASSWORD="$(openssl rand -hex 18)"
export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"
export TALOS_HTTP_PORT=18080
export TALOS_HTTPS_PORT=18443
export TALOS_PROMETHEUS_PORT=19090
export TALOS_GRAFANA_PORT=13000

openssl rand -hex 32 > "$METRICS_SCRAPE_TOKEN_FILE"
chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"

PLATFORM_HOST=p10-sp05.invalid
PLATFORM_ORIGIN="https://$PLATFORM_HOST"
PLATFORM_CONNECT="$PLATFORM_HOST:443:127.0.0.1:$TALOS_HTTPS_PORT"
TENANT_A_SLUG="p10sp05-a-$SAFE_ID"
TENANT_B_SLUG="p10sp05-b-$SAFE_ID"
TENANT_A_HOST="$TENANT_A_SLUG.talos.invalid"
TENANT_B_HOST="$TENANT_B_SLUG.talos.invalid"
TENANT_A_ORIGIN="https://$TENANT_A_HOST"
TENANT_B_ORIGIN="https://$TENANT_B_HOST"
TENANT_A_CONNECT="$TENANT_A_HOST:443:127.0.0.1:$TALOS_HTTPS_PORT"
TENANT_B_CONNECT="$TENANT_B_HOST:443:127.0.0.1:$TALOS_HTTPS_PORT"

compose() {
  docker compose -p "$PROJECT"     -f deploy/compose.production.yml     -f deploy/compose.p9-sp03.yml     -f deploy/compose.p10-sp05.yml     "$@"
}

scan_evidence() {
  local leak=0
  local secret
  for secret in "$DB_PASSWORD" "$P9_SP03_BOOTSTRAP_PASSWORD" "$MACHINE_SECRET"; do
    if test -n "$secret" && grep -R -F -l --binary-files=without-match "$secret" "$EVIDENCE_DIR" >/dev/null 2>&1; then
      echo "runtime secret leaked into retained SP05 evidence" >&2
      leak=1
    fi
  done
  if test -n "$(find "$EVIDENCE_DIR" -type f \( -name '*.key' -o -name '*.cookies' -o -name 'pgpass' -o -name 'backup-key' -o -name '*.dump' \) -print -quit)"; then
    echo "forbidden runtime material retained in SP05 evidence" >&2
    leak=1
  fi
  test "$leak" = 0
}

cleanup() {
  exit_code=$?
  set +e
  set +u
  compose ps --all > "$EVIDENCE_DIR/compose-ps-final.txt" 2>&1 || true
  compose logs --no-color > "$EVIDENCE_DIR/compose.log" 2>&1 || true
  if docker inspect "$ROLLBACK_APP" >/dev/null 2>&1; then
    docker logs "$ROLLBACK_APP" > "$EVIDENCE_DIR/rollback-app-final.log" 2>&1 || true
    docker rm -f "$ROLLBACK_APP" >/dev/null 2>&1 || true
  fi
  compose down -v --remove-orphans > "$EVIDENCE_DIR/compose-down.log" 2>&1 || true
  docker image rm "$P9_IMAGE" >/dev/null 2>&1 || true
  if ! scan_evidence; then
    exit_code=1
    qualification_status=fail
  fi
  if test "$qualification_status" = pass && test "$exit_code" -eq 0; then
    printf 'status=pass\nexit_code=0\nevidence_scan=pass\n' > "$STATUS_FILE"
  else
    printf 'status=fail\nexit_code=%s\n' "$exit_code" > "$STATUS_FILE"
  fi
  rm -rf "$RUNTIME_DIR"
  trap - EXIT
  exit "$exit_code"
}
trap cleanup EXIT

for command in docker curl openssl python3 node cargo git grep sha256sum; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required qualification command unavailable: $command" >&2
    exit 1
  }
done

openssl req -x509 -newkey rsa:2048 -sha256 -days 1 -nodes   -keyout "$TLS_KEY_FILE"   -out "$TLS_CERT_FILE"   -subj "/CN=localhost"   -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"   >/dev/null 2>&1

: "${TALOS_QUALIFIED_SOURCE_SHA:?TALOS_QUALIFIED_SOURCE_SHA is required}"
: "${TALOS_QUALIFIED_SOURCE_TREE_SHA:?TALOS_QUALIFIED_SOURCE_TREE_SHA is required}"
[[ "$TALOS_QUALIFIED_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$TALOS_QUALIFIED_SOURCE_TREE_SHA" =~ ^[0-9a-f]{40}$ ]]
git rev-parse HEAD > "$EVIDENCE_DIR/transport-sha.txt"
printf '%s\n' "$TALOS_QUALIFIED_SOURCE_SHA" > "$EVIDENCE_DIR/source-sha.txt"
printf '%s\n' "$TALOS_QUALIFIED_SOURCE_TREE_SHA" > "$EVIDENCE_DIR/source-tree-sha.txt"

mapfile -t source_migration_files < <(find backend/src/db/migrations/postgres -maxdepth 1 -type f -name '*.sql' -print | LC_ALL=C sort)
test "${#source_migration_files[@]}" -gt 1
source_migration_head="$(basename "${source_migration_files[${#source_migration_files[@]}-1]}")"
source_migration_registry_head="${source_migration_head%.sql}"
source_migration_manifest_sha256="$(
  for file in "${source_migration_files[@]}"; do
    printf '%s  %s\n' "$(sha256sum "$file" | awk '{print $1}')" "$file"
  done | sha256sum | awk '{print $1}'
)"
[[ "$source_migration_manifest_sha256" =~ ^[0-9a-f]{64}$ ]]
printf 'head=%s\nregistry_head=%s\nsha256=%s\n' "$source_migration_head" "$source_migration_registry_head" "$source_migration_manifest_sha256" > "$EVIDENCE_DIR/source-migration-manifest.txt"

docker version > "$EVIDENCE_DIR/docker-version.txt"
docker compose version > "$EVIDENCE_DIR/docker-compose-version.txt"
compose config >/dev/null
compose config --images > "$EVIDENCE_DIR/compose-images.txt"
compose down -v --remove-orphans >/dev/null 2>&1 || true
compose pull db > "$EVIDENCE_DIR/pull.log"
compose build --pull app nginx > "$EVIDENCE_DIR/current-build.log"
compose up -d --no-build db app nginx

wait_ready() {
  local output="$1"
  for _ in $(seq 1 90); do
    if curl --silent --show-error --fail --insecure --noproxy '*'       --connect-to "$PLATFORM_CONNECT"       "$PLATFORM_ORIGIN/ready" -o "$output"; then
      grep -F '"status":"ready"' "$output" >/dev/null &&
      grep -F '"database":"postgres"' "$output" >/dev/null &&
      return 0
    fi
    sleep 2
  done
  return 1
}
wait_ready "$EVIDENCE_DIR/current-ready.json"

PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"
platform_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$PLATFORM_CONNECT"   --cookie-jar "$PLATFORM_COOKIE"   --output "$RUNTIME_DIR/platform-login.json"   --write-out '%{http_code}'   -H "Origin: $PLATFORM_ORIGIN"   -H 'Content-Type: application/json'   --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")"   "$PLATFORM_ORIGIN/auth/platform/login")"
test "$platform_status" = 200

SOURCE_IDENTITY_ID="$(python3 - "$RUNTIME_DIR/platform-login.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    d=json.load(f)
assert d.get("ok") is True
print(d["user"]["id"])
PY
)"
test -n "$SOURCE_IDENTITY_ID"

create_tenant() {
  local slug="$1"
  local label="$2"
  local out="$3"
  local status
  status="$(curl --silent --show-error --insecure --noproxy '*'     --connect-to "$PLATFORM_CONNECT"     --cookie "$PLATFORM_COOKIE"     --output "$out"     --write-out '%{http_code}'     -H 'X-Talos-Authority: platform'     -H "Origin: $PLATFORM_ORIGIN"     -H 'Content-Type: application/json'     --data "$(printf '{"name":"%s","slug":"%s"}' "$label" "$slug")"     "$PLATFORM_ORIGIN/api/tenants")"
  test "$status" = 200
  python3 - "$out" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    d=json.load(f)
assert d["status"] == "active"
print(d["id"])
PY
}

TENANT_A_ID="$(create_tenant "$TENANT_A_SLUG" "P10 SP05 Tenant A" "$RUNTIME_DIR/tenant-a.json")"
TENANT_B_ID="$(create_tenant "$TENANT_B_SLUG" "P10 SP05 Tenant B" "$RUNTIME_DIR/tenant-b.json")"
test "$TENANT_A_ID" != "$TENANT_B_ID"

login_tenant() {
  local origin="$1"
  local connect="$2"
  local cookie="$3"
  local out="$4"
  local status
  status="$(curl --silent --show-error --insecure --noproxy '*'     --connect-to "$connect"     --cookie-jar "$cookie"     --output "$out"     --write-out '%{http_code}'     -H "Origin: $origin"     -H 'Content-Type: application/json'     --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP03_BOOTSTRAP_USERNAME" "$P9_SP03_BOOTSTRAP_PASSWORD")"     "$origin/auth/login")"
  test "$status" = 200
}

TENANT_A_COOKIE="$RUNTIME_DIR/tenant-a.cookies"
TENANT_B_COOKIE="$RUNTIME_DIR/tenant-b.cookies"
login_tenant "$TENANT_A_ORIGIN" "$TENANT_A_CONNECT" "$TENANT_A_COOKIE" "$RUNTIME_DIR/login-a.json"
login_tenant "$TENANT_B_ORIGIN" "$TENANT_B_CONNECT" "$TENANT_B_COOKIE" "$RUNTIME_DIR/login-b.json"

MODEL_BODY="$(printf '{"name":"P10 SP05 Model %s","category":"qualification","prefix":"P10R","enabled":true}' "$SAFE_ID")"
model_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/model.json" --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN" -H 'Content-Type: application/json'   --data "$MODEL_BODY" "$TENANT_A_ORIGIN/api/device-models")"
test "$model_status" = 200
MODEL_ID="$(python3 - "$RUNTIME_DIR/model.json" <<'PY'
import json,sys
with open(sys.argv[1],encoding='utf-8') as f: d=json.load(f)
v=d.get("id") or d.get("model",{}).get("id")
assert isinstance(v,str) and v
print(v)
PY
)"

machine_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/machine.json" --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN" -H 'Content-Type: application/json'   --data '{"name":"P10 SP05 rollback writer","scopes":[{"module":"device","command":"create_device"}],"rate_limit_rpm":60,"role":"admin"}'   "$TENANT_A_ORIGIN/api/machine-clients")"
test "$machine_status" = 200
readarray -t MACHINE < <(python3 - "$RUNTIME_DIR/machine.json" <<'PY'
import json,sys
with open(sys.argv[1],encoding='utf-8') as f: d=json.load(f)
print(d["client_id"]); print(d["secret"])
PY
)
MACHINE_CLIENT_ID="${MACHINE[0]}"
MACHINE_SECRET="${MACHINE[1]}"
test -n "$MACHINE_CLIENT_ID"
test -n "$MACHINE_SECRET"

BASE_SERIAL="P10SP05BASE$(printf '%s' "$SAFE_ID" | tr -cd 'A-Za-z0-9')"
BASE_BODY="$(printf '{"module":"device","command":"create_device","payload":{"serialNo":"%s","modelId":"%s","warehouseId":"","status":"available"}}' "$BASE_SERIAL" "$MODEL_ID")"
base_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --output "$RUNTIME_DIR/base-write.json" --write-out '%{http_code}'   -H "Authorization: Bearer $MACHINE_SECRET" -H 'Content-Type: application/json'   --data "$BASE_BODY" "$TENANT_A_ORIGIN/api/machine/v1/tenants/$TENANT_A_ID/execute")"
test "$base_status" = 200

MANIFEST='{"providerId":"fixture-p10-rollback","version":"1.0.0","capabilities":["qualification.rollback"],"configSchema":[{"name":"endpoint","required":true,"valueType":"https_origin"}],"secretSchema":[],"apiVersions":{"qualification.rollback":"v1"},"webhookTypes":[],"simulationCapabilities":[],"readiness":"fixture","compatibility":{"qualification.rollback":"backward_compatible"}}'
manifest_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$PLATFORM_CONNECT" --cookie "$PLATFORM_COOKIE"   --output "$RUNTIME_DIR/manifest.json" --write-out '%{http_code}'   -H 'X-Talos-Authority: platform' -H "Origin: $PLATFORM_ORIGIN" -H 'Content-Type: application/json'   --data "$MANIFEST" "$PLATFORM_ORIGIN/api/integrations/manifests")"
test "$manifest_status" = 200

INSTANCE_ID="p10-sp05-instance-$SAFE_ID"
BINDING_ID="p10-sp05-binding-$SAFE_ID"
INSTANCE_BODY="$(printf '{"id":"%s","providerId":"fixture-p10-rollback","manifestVersion":"1.0.0","configRevision":"revision-1","config":{"endpoint":"https://fixture.invalid"},"secretRefs":{},"lifecycle":"active","health":"ready","readiness":"fixture"}' "$INSTANCE_ID")"
instance_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/instance.json" --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN" -H 'Content-Type: application/json'   --data "$INSTANCE_BODY" "$TENANT_A_ORIGIN/api/integrations/instances")"
test "$instance_status" = 200

binding_body() {
  printf '{"id":"%s","providerInstanceId":"%s","capability":"qualification.rollback","configRevision":"revision-1","enabled":%s}' "$BINDING_ID" "$INSTANCE_ID" "$1"
}
binding_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/binding-enabled.json" --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN" -H 'Content-Type: application/json'   --data "$(binding_body true)" "$TENANT_A_ORIGIN/api/integrations/bindings")"
test "$binding_status" = 200

DB_CONTAINER="$(compose ps -q db)"
CURRENT_APP_CONTAINER="$(compose ps -q app)"
NGINX_CONTAINER="$(compose ps -q nginx)"
test -n "$DB_CONTAINER"
test -n "$CURRENT_APP_CONTAINER"
test -n "$NGINX_CONTAINER"
DB_NETWORK="$(docker inspect -f '{{range $name, $_ := .NetworkSettings.Networks}}{{println $name}}{{end}}' "$DB_CONTAINER" | head -n1)"
test -n "$DB_NETWORK"

OP_BODY="$(printf '{"capability":"qualification.rollback","operationType":"qualification","idempotencyKey":"p10-sp05-%s","requestHash":"p10-sp05-request-%s"}' "$SAFE_ID" "$SAFE_ID")"
op_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/operation.json" --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN" -H 'Content-Type: application/json'   --data "$OP_BODY" "$TENANT_A_ORIGIN/api/integrations/operations")"
test "$op_status" = 200
OPERATION_ID="$(python3 - "$RUNTIME_DIR/operation.json" <<'PY'
import json,sys
with open(sys.argv[1],encoding='utf-8') as f: d=json.load(f)
assert d.get("state") == "ready"
print(d["externalOperationId"])
PY
)"

# Freeze this synthetic operation out of the production scheduler while the
# rollback image is built. The production claim authority already treats a
# future next_retry_at as not due; the later controlled crash cut clears it.
docker exec "$DB_CONTAINER" psql -U talos -d talos -v tenant_id="$TENANT_A_ID" -v operation_id="$OPERATION_ID" <<'SQL'
\set ON_ERROR_STOP on
UPDATE external_operations
SET next_retry_at='9999-12-31T23:59:59Z',
    updated_at=CURRENT_TIMESTAMP::text
WHERE tenant_id=:'tenant_id'
  AND id=:'operation_id'
  AND state='ready'
  AND attempt_count=0;
SQL
DEFERRED_OPERATION="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count || '|' || COALESCE(next_retry_at,'') FROM external_operations WHERE tenant_id='$TENANT_A_ID' AND id='$OPERATION_ID';")"
PRE_CUT_BINDING_ENABLED="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT enabled FROM provider_bindings WHERE tenant_id='$TENANT_A_ID' AND id='$BINDING_ID';")"
printf 'P10_SP05_DEFERRED_OPERATION operation=%s binding_enabled=%s\n' "$DEFERRED_OPERATION" "$PRE_CUT_BINDING_ENABLED"
test "$DEFERRED_OPERATION" = 'ready|0|9999-12-31T23:59:59Z'
test "$PRE_CUT_BINDING_ENABLED" = t

LATEST_MIGRATION="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1;")"
MIGRATION_COUNT="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM schema_migrations;")"
test "$LATEST_MIGRATION" = "$source_migration_registry_head"

test "$(git hash-object Dockerfile)" = "be8c6aaa7470d7865238e541a94400b13a248648"
test "$(git hash-object backend/Cargo.toml)" = "cfaa8246e34ce909ccb4c64aedc2947ae83d3000"
test "$(git hash-object backend/Cargo.lock)" = "778df55af576a95ffd93b99b0b2f4fb4d1fa45b2"
test "$(git hash-object backend/src/main.rs)" = "733b6738c24dd4902522c7875198da3ec358e76a"
test "$(git hash-object "$P9_DB_MOD_FIXTURE")" = "a1c2eb7ef070c9d7aa7ebc3831d5282f789870de"
test "$(git hash-object "$P9_INTEGRATION_MOD_FIXTURE")" = "db1e3ff2e7be8c0d920e70f70e72b149ac8f0e3b"

mkdir -p "$P9_BUILD_CONTEXT"
cp Dockerfile "$P9_BUILD_CONTEXT/Dockerfile"
cp -a backend "$P9_BUILD_CONTEXT/backend"
cp "$P9_DB_MOD_FIXTURE" "$P9_BUILD_CONTEXT/backend/src/db/mod.rs"
cp "$P9_INTEGRATION_MOD_FIXTURE" "$P9_BUILD_CONTEXT/backend/src/integration/mod.rs"
rm -f "$P9_BUILD_CONTEXT/backend/src/db/p10_restore_qualification.rs"
rm -f "$P9_BUILD_CONTEXT/backend/src/integration/p10_recovery_qualification.rs"

rollback_candidate_max="$(find "$P9_BUILD_CONTEXT/backend/src/db/migrations/postgres" -maxdepth 1 -type f -name '*.sql' -print | LC_ALL=C sort | tail -n1 | xargs -r basename)"
rollback_candidate_max="${rollback_candidate_max%.sql}"
test -n "$rollback_candidate_max"
test "$rollback_candidate_max" = "$P9_ROLLBACK_MAX_MIGRATION"

node scripts/r4-p10-schema-compatibility.mjs \
  --candidate-ref "$P9_ROLLBACK_SOURCE_SHA" \
  --candidate-max "$rollback_candidate_max" \
  --restored-latest "$LATEST_MIGRATION" \
  > "$EVIDENCE_DIR/compatible-schema-gate.log"
grep -F ROLLBACK_SCHEMA_COMPATIBLE "$EVIDENCE_DIR/compatible-schema-gate.log" >/dev/null

docker build -t "$P9_IMAGE" "$P9_BUILD_CONTEXT" > "$EVIDENCE_DIR/p9-image-build.log"

for _ in $(seq 1 30); do
  DRAIN_DISPATCHING="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operations WHERE state='dispatching';")"
  DRAIN_WORKFLOW="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM workflow_steps WHERE state='running';")"
  DRAIN_OUTBOX="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM domain_outbox WHERE state='processing';")"
  if [[ "$DRAIN_DISPATCHING" = 0 && "$DRAIN_WORKFLOW" = 0 && "$DRAIN_OUTBOX" = 0 ]]; then
    break
  fi
  sleep 1
done
printf 'P10_SP05_DRAIN dispatching=%s workflow_running=%s outbox_processing=%s\n' "$DRAIN_DISPATCHING" "$DRAIN_WORKFLOW" "$DRAIN_OUTBOX"
test "$DRAIN_DISPATCHING" = 0
test "$DRAIN_WORKFLOW" = 0
test "$DRAIN_OUTBOX" = 0

ATTEMPT_ID="p10-sp05-attempt-$SAFE_ID"
EVENT_ID="p10-sp05-claimed-$SAFE_ID"
echo "P10_SP05_STAGE crash_cut_begin"
docker exec "$DB_CONTAINER" psql -U talos -d talos   -v tenant_id="$TENANT_A_ID" -v operation_id="$OPERATION_ID"   -v attempt_id="$ATTEMPT_ID" -v event_id="$EVENT_ID" <<'SQL'
\set ON_ERROR_STOP on
BEGIN;
UPDATE external_operations
SET state='dispatching',attempt_count=1,next_retry_at=NULL,updated_at=CURRENT_TIMESTAMP::text
WHERE tenant_id=:'tenant_id' AND id=:'operation_id' AND state='ready';
INSERT INTO external_operation_attempts
  (id,tenant_id,operation_id,attempt_number,state,classification,request_hash,provider_result_ref,started_at,completed_at)
SELECT
  :'attempt_id',tenant_id,id,1,'dispatching',NULL,request_hash,NULL,CURRENT_TIMESTAMP::text,NULL
FROM external_operations
WHERE tenant_id=:'tenant_id' AND id=:'operation_id' AND state='dispatching';
INSERT INTO external_operation_runtime_events
  (id,tenant_id,operation_id,attempt_id,event_type,classification,occurred_at)
VALUES
  (:'event_id',:'tenant_id',:'operation_id',:'attempt_id','claimed','p10_sp05_controlled_crash_cut',CURRENT_TIMESTAMP::text);
COMMIT;
SQL
CUT_STATE="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='$TENANT_A_ID' AND id='$OPERATION_ID';")"
CUT_ATTEMPTS="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operation_attempts WHERE tenant_id='$TENANT_A_ID' AND operation_id='$OPERATION_ID' AND attempt_number=1 AND state='dispatching';")"
CUT_EVENTS="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operation_runtime_events WHERE tenant_id='$TENANT_A_ID' AND operation_id='$OPERATION_ID' AND event_type='claimed' AND classification='p10_sp05_controlled_crash_cut';")"
printf 'P10_SP05_CRASH_CUT state=%s attempts=%s claimed_events=%s\n' "$CUT_STATE" "$CUT_ATTEMPTS" "$CUT_EVENTS"
test "$CUT_STATE" = 'dispatching|1'
test "$CUT_ATTEMPTS" = 1
test "$CUT_EVENTS" = 1

echo "P10_SP05_STAGE binding_freeze_begin"
disable_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$TENANT_A_COOKIE"   --output "$RUNTIME_DIR/binding-disabled.json" --write-out '%{http_code}'   -H "Origin: $TENANT_A_ORIGIN" -H 'Content-Type: application/json'   --data "$(binding_body false)" "$TENANT_A_ORIGIN/api/integrations/bindings")"
BINDING_ENABLED="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT enabled FROM provider_bindings WHERE tenant_id='$TENANT_A_ID' AND id='$BINDING_ID';")"
BINDING_DISABLE_HISTORY="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM provider_binding_history WHERE tenant_id='$TENANT_A_ID' AND binding_id='$BINDING_ID' AND action='disabled';")"
POST_FREEZE_OPERATION="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='$TENANT_A_ID' AND id='$OPERATION_ID';")"
printf 'P10_SP05_BINDING_FREEZE http=%s enabled=%s history=%s operation=%s\n' "$disable_status" "$BINDING_ENABLED" "$BINDING_DISABLE_HISTORY" "$POST_FREEZE_OPERATION"
test "$disable_status" = 200
test "$BINDING_ENABLED" = f
test "$BINDING_DISABLE_HISTORY" = 1
test "$POST_FREEZE_OPERATION" = 'dispatching|1'

echo "P10_SP05_STAGE app_stop_begin"
compose stop -t 20 nginx app
docker logs "$CURRENT_APP_CONTAINER" > "$EVIDENCE_DIR/current-before-rollback.log" 2>&1
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/current-before-rollback.log" >/dev/null
docker rm "$CURRENT_APP_CONTAINER" >/dev/null

docker run -d   --name "$ROLLBACK_APP"   --network "$DB_NETWORK"   --ip 172.29.0.20   --network-alias app   -e NODE_ENV=production   -e HOST=0.0.0.0   -e PORT=8080   -e DB_BACKEND=postgres   -e DATABASE_URL="$TALOS_PRODUCTION_DATABASE_URL"   -e PUBLIC_DIR=/app/public   -e PUBLIC_HTTPS=true   -e CORS_ALLOWED_ORIGIN="$CORS_ALLOWED_ORIGIN"   -e TRUSTED_PROXY_COUNT=1   -e TRUSTED_PROXY_CIDRS=172.29.0.0/24   -e METRICS_SCRAPE_TOKEN_FILE=/run/secrets/talos_metrics_scrape_token   -e METRICS_SCRAPE_PEER=172.29.0.40   -e AUTH_BOOTSTRAP_ON_START=true   -e AUTH_BOOTSTRAP_ADMIN_USERNAME="$P9_SP03_BOOTSTRAP_USERNAME"   -e AUTH_BOOTSTRAP_ADMIN_PASSWORD="$P9_SP03_BOOTSTRAP_PASSWORD"   -e RUST_LOG=info   -v "$METRICS_SCRAPE_TOKEN_FILE:/run/secrets/talos_metrics_scrape_token:ro"   "$P9_IMAGE" > "$EVIDENCE_DIR/rollback-container-id.txt"

docker start "$NGINX_CONTAINER" >/dev/null
wait_ready "$EVIDENCE_DIR/rollback-ready.json"
docker logs "$ROLLBACK_APP" > "$EVIDENCE_DIR/rollback-app-startup.log" 2>&1
grep -F 'Database profile: Postgres18' "$EVIDENCE_DIR/rollback-app-startup.log" >/dev/null
grep -F 'No pending PG migrations' "$EVIDENCE_DIR/rollback-app-startup.log" >/dev/null
! grep -F 'SQLite local database path' "$EVIDENCE_DIR/rollback-app-startup.log" >/dev/null

for _ in $(seq 1 30); do
  RECOVERY_STATE="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count || '|' || COALESCE(classification,'') FROM external_operations WHERE tenant_id='$TENANT_A_ID' AND id='$OPERATION_ID';")"
  test "$RECOVERY_STATE" = 'unknown_outcome|1|worker_restarted_after_dispatch' && break
  sleep 1
done
test "$RECOVERY_STATE" = 'unknown_outcome|1|worker_restarted_after_dispatch'
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operation_attempts WHERE tenant_id='$TENANT_A_ID' AND operation_id='$OPERATION_ID';")" = 1
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operation_runtime_events WHERE tenant_id='$TENANT_A_ID' AND operation_id='$OPERATION_ID' AND event_type='recovered_after_restart' AND classification='worker_restarted_after_dispatch';")" = 1

ROLLBACK_COOKIE="$RUNTIME_DIR/rollback.cookies"
login_tenant "$TENANT_A_ORIGIN" "$TENANT_A_CONNECT" "$ROLLBACK_COOKIE" "$RUNTIME_DIR/rollback-login.json"
rollback_read_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$ROLLBACK_COOKIE"   --output "$RUNTIME_DIR/rollback-read.json" --write-out '%{http_code}'   "$TENANT_A_ORIGIN/devices/$BASE_SERIAL")"
test "$rollback_read_status" = 200

ROLLBACK_SERIAL="P10SP05ROLL$(printf '%s' "$SAFE_ID" | tr -cd 'A-Za-z0-9')"
ROLLBACK_BODY="$(printf '{"module":"device","command":"create_device","payload":{"serialNo":"%s","modelId":"%s","warehouseId":"","status":"available"}}' "$ROLLBACK_SERIAL" "$MODEL_ID")"
ROLLBACK_HEADERS="$RUNTIME_DIR/rollback-write.headers"
rollback_write_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --dump-header "$ROLLBACK_HEADERS"   --output "$RUNTIME_DIR/rollback-write.json" --write-out '%{http_code}'   -H "Authorization: Bearer $MACHINE_SECRET" -H 'Content-Type: application/json'   --data "$ROLLBACK_BODY" "$TENANT_A_ORIGIN/api/machine/v1/tenants/$TENANT_A_ID/execute")"
test "$rollback_write_status" = 200
ROLLBACK_CORRELATION="$(awk 'tolower($1)=="x-correlation-id:" {gsub("\r","",$2); print $2}' "$ROLLBACK_HEADERS" | tail -n1)"
test -n "$ROLLBACK_CORRELATION"
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM devices WHERE tenant_id='$TENANT_A_ID' AND serialNo='$ROLLBACK_SERIAL';")" = 1
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM audit_events WHERE tenant_id='$TENANT_A_ID' AND correlation_id='$ROLLBACK_CORRELATION' AND action='device.create_device' AND detail_json->>'result'='succeeded';")" = 1
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='$TENANT_A_ID' AND id='$OPERATION_ID';")" = 'unknown_outcome|1'

docker stop --time 20 "$NGINX_CONTAINER" >/dev/null
docker stop --time 20 "$ROLLBACK_APP" >/dev/null
docker logs "$ROLLBACK_APP" > "$EVIDENCE_DIR/rollback-app.log" 2>&1
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/rollback-app.log" >/dev/null
docker rm "$ROLLBACK_APP" >/dev/null

MIGRATION_COUNT_BEFORE_BLOCK="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM schema_migrations;")"
LATEST_BEFORE_BLOCK="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1;")"
set +e
incompatible_candidate_max="$(find "$P9_BUILD_CONTEXT/backend/src/db/migrations/postgres" -maxdepth 1 -type f -name '*.sql' -print | LC_ALL=C sort | tail -n2 | head -n1 | xargs -r basename)"
incompatible_candidate_max="${incompatible_candidate_max%.sql}"
test -n "$incompatible_candidate_max"
node scripts/r4-p10-schema-compatibility.mjs \
  --candidate-ref qualification-incompatible-previous-migration \
  --candidate-max "$incompatible_candidate_max" \
  --restored-latest "$LATEST_BEFORE_BLOCK" \
  > "$EVIDENCE_DIR/incompatible-schema-gate.log" 2>&1
BLOCK_STATUS=$?
set -e
test "$BLOCK_STATUS" = 42
grep -F ROLLBACK_SCHEMA_INCOMPATIBLE "$EVIDENCE_DIR/incompatible-schema-gate.log" >/dev/null
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM schema_migrations;")" = "$MIGRATION_COUNT_BEFORE_BLOCK"
test "$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1;")" = "$LATEST_BEFORE_BLOCK"
test ! -e "$RUNTIME_DIR/incompatible-app-started"

compose up -d --no-deps --force-recreate app nginx
FORWARD_APP="$(compose ps -q app)"
test -n "$FORWARD_APP"
wait_ready "$EVIDENCE_DIR/forward-fix-ready.json"
docker logs "$FORWARD_APP" > "$EVIDENCE_DIR/forward-fix-startup.log" 2>&1
grep -F 'Database profile: Postgres18' "$EVIDENCE_DIR/forward-fix-startup.log" >/dev/null
grep -F 'No pending PG migrations' "$EVIDENCE_DIR/forward-fix-startup.log" >/dev/null

FORWARD_COOKIE="$RUNTIME_DIR/forward.cookies"
login_tenant "$TENANT_A_ORIGIN" "$TENANT_A_CONNECT" "$FORWARD_COOKIE" "$RUNTIME_DIR/forward-login.json"
forward_read_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --cookie "$FORWARD_COOKIE"   --output "$RUNTIME_DIR/forward-read.json" --write-out '%{http_code}'   "$TENANT_A_ORIGIN/devices/$ROLLBACK_SERIAL")"
test "$forward_read_status" = 200

FORWARD_SERIAL="P10SP05FWD$(printf '%s' "$SAFE_ID" | tr -cd 'A-Za-z0-9')"
FORWARD_BODY="$(printf '{"module":"device","command":"create_device","payload":{"serialNo":"%s","modelId":"%s","warehouseId":"","status":"available"}}' "$FORWARD_SERIAL" "$MODEL_ID")"
forward_write_status="$(curl --silent --show-error --insecure --noproxy '*'   --connect-to "$TENANT_A_CONNECT" --output "$RUNTIME_DIR/forward-write.json" --write-out '%{http_code}'   -H "Authorization: Bearer $MACHINE_SECRET" -H 'Content-Type: application/json'   --data "$FORWARD_BODY" "$TENANT_A_ORIGIN/api/machine/v1/tenants/$TENANT_A_ID/execute")"
test "$forward_write_status" = 200

export TALOS_P10_RECOVERY_DATABASE_URL="postgresql://talos:$DB_PASSWORD@127.0.0.1:15432/talos"
export TALOS_P10_RECOVERY_TENANT_ID="$TENANT_A_ID"
export TALOS_P10_RECOVERY_OPERATION_ID="$OPERATION_ID"
cargo test   --manifest-path backend/Cargo.toml   --features postgres   --bin talos-backend   p10_unknown_outcome_requires_reconciliation_before_retry   --locked   -- --ignored --nocapture > "$EVIDENCE_DIR/reconciliation.log" 2>&1
unset TALOS_P10_RECOVERY_DATABASE_URL TALOS_P10_RECOVERY_TENANT_ID TALOS_P10_RECOVERY_OPERATION_ID
grep -F P10_EFFECT_RECONCILIATION "$EVIDENCE_DIR/reconciliation.log" >/dev/null

FINAL_OPERATION="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='$TENANT_A_ID' AND id='$OPERATION_ID';")"
FINAL_RECONCILIATION="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT outcome || '|' || COALESCE(resolved_by,'') FROM external_operation_reconciliations WHERE tenant_id='$TENANT_A_ID' AND operation_id='$OPERATION_ID' ORDER BY created_at DESC LIMIT 1;")"
FINAL_ATTEMPTS="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM external_operation_attempts WHERE tenant_id='$TENANT_A_ID' AND operation_id='$OPERATION_ID';")"
FINAL_BINDING="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT enabled FROM provider_bindings WHERE tenant_id='$TENANT_A_ID' AND id='$BINDING_ID';")"
FINAL_ROLLBACK_DEVICE="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM devices WHERE tenant_id='$TENANT_A_ID' AND serialNo='$ROLLBACK_SERIAL';")"
FINAL_FORWARD_DEVICE="$(docker exec "$DB_CONTAINER" psql -U talos -d talos -Atc "SELECT COUNT(*) FROM devices WHERE tenant_id='$TENANT_A_ID' AND serialNo='$FORWARD_SERIAL';")"
FINAL_SQLITE_FILES="$(docker exec "$FORWARD_APP" sh -lc "find /app -maxdepth 4 -type f \( -name '*.db' -o -name '*.sqlite' -o -name '*.sqlite3' \) -print")"

test "$FINAL_OPERATION" = 'resolved|1'
test "$FINAL_RECONCILIATION" = 'effect_confirmed|p10-sp05-recovery-operator'
test "$FINAL_ATTEMPTS" = 1
test "$FINAL_BINDING" = f
test "$FINAL_ROLLBACK_DEVICE" = 1
test "$FINAL_FORWARD_DEVICE" = 1
test -z "$FINAL_SQLITE_FILES"

compose stop -t 20 nginx app
docker logs "$FORWARD_APP" > "$EVIDENCE_DIR/forward-fix-app.log" 2>&1
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/forward-fix-app.log" >/dev/null

cat > "$EVIDENCE_DIR/rollback-forward-fix-summary.txt" <<EOF
source_sha=$TALOS_QUALIFIED_SOURCE_SHA
source_tree=$TALOS_QUALIFIED_SOURCE_TREE_SHA
transport_sha=$(git rev-parse HEAD)
source_migration_head=$source_migration_head
source_migration_manifest_sha256=$source_migration_manifest_sha256
compatible_rollback_candidate=$P9_ROLLBACK_SOURCE_SHA
rollback_candidate_max=$rollback_candidate_max
rollback_projection=runtime_build_projection
schema_migration_count=$MIGRATION_COUNT
schema_latest=$LATEST_MIGRATION
compatible_schema_gate=pass
rollback_app_ready=pass
rollback_read=$rollback_read_status
rollback_write=$rollback_write_status
incompatible_schema_gate=blocked
incompatible_candidate_process_started=false
schema_downgrade_attempted=false
forward_fix_ready=pass
forward_fix_read=$forward_read_status
forward_fix_write=$forward_write_status
drain_dispatching_before_cut=$DRAIN_DISPATCHING
drain_workflow_running=$DRAIN_WORKFLOW
drain_outbox_processing=$DRAIN_OUTBOX
binding_disabled=true
binding_disable_history=1
recovered_unknown_outcome=true
blind_retry=false
final_external_operation=$FINAL_OPERATION
reconciliation=$FINAL_RECONCILIATION
external_operation_attempts=$FINAL_ATTEMPTS
compensation=not_applicable_nonfinancial_fixture
sqlite_production_files=0
EOF

qualification_status=pass
echo "P10_SP05_ROLLBACK_FORWARD_FIX compatible_rollback=pass incompatible_rollback=blocked forward_fix=pass binding_freeze=pass unknown_outcome=no_blind_retry reconciliation=effect_confirmed attempts=1 sqlite_absent=pass"
echo "P10_ROLLBACK_FORWARD_FIX_PASS"
