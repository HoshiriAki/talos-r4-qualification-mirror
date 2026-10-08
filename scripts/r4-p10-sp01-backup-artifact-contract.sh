#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="$ROOT/.talos-evidence/p10-sp01"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
STATUS_FILE="$EVIDENCE_DIR/qualification-status.txt"
printf 'status=started\n' > "$STATUS_FILE"
qualification_status="fail"
RUNTIME_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/talos-p10-sp01.XXXXXX")"
chmod 700 "$RUNTIME_DIR"
mkdir -p "$RUNTIME_DIR/out"
chmod 700 "$RUNTIME_DIR/out"

run_id="${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}"
safe_run_id="$(printf '%s' "$run_id" | tr -cd 'A-Za-z0-9_.-')"
test -n "$safe_run_id"
NETWORK="talos-p10-sp01-${safe_run_id}"
DB_CONTAINER="talos-p10-sp01-db-${safe_run_id}"
PG_IMAGE="postgres:18-alpine@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2"
DB_PASSWORD="$(openssl rand -hex 24)"
PGPASS_FILE="$RUNTIME_DIR/pgpass"
BACKUP_KEY_FILE="$RUNTIME_DIR/backup-key"

cleanup() {
  exit_code=$?
  set +e
  if test "$qualification_status" = "pass" && test "$exit_code" -eq 0; then
    printf 'status=pass\nexit_code=0\n' > "$STATUS_FILE"
  else
    printf 'status=fail\nexit_code=%s\n' "$exit_code" > "$STATUS_FILE"
  fi
  docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true
  docker network rm "$NETWORK" >/dev/null 2>&1 || true
  rm -rf "$RUNTIME_DIR"
  return "$exit_code"
}
trap cleanup EXIT

for command in docker gpg openssl python3 sha256sum git; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required qualification command unavailable: $command" >&2
    exit 1
  }
done

docker version > "$EVIDENCE_DIR/docker-version.txt"
gpg --version > "$EVIDENCE_DIR/gpg-version.txt"
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
migration_manifest_sha256="$(
  for file in "${migration_files[@]}"; do
    printf '%s  %s\n' "$(sha256sum "$file" | awk '{print $1}')" "$file"
  done | sha256sum | awk '{print $1}'
)"
[[ "$migration_manifest_sha256" =~ ^[0-9a-f]{64}$ ]]
printf 'head=%s\nsha256=%s\n' "$migration_head" "$migration_manifest_sha256" > "$EVIDENCE_DIR/migration-manifest.txt"

printf 'source-db:5432:talos_p10:talos:%s\n' "$DB_PASSWORD" > "$PGPASS_FILE"
chmod 600 "$PGPASS_FILE"
openssl rand -base64 48 > "$BACKUP_KEY_FILE"
chmod 600 "$BACKUP_KEY_FILE"

docker network create "$NETWORK" >/dev/null
docker run -d \
  --name "$DB_CONTAINER" \
  --network "$NETWORK" \
  --network-alias source-db \
  -e POSTGRES_USER=talos \
  -e POSTGRES_PASSWORD="$DB_PASSWORD" \
  -e POSTGRES_DB=talos_p10 \
  "$PG_IMAGE" >/dev/null

for _ in $(seq 1 60); do
  if docker exec "$DB_CONTAINER" pg_isready -U talos -d talos_p10 >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$DB_CONTAINER" pg_isready -U talos -d talos_p10 >/dev/null

docker exec -i -e PGPASSWORD="$DB_PASSWORD" "$DB_CONTAINER" \
  psql -X -v ON_ERROR_STOP=1 -U talos -d talos_p10 <<'SQL'
