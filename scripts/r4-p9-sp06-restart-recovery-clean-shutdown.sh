#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p9-sp06"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p9-sp06.XXXXXX")"
mkdir -p "$RUNTIME_DIR/tls"

run_id="${GITHUB_RUN_ID:-local}"
PROJECT="talos-p9-sp06-${run_id}"
PROJECT="$(printf '%s' "$PROJECT" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"

export DB_PASSWORD="$(openssl rand -hex 18)"
export TALOS_PRODUCTION_DATABASE_URL="postgresql://talos:${DB_PASSWORD}@db:5432/talos"
export P9_SP06_BOOTSTRAP_USERNAME="p9sp06-${run_id}"
export P9_SP06_BOOTSTRAP_PASSWORD="$(openssl rand -hex 24)"
export CORS_ALLOWED_ORIGIN="https://p9-sp06.invalid"
export TLS_CERT_FILE="$RUNTIME_DIR/tls/tls.crt"
export TLS_KEY_FILE="$RUNTIME_DIR/tls/tls.key"
export GRAFANA_USER="p9sp06-admin"
export GRAFANA_PASSWORD="$(openssl rand -hex 18)"
export METRICS_SCRAPE_TOKEN_FILE="$RUNTIME_DIR/metrics-scrape-token"
export TALOS_HTTP_PORT="18080"
export TALOS_HTTPS_PORT="18443"
export TALOS_PROMETHEUS_PORT="19090"
export TALOS_GRAFANA_PORT="13000"

test "$(stat -c '%a' "$RUNTIME_DIR")" = "700"
openssl rand -hex 32 > "$METRICS_SCRAPE_TOKEN_FILE"
chmod 0444 "$METRICS_SCRAPE_TOKEN_FILE"

PLATFORM_HOST="p9-sp06.invalid"
PLATFORM_ORIGIN="https://${PLATFORM_HOST}"
PLATFORM_CONNECT="${PLATFORM_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"
TENANT_SLUG="p9sp06-${run_id}"
TENANT_HOST="${TENANT_SLUG}.talos.invalid"
TENANT_ORIGIN="https://${TENANT_HOST}"
TENANT_CONNECT="${TENANT_HOST}:443:127.0.0.1:${TALOS_HTTPS_PORT}"

compose=(
  docker compose
  -p "$PROJECT"
  -f deploy/compose.production.yml
  -f deploy/compose.p9-sp06.yml
)

cleanup() {
  set +e
  "${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-final.txt" 2>&1
  "${compose[@]}" logs --no-color > "$EVIDENCE_DIR/compose.log" 2>&1
  "${compose[@]}" down -v --remove-orphans > "$EVIDENCE_DIR/compose-down-cleanup.log" 2>&1
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

wait_ready "$EVIDENCE_DIR/ready-before.json"

PLATFORM_COOKIE="$RUNTIME_DIR/platform.cookies"
platform_login_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie-jar "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/platform-login.json" \
  --write-out '%{http_code}' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP06_BOOTSTRAP_USERNAME" "$P9_SP06_BOOTSTRAP_PASSWORD")" \
  "$PLATFORM_ORIGIN/auth/platform/login")"
test "$platform_login_status" = "200"

tenant_create_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$PLATFORM_CONNECT" \
  --cookie "$PLATFORM_COOKIE" \
  --output "$RUNTIME_DIR/tenant-create.json" \
  --write-out '%{http_code}' \
  -H 'X-Talos-Authority: platform' \
  -H "Origin: $PLATFORM_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"name":"P9 SP06 Tenant","slug":"%s"}' "$TENANT_SLUG")" \
  "$PLATFORM_ORIGIN/api/tenants")"
test "$tenant_create_status" = "200"
TENANT_ID="$(python3 - "$RUNTIME_DIR/tenant-create.json" <<'PY'
import json,sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
value=data.get("id")
assert isinstance(value,str) and value
assert data.get("status") == "active"
print(value)
PY
)"

TENANT_COOKIE="$RUNTIME_DIR/tenant.cookies"
tenant_login_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie-jar "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/tenant-login.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$(printf '{"username":"%s","password":"%s"}' "$P9_SP06_BOOTSTRAP_USERNAME" "$P9_SP06_BOOTSTRAP_PASSWORD")" \
  "$TENANT_ORIGIN/auth/login")"
