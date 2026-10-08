#!/usr/bin/env bash
set -euo pipefail
umask 077

: "${P10_PG_NETWORK:?P10_PG_NETWORK is required}"
: "${P10_PG_HOST:?P10_PG_HOST is required}"
: "${P10_PG_PORT:?P10_PG_PORT is required}"
: "${P10_PG_DATABASE:?P10_PG_DATABASE is required}"
: "${P10_PG_USER:?P10_PG_USER is required}"
: "${P10_PGPASS_FILE:?P10_PGPASS_FILE is required}"
: "${P10_BACKUP_KEY_FILE:?P10_BACKUP_KEY_FILE is required}"
: "${P10_BACKUP_OUTPUT_DIR:?P10_BACKUP_OUTPUT_DIR is required}"
: "${P10_SOURCE_SHA:?P10_SOURCE_SHA is required}"
: "${P10_SOURCE_TREE:?P10_SOURCE_TREE is required}"
: "${P10_MIGRATION_MANIFEST_SHA256:?P10_MIGRATION_MANIFEST_SHA256 is required}"
: "${P10_MIGRATION_HEAD:?P10_MIGRATION_HEAD is required}"
: "${P10_OPERATOR_ID:?P10_OPERATOR_ID is required}"
: "${P10_RUN_ID:?P10_RUN_ID is required}"
: "${P10_PG_CLIENT_IMAGE:?P10_PG_CLIENT_IMAGE is required}"

for command in docker gpg gpgconf python3 sha256sum stat date; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command unavailable: $command" >&2
    exit 1
  }
done