CREATE TABLE schema_migrations (
  id TEXT PRIMARY KEY,
  description TEXT NOT NULL,
  applied_at TEXT NOT NULL
);
INSERT INTO schema_migrations (id, description, applied_at)
VALUES ('p10_sp01_fixture', 'P10 SP01 protected backup contract fixture', CURRENT_TIMESTAMP::text);
CREATE TABLE p10_backup_fixture (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  payload TEXT NOT NULL
);
INSERT INTO p10_backup_fixture (id, tenant_id, payload)
VALUES ('fixture-a', 'tenant-a', 'protected-backup-roundtrip');
SQL

export P10_PG_NETWORK="$NETWORK"
export P10_PG_HOST="source-db"
export P10_PG_PORT="5432"
export P10_PG_DATABASE="talos_p10"
export P10_PG_USER="talos"
export P10_PGPASS_FILE="$PGPASS_FILE"
export P10_BACKUP_KEY_FILE="$BACKUP_KEY_FILE"
export P10_BACKUP_OUTPUT_DIR="$RUNTIME_DIR/out"
export P10_SOURCE_SHA="$TALOS_QUALIFIED_SOURCE_SHA"
export P10_SOURCE_TREE="$TALOS_QUALIFIED_SOURCE_TREE_SHA"
export P10_MIGRATION_MANIFEST_SHA256="$migration_manifest_sha256"
export P10_MIGRATION_HEAD="$migration_head"
export P10_OPERATOR_ID="${GITHUB_ACTOR:-local-operator}"
export P10_RUN_ID="$run_id"
export P10_PG_CLIENT_IMAGE="$PG_IMAGE"

bash scripts/r4-p10-pg-backup.sh > "$EVIDENCE_DIR/backup.log" 2>&1

readarray -t artifacts < <(find "$RUNTIME_DIR/out" -maxdepth 1 -type f -name '*.dump.gpg' -print)
readarray -t metadata_files < <(find "$RUNTIME_DIR/out" -maxdepth 1 -type f -name '*.metadata.json' -print)
test "${#artifacts[@]}" = "1"
test "${#metadata_files[@]}" = "1"
BACKUP_ARTIFACT="${artifacts[0]}"
BACKUP_METADATA="${metadata_files[0]}"

test -z "$(find "$RUNTIME_DIR/out" -maxdepth 1 -type f -name '*.dump' -print -quit)"

export P10_BACKUP_ARTIFACT="$BACKUP_ARTIFACT"
export P10_BACKUP_METADATA="$BACKUP_METADATA"
bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/verify.log" 2>&1

# Ciphertext tampering must be rejected before decryption is trusted.
TAMPERED_ARTIFACT="$RUNTIME_DIR/tampered.dump.gpg"
cp "$BACKUP_ARTIFACT" "$TAMPERED_ARTIFACT"
python3 - "$TAMPERED_ARTIFACT" <<'PY'
import os, sys
path=sys.argv[1]
with open(path, 'r+b') as f:
    size=os.fstat(f.fileno()).st_size
    assert size > 64
    offset=min(64, size-1)
    f.seek(offset)
    value=f.read(1)
    f.seek(offset)
    f.write(bytes([value[0] ^ 0x01]))
PY
if P10_BACKUP_ARTIFACT="$TAMPERED_ARTIFACT" \
   P10_BACKUP_METADATA="$BACKUP_METADATA" \
   P10_BACKUP_KEY_FILE="$BACKUP_KEY_FILE" \
   P10_PG_CLIENT_IMAGE="$PG_IMAGE" \
   bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/tampered-ciphertext.log" 2>&1; then
  echo "tampered ciphertext unexpectedly verified" >&2
  exit 1
fi
printf 'tampered_ciphertext=rejected\n' > "$EVIDENCE_DIR/tamper-summary.txt"

# Metadata that lies about the decrypted plaintext digest must also fail.
TAMPERED_METADATA="$RUNTIME_DIR/tampered.metadata.json"
python3 - "$BACKUP_METADATA" "$TAMPERED_METADATA" <<'PY'
import json, sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
data['dump']['plaintext_sha256']='0'*64
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    json.dump(data, f, sort_keys=True, indent=2)
    f.write('\n')