test "$tenant_login_status" = "200"

MANIFEST_BODY='{"providerId":"fixture-restart","version":"1.0.0","capabilities":["qualification.restart"],"configSchema":[{"name":"endpoint","required":true,"valueType":"https_origin"}],"secretSchema":[],"apiVersions":{"qualification.restart":"v1"},"webhookTypes":[],"simulationCapabilities":[],"readiness":"fixture","compatibility":{"qualification.restart":"backward_compatible"}}'
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

INSTANCE_BODY='{"id":"p9-sp06-instance","providerId":"fixture-restart","manifestVersion":"1.0.0","configRevision":"revision-1","config":{"endpoint":"https://fixture.invalid"},"secretRefs":{},"lifecycle":"active","health":"ready","readiness":"fixture"}'
instance_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/instance.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$INSTANCE_BODY" \
  "$TENANT_ORIGIN/api/integrations/instances")"
test "$instance_status" = "200"

BINDING_BODY='{"id":"p9-sp06-binding","providerInstanceId":"p9-sp06-instance","capability":"qualification.restart","configRevision":"revision-1","enabled":true}'
binding_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/binding.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$BINDING_BODY" \
  "$TENANT_ORIGIN/api/integrations/bindings")"
test "$binding_status" = "200"

# Prevent the periodic production worker from dispatching the qualification
# operation before the controlled crash cut-point is established. This uses
# the existing circuit authority, so a worker tick can only defer the operation
# in Ready state; no test-only worker switch or timing assumption is introduced.
"${compose[@]}" exec -T db psql -U talos -d talos \
  -v tenant_id="$TENANT_ID" <<'SQL'
\set ON_ERROR_STOP on
INSERT INTO integration_circuit_state
    (tenant_id,binding_id,state,failure_count,opened_until,updated_at)
VALUES
    (:'tenant_id','p9-sp06-binding','open',1,'9999-12-31T23:59:59Z',CURRENT_TIMESTAMP::text)
ON CONFLICT (tenant_id,binding_id) DO UPDATE
SET state='open',
    failure_count=GREATEST(integration_circuit_state.failure_count,1),
    opened_until='9999-12-31T23:59:59Z',
    updated_at=CURRENT_TIMESTAMP::text;
SQL

OPERATION_BODY="$(printf '{"capability":"qualification.restart","operationType":"qualification","idempotencyKey":"p9-sp06-%s","requestHash":"p9-sp06-request-%s"}' "$run_id" "$run_id")"
operation_status="$(curl --silent --show-error --insecure --noproxy '*' \
  --connect-to "$TENANT_CONNECT" \
  --cookie "$TENANT_COOKIE" \
  --output "$RUNTIME_DIR/operation.json" \
  --write-out '%{http_code}' \
  -H "Origin: $TENANT_ORIGIN" \
  -H 'Content-Type: application/json' \
  --data "$OPERATION_BODY" \
  "$TENANT_ORIGIN/api/integrations/operations")"
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

# Even if a periodic worker tick occurred after admission, the pre-opened circuit
# must have kept this governed operation in Ready state without an attempt.
queued_state="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='${TENANT_ID}' AND id='${OPERATION_ID}';")"
test "$queued_state" = "ready|0"

DB_CONTAINER_BEFORE="$("${compose[@]}" ps -q db)"
APP_CONTAINER_BEFORE="$("${compose[@]}" ps -q app)"
test -n "$DB_CONTAINER_BEFORE"
test -n "$APP_CONTAINER_BEFORE"
PG_SYSTEM_ID_BEFORE="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT system_identifier FROM pg_control_system();")"
test -n "$PG_SYSTEM_ID_BEFORE"

# Compose sends SIGTERM for an ordinary stop. A clean application exit must
# therefore exercise the executable's SIGTERM graceful-shutdown path.
"${compose[@]}" stop -t 20 app
docker logs "$APP_CONTAINER_BEFORE" > "$EVIDENCE_DIR/app-before-restart.log" 2>&1
docker inspect -f 'id={{.Id}} status={{.State.Status}} exit_code={{.State.ExitCode}} oom_killed={{.State.OOMKilled}}' \
  "$APP_CONTAINER_BEFORE" > "$EVIDENCE_DIR/app-before-restart-state.txt"