private_file() {
  local path="$1"
  test -f "$path" || return 1
  local mode
  mode="$(stat -c '%a' "$path")"
  # Secret files may be owner-read-only or owner-read/write, but never group/world accessible.
  (( (8#$mode & 077) == 0 ))
}

private_file "$P10_PGPASS_FILE" || {
  echo "P10_PGPASS_FILE must be an owner-private regular file" >&2
  exit 1
}
private_file "$P10_BACKUP_KEY_FILE" || {
  echo "P10_BACKUP_KEY_FILE must be an owner-private regular file" >&2
  exit 1
}

test -s "$P10_PGPASS_FILE"
test -s "$P10_BACKUP_KEY_FILE"

mkdir -p "$P10_BACKUP_OUTPUT_DIR"
chmod 700 "$P10_BACKUP_OUTPUT_DIR"

safe_run_id="$(printf '%s' "$P10_RUN_ID" | tr -cd 'A-Za-z0-9_.-')"
test -n "$safe_run_id"
created_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
backup_id="p10-${safe_run_id}-$(date -u +%Y%m%dT%H%M%SZ)"
work_dir="$(mktemp -d "$P10_BACKUP_OUTPUT_DIR/.p10-backup-work.XXXXXX")"
chmod 700 "$work_dir"
gpg_home="$work_dir/gnupg"
mkdir -p "$gpg_home"
chmod 700 "$gpg_home"

plaintext="$work_dir/${backup_id}.dump"
protected_tmp="$work_dir/${backup_id}.dump.gpg"
metadata_tmp="$work_dir/${backup_id}.metadata.json"
verify_plaintext="$work_dir/${backup_id}.verify.dump"
protected_final="$P10_BACKUP_OUTPUT_DIR/${backup_id}.dump.gpg"
metadata_final="$P10_BACKUP_OUTPUT_DIR/${backup_id}.metadata.json"
published=false

cleanup() {
  set +e
  rm -f "$plaintext" "$verify_plaintext" "$protected_tmp" "$metadata_tmp"
  gpgconf --homedir "$gpg_home" --kill gpg-agent >/dev/null 2>&1 || true
  rm -rf "$work_dir"
  if test "$published" != "true"; then
    rm -f "$protected_final" "$metadata_final"
  fi
}
trap cleanup EXIT

test ! -e "$protected_final"
test ! -e "$metadata_final"

pg_client() {
  docker run --rm \
    --network "$P10_PG_NETWORK" \
    -v "$P10_PGPASS_FILE:/run/secrets/p10-pgpass:ro" \
    -e PGPASSFILE=/run/secrets/p10-pgpass \
    "$P10_PG_CLIENT_IMAGE" "$@"
}

source_identity="$(pg_client psql \
  -X -v ON_ERROR_STOP=1 -At -F '|' \
  -h "$P10_PG_HOST" -p "$P10_PG_PORT" -U "$P10_PG_USER" -d "$P10_PG_DATABASE" \
  -c "SELECT current_database(), current_user, current_setting('server_version_num'), pg_is_in_recovery(), (SELECT COUNT(*) FROM schema_migrations), COALESCE((SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1),'');")"

IFS='|' read -r source_database source_user server_version_num in_recovery migration_count latest_migration <<<"$source_identity"
test "$source_database" = "$P10_PG_DATABASE"
test "$source_user" = "$P10_PG_USER"
test -n "$server_version_num"
test "$in_recovery" = "f" || test "$in_recovery" = "false"
[[ "$migration_count" =~ ^[0-9]+$ ]]
test -n "$latest_migration"

# The client container writes only into the private work directory. The final
# publication directory never receives plaintext.
docker run --rm \
  --network "$P10_PG_NETWORK" \
  -v "$P10_PGPASS_FILE:/run/secrets/p10-pgpass:ro" \
  -v "$work_dir:/backup" \
  -e PGPASSFILE=/run/secrets/p10-pgpass \
  "$P10_PG_CLIENT_IMAGE" \
  pg_dump \
    -h "$P10_PG_HOST" \
    -p "$P10_PG_PORT" \
    -U "$P10_PG_USER" \
    -d "$P10_PG_DATABASE" \
    --format=custom \
    --no-owner \
    --no-acl \
    --file="/backup/${backup_id}.dump"

test -s "$plaintext"
plaintext_bytes="$(stat -c '%s' "$plaintext")"
plaintext_sha256="$(sha256sum "$plaintext" | awk '{print $1}')"
[[ "$plaintext_sha256" =~ ^[0-9a-f]{64}$ ]]

gpg_version="$(gpg --homedir "$gpg_home" --version | head -n1)"
gpg --homedir "$gpg_home" --batch --yes --no-options \
  --pinentry-mode loopback \
  --passphrase-file "$P10_BACKUP_KEY_FILE" \
  --symmetric \
  --cipher-algo AES256 \
  --s2k-mode 3 \
  --s2k-digest-algo SHA512 \
  --compress-algo none \
  --output "$protected_tmp" \
  "$plaintext"

test -s "$protected_tmp"
protected_bytes="$(stat -c '%s' "$protected_tmp")"
protected_sha256="$(sha256sum "$protected_tmp" | awk '{print $1}')"
[[ "$protected_sha256" =~ ^[0-9a-f]{64}$ ]]

# Immediate round-trip verification is part of backup publication. There is no
# path that publishes ciphertext merely because encryption returned success.
gpg --homedir "$gpg_home" --batch --yes --no-options \
  --pinentry-mode loopback \
  --passphrase-file "$P10_BACKUP_KEY_FILE" \
  --output "$verify_plaintext" \
  --decrypt "$protected_tmp"

test -s "$verify_plaintext"
verify_sha256="$(sha256sum "$verify_plaintext" | awk '{print $1}')"
test "$verify_sha256" = "$plaintext_sha256"

docker run --rm --network none \
  -v "$verify_plaintext:/backup/verified.dump:ro" \
  "$P10_PG_CLIENT_IMAGE" \
  pg_restore --list /backup/verified.dump >/dev/null

export P10_META_BACKUP_ID="$backup_id"
export P10_META_CREATED_AT="$created_at"
export P10_META_SOURCE_DATABASE="$source_database"
export P10_META_SOURCE_USER="$source_user"
export P10_META_SERVER_VERSION_NUM="$server_version_num"
export P10_META_IN_RECOVERY="$in_recovery"
export P10_META_MIGRATION_COUNT="$migration_count"
export P10_META_LATEST_MIGRATION="$latest_migration"
export P10_META_PLAINTEXT_BYTES="$plaintext_bytes"
export P10_META_PLAINTEXT_SHA256="$plaintext_sha256"
export P10_META_PROTECTED_BYTES="$protected_bytes"
export P10_META_PROTECTED_SHA256="$protected_sha256"
export P10_META_PROTECTED_FILENAME="$(basename "$protected_final")"
export P10_META_GPG_VERSION="$gpg_version"
export P10_META_SOURCE_TREE="$P10_SOURCE_TREE"
export P10_META_MIGRATION_MANIFEST_SHA256="$P10_MIGRATION_MANIFEST_SHA256"
export P10_META_MIGRATION_HEAD="$P10_MIGRATION_HEAD"
export P10_META_OPERATOR_ID="$P10_OPERATOR_ID"

python3 - "$metadata_tmp" <<'PY'
import json, os, sys

metadata = {
    "schema": "talos.p10.backup-metadata/v1",
    "backup_id": os.environ["P10_META_BACKUP_ID"],
    "created_at": os.environ["P10_META_CREATED_AT"],
    "source_sha": os.environ["P10_SOURCE_SHA"],
    "source_tree": os.environ["P10_META_SOURCE_TREE"],
    "run_id": os.environ["P10_RUN_ID"],
    "operator_id": os.environ["P10_META_OPERATOR_ID"],
    "source": {
        "database": os.environ["P10_META_SOURCE_DATABASE"],
        "user": os.environ["P10_META_SOURCE_USER"],
        "server_version_num": os.environ["P10_META_SERVER_VERSION_NUM"],
        "pg_is_in_recovery": os.environ["P10_META_IN_RECOVERY"] in ("t", "true"),
    },
    "migration": {
        "count": int(os.environ["P10_META_MIGRATION_COUNT"]),
        "latest_id": os.environ["P10_META_LATEST_MIGRATION"],
        "repository_manifest_sha256": os.environ["P10_META_MIGRATION_MANIFEST_SHA256"],
        "repository_head": os.environ["P10_META_MIGRATION_HEAD"],
    },
    "pg_client_image": os.environ["P10_PG_CLIENT_IMAGE"],
    "dump": {
        "format": "pg_dump-custom",
        "plaintext_bytes": int(os.environ["P10_META_PLAINTEXT_BYTES"]),
        "checksum_algorithm": "sha256",
        "plaintext_sha256": os.environ["P10_META_PLAINTEXT_SHA256"],
    },
    "protected_artifact": {
        "filename": os.environ["P10_META_PROTECTED_FILENAME"],
        "bytes": int(os.environ["P10_META_PROTECTED_BYTES"]),
        "checksum_algorithm": "sha256",
        "sha256": os.environ["P10_META_PROTECTED_SHA256"],
        "encryption_policy": "gnupg-symmetric-aes256-s2k3-sha512",
        "gpg_version": os.environ["P10_META_GPG_VERSION"],
    },
}
with open(sys.argv[1], "x", encoding="utf-8") as f:
    json.dump(metadata, f, sort_keys=True, indent=2)
    f.write("\n")
PY

test -s "$metadata_tmp"

# Plaintext is destroyed before the retained pair is published.
rm -f "$plaintext" "$verify_plaintext"
test ! -e "$plaintext"
test ! -e "$verify_plaintext"

mv "$protected_tmp" "$protected_final"
mv "$metadata_tmp" "$metadata_final"
published=true

printf 'P10_BACKUP_CREATED backup_id=%s protected_file=%s metadata_file=%s source_database=%s migration_count=%s latest_migration=%s\n' \
  "$backup_id" "$(basename "$protected_final")" "$(basename "$metadata_final")" \
  "$source_database" "$migration_count" "$latest_migration"