PY
if P10_BACKUP_ARTIFACT="$BACKUP_ARTIFACT" \
   P10_BACKUP_METADATA="$TAMPERED_METADATA" \
   P10_BACKUP_KEY_FILE="$BACKUP_KEY_FILE" \
   P10_PG_CLIENT_IMAGE="$PG_IMAGE" \
   bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/tampered-metadata.log" 2>&1; then
  echo "tampered metadata unexpectedly verified" >&2
  exit 1
fi
printf 'tampered_metadata=rejected\n' >> "$EVIDENCE_DIR/tamper-summary.txt"

# Wrong runtime key material must never produce a usable plaintext artifact.
WRONG_KEY="$RUNTIME_DIR/wrong-key"
openssl rand -base64 48 > "$WRONG_KEY"
chmod 600 "$WRONG_KEY"
if P10_BACKUP_ARTIFACT="$BACKUP_ARTIFACT" \
   P10_BACKUP_METADATA="$BACKUP_METADATA" \
   P10_BACKUP_KEY_FILE="$WRONG_KEY" \
   P10_PG_CLIENT_IMAGE="$PG_IMAGE" \
   bash scripts/r4-p10-verify-backup.sh > "$EVIDENCE_DIR/wrong-key.log" 2>&1; then
  echo "wrong key unexpectedly verified" >&2
  exit 1
fi
printf 'wrong_key=rejected\n' >> "$EVIDENCE_DIR/tamper-summary.txt"

cp "$BACKUP_ARTIFACT" "$EVIDENCE_DIR/"
cp "$BACKUP_METADATA" "$EVIDENCE_DIR/"

# Retained evidence must not contain qualification secret material or plaintext dumps.
if grep -R -F -l --binary-files=without-match "$DB_PASSWORD" "$EVIDENCE_DIR" >/dev/null 2>&1; then
  echo "database password leaked into retained evidence" >&2
  exit 1
fi
backup_key_value="$(cat "$BACKUP_KEY_FILE")"
if grep -R -F -l --binary-files=without-match "$backup_key_value" "$EVIDENCE_DIR" >/dev/null 2>&1; then
  echo "backup key leaked into retained evidence" >&2
  exit 1
fi
test -z "$(find "$EVIDENCE_DIR" -maxdepth 1 -type f -name '*.dump' -print -quit)"

python3 - "$BACKUP_METADATA" > "$EVIDENCE_DIR/artifact-summary.txt" <<'PY'
import json, sys
with open(sys.argv[1], encoding='utf-8') as f:
    data=json.load(f)
print('schema='+data['schema'])
print('source_sha='+data['source_sha'])
print('source_tree='+data['source_tree'])
print('operator_id='+data['operator_id'])
print('source_database='+data['source']['database'])
print('server_version_num='+data['source']['server_version_num'])
print('migration_count='+str(data['migration']['count']))
print('latest_migration='+data['migration']['latest_id'])
print('repository_migration_head='+data['migration']['repository_head'])
print('repository_migration_manifest_sha256='+data['migration']['repository_manifest_sha256'])
print('dump_format='+data['dump']['format'])
print('plaintext_checksum_algorithm='+data['dump']['checksum_algorithm'])
print('protected_checksum_algorithm='+data['protected_artifact']['checksum_algorithm'])
print('encryption_policy='+data['protected_artifact']['encryption_policy'])
PY

qualification_status="pass"
printf 'P10_SP01_BACKUP_ARTIFACT_CONTRACT source_sha=%s source_tree=%s backup=protected verify=pass ciphertext_tamper=rejected metadata_tamper=rejected wrong_key=rejected plaintext_retained=false recovery_rehearsal=not_claimed rto_rpo=not_measured\n' "$P10_SOURCE_SHA" "$P10_SOURCE_TREE"
echo "P10_BACKUP_ARTIFACT_CONTRACT_PASS"