read -r FIRST_EXIT FIRST_OOM < <(docker inspect -f '{{.State.ExitCode}} {{.State.OOMKilled}}' "$APP_CONTAINER_BEFORE")
test "$FIRST_EXIT" = "0"
test "$FIRST_OOM" = "false"
grep -F 'shutdown signal received' "$EVIDENCE_DIR/app-before-restart.log" >/dev/null
grep -F 'SIGTERM' "$EVIDENCE_DIR/app-before-restart.log" >/dev/null
grep -F 'background workers stopped' "$EVIDENCE_DIR/app-before-restart.log" >/dev/null
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/app-before-restart.log" >/dev/null

# Create the durable crash cut-point only after the old process is stopped.
# The operation itself was admitted through the real Integration Registry API;
# this SQL only models the persisted state left after claim and before outcome.
ATTEMPT_ID="$(python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
)"
RUNTIME_EVENT_ID="$(python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
)"
"${compose[@]}" exec -T db psql -U talos -d talos \
  -v tenant_id="$TENANT_ID" \
  -v operation_id="$OPERATION_ID" \
  -v attempt_id="$ATTEMPT_ID" \
  -v runtime_event_id="$RUNTIME_EVENT_ID" <<'SQL'
\set ON_ERROR_STOP on
BEGIN;
UPDATE external_operations
   SET state='dispatching',
       attempt_count=1,
       classification=NULL,
       next_retry_at=NULL,
       updated_at=CURRENT_TIMESTAMP::text
 WHERE tenant_id=:'tenant_id'
   AND id=:'operation_id'
   AND state='ready';

INSERT INTO external_operation_attempts
    (id,tenant_id,operation_id,attempt_number,state,classification,request_hash,provider_result_ref,started_at,completed_at)
SELECT :'attempt_id',tenant_id,id,1,'dispatching',NULL,request_hash,NULL,CURRENT_TIMESTAMP::text,NULL
  FROM external_operations
 WHERE tenant_id=:'tenant_id'
   AND id=:'operation_id'
   AND state='dispatching';

INSERT INTO external_operation_runtime_events
    (id,tenant_id,operation_id,attempt_id,event_type,classification,occurred_at)
VALUES
    (:'runtime_event_id',:'tenant_id',:'operation_id',:'attempt_id','claimed','p9_sp06_restart_cutpoint',CURRENT_TIMESTAMP::text);
COMMIT;
SQL

pre_restart_state="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT state || '|' || attempt_count FROM external_operations WHERE tenant_id='${TENANT_ID}' AND id='${OPERATION_ID}';")"
test "$pre_restart_state" = "dispatching|1"

# Replace only the application container. PostgreSQL must remain the same
# running authority across this process boundary.
"${compose[@]}" rm -f app >/dev/null
"${compose[@]}" up -d --no-build app
APP_CONTAINER_AFTER="$("${compose[@]}" ps -q app)"
DB_CONTAINER_AFTER="$("${compose[@]}" ps -q db)"
test -n "$APP_CONTAINER_AFTER"
test "$APP_CONTAINER_AFTER" != "$APP_CONTAINER_BEFORE"
test "$DB_CONTAINER_AFTER" = "$DB_CONTAINER_BEFORE"

wait_ready "$EVIDENCE_DIR/ready-after.json"
PG_SYSTEM_ID_AFTER="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc "SELECT system_identifier FROM pg_control_system();")"
test "$PG_SYSTEM_ID_AFTER" = "$PG_SYSTEM_ID_BEFORE"

for _ in $(seq 1 60); do
  recovery_state="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
    "SELECT state || '|' || COALESCE(classification,'') FROM external_operations WHERE tenant_id='${TENANT_ID}' AND id='${OPERATION_ID}';")"
  if test "$recovery_state" = "unknown_outcome|worker_restarted_after_dispatch"; then
    break
  fi
  sleep 1
done
test "$recovery_state" = "unknown_outcome|worker_restarted_after_dispatch"

attempt_state="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT state || '|' || COALESCE(classification,'') || '|' || CASE WHEN completed_at IS NULL THEN 'open' ELSE 'completed' END FROM external_operation_attempts WHERE tenant_id='${TENANT_ID}' AND operation_id='${OPERATION_ID}' AND attempt_number=1;")"
test "$attempt_state" = "unknown_outcome|worker_restarted_after_dispatch|completed"

recovery_event_count="$("${compose[@]}" exec -T db psql -U talos -d talos -Atc \
  "SELECT COUNT(*) FROM external_operation_runtime_events WHERE tenant_id='${TENANT_ID}' AND operation_id='${OPERATION_ID}' AND event_type='recovered_after_restart' AND classification='worker_restarted_after_dispatch';")"
test "$recovery_event_count" = "1"

docker logs "$APP_CONTAINER_AFTER" > "$EVIDENCE_DIR/app-after-restart.log" 2>&1
grep -F 'Database profile: Postgres18' "$EVIDENCE_DIR/app-after-restart.log" >/dev/null
! grep -F 'SQLite local database path' "$EVIDENCE_DIR/app-after-restart.log" >/dev/null
! grep -F 'startup_recovery_failed' "$EVIDENCE_DIR/app-after-restart.log" >/dev/null

app_db_backend="$("${compose[@]}" exec -T app sh -lc 'printf "%s" "$DB_BACKEND"')"
test "$app_db_backend" = "postgres"
sqlite_files="$("${compose[@]}" exec -T app sh -lc "find /app -maxdepth 4 -type f \\( -name '*.db' -o -name '*.sqlite' -o -name '*.sqlite3' \\) -print")"
test -z "$sqlite_files"

# Prove the replacement process also exits cleanly under ordinary Compose
# SIGTERM rather than relying on container removal or SIGKILL.
"${compose[@]}" stop -t 20 app
docker logs "$APP_CONTAINER_AFTER" > "$EVIDENCE_DIR/app-final-shutdown.log" 2>&1
docker inspect -f 'id={{.Id}} status={{.State.Status}} exit_code={{.State.ExitCode}} oom_killed={{.State.OOMKilled}}' \
  "$APP_CONTAINER_AFTER" > "$EVIDENCE_DIR/app-final-shutdown-state.txt"
read -r FINAL_EXIT FINAL_OOM < <(docker inspect -f '{{.State.ExitCode}} {{.State.OOMKilled}}' "$APP_CONTAINER_AFTER")
test "$FINAL_EXIT" = "0"
test "$FINAL_OOM" = "false"
grep -F 'shutdown signal received' "$EVIDENCE_DIR/app-final-shutdown.log" >/dev/null
grep -F 'SIGTERM' "$EVIDENCE_DIR/app-final-shutdown.log" >/dev/null
grep -F 'background workers stopped' "$EVIDENCE_DIR/app-final-shutdown.log" >/dev/null
grep -F 'graceful shutdown complete' "$EVIDENCE_DIR/app-final-shutdown.log" >/dev/null

"${compose[@]}" stop -t 20 nginx db
"${compose[@]}" ps --all > "$EVIDENCE_DIR/compose-ps-stopped.txt"
"${compose[@]}" down -v --remove-orphans > "$EVIDENCE_DIR/compose-down.log" 2>&1

cat > "$EVIDENCE_DIR/restart-recovery-summary.txt" <<EOF
source_sha=$(git rev-parse HEAD)
db_container_before=$DB_CONTAINER_BEFORE
db_container_after=$DB_CONTAINER_AFTER
db_container_identity_stable=true
postgres_system_identifier_before=$PG_SYSTEM_ID_BEFORE
postgres_system_identifier_after=$PG_SYSTEM_ID_AFTER
postgres_system_identifier_stable=true
application_container_before=$APP_CONTAINER_BEFORE
application_container_after=$APP_CONTAINER_AFTER
application_container_replaced=true
first_app_shutdown_exit=$FIRST_EXIT
first_app_shutdown_oom_killed=$FIRST_OOM
restart_readiness=postgres
durable_operation_pre_restart=dispatching
durable_operation_post_restart=unknown_outcome
durable_recovery_classification=worker_restarted_after_dispatch
durable_recovery_event_count=$recovery_event_count
sqlite_production_files=0
final_app_shutdown_exit=$FINAL_EXIT
final_app_shutdown_oom_killed=$FINAL_OOM
clean_environment_exit=CLEAN_ENV_DEPLOYMENT_PASS
EOF

trap - EXIT
rm -rf "$RUNTIME_DIR"

echo "P9_SP06_RESTART_RECOVERY db_authority=stable app_replaced=pass durable_recovery=pass clean_shutdown=pass sqlite_absence=pass"
echo "CLEAN_ENV_DEPLOYMENT_PASS"
