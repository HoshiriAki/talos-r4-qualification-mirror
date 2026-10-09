#!/usr/bin/env bash
set -euo pipefail
umask 077

: "${P10_BACKUP_ARTIFACT:?P10_BACKUP_ARTIFACT is required}"
: "${P10_BACKUP_METADATA:?P10_BACKUP_METADATA is required}"
: "${P10_BACKUP_KEY_FILE:?P10_BACKUP_KEY_FILE is required}"
: "${P10_PG_CLIENT_IMAGE:?P10_PG_CLIENT_IMAGE is required}"
: "${P10_RESTORE_NETWORK:?P10_RESTORE_NETWORK is required}"
: "${P10_RESTORE_HOST:?P10_RESTORE_HOST is required}"
: "${P10_RESTORE_PORT:?P10_RESTORE_PORT is required}"
: "${P10_RESTORE_DATABASE:?P10_RESTORE_DATABASE is required}"
: "${P10_RESTORE_USER:?P10_RESTORE_USER is required}"
: "${P10_RESTORE_PGPASS_FILE:?P10_RESTORE_PGPASS_FILE is required}"
: "${P10_EXPECTED_SOURCE_SHA:?P10_EXPECTED_SOURCE_SHA is required}"
: "${P10_EXPECTED_SOURCE_TREE:?P10_EXPECTED_SOURCE_TREE is required}"
: "${P10_EXPECTED_MIGRATION_MANIFEST_SHA256:?P10_EXPECTED_MIGRATION_MANIFEST_SHA256 is required}"
: "${P10_EXPECTED_MIGRATION_HEAD:?P10_EXPECTED_MIGRATION_HEAD is required}"

for command in docker gpg gpgconf python3 sha256sum stat; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required restore command unavailable: $command" >&2
    exit 1
  }
done

private_file() {
  local path="$1"
  test -f "$path" || return 1
  local mode
  mode="$(stat -c '%a' "$path")"
  (( (8#$mode & 077) == 0 ))
}

private_file "$P10_BACKUP_KEY_FILE" || {
  echo "P10_BACKUP_KEY_FILE must be owner-private" >&2
  exit 1
}
private_file "$P10_RESTORE_PGPASS_FILE" || {
  echo "P10_RESTORE_PGPASS_FILE must be owner-private" >&2
  exit 1
}
test -s "$P10_BACKUP_ARTIFACT"
test -s "$P10_BACKUP_METADATA"
test -s "$P10_BACKUP_KEY_FILE"
test -s "$P10_RESTORE_PGPASS_FILE"

readarray -t meta < <(python3 - "$P10_BACKUP_METADATA" <<'PY'
import json,os,re,sys
with open(sys.argv[1], encoding="utf-8") as f:
    data=json.load(f)
assert data.get("schema") == "talos.p10.backup-metadata/v1"
assert data.get("source_sha") == os.environ["P10_EXPECTED_SOURCE_SHA"]
assert data.get("source_tree") == os.environ["P10_EXPECTED_SOURCE_TREE"]
migration=data.get("migration")
assert isinstance(migration,dict)
assert migration.get("repository_manifest_sha256") == os.environ["P10_EXPECTED_MIGRATION_MANIFEST_SHA256"]
assert migration.get("repository_head") == os.environ["P10_EXPECTED_MIGRATION_HEAD"]
assert isinstance(data.get("backup_id"), str) and data["backup_id"]
assert isinstance(data.get("pg_client_image"), str) and data["pg_client_image"]
dump=data.get("dump")
protected=data.get("protected_artifact")
assert isinstance(dump,dict) and dump.get("format") == "pg_dump-custom"
assert dump.get("checksum_algorithm") == "sha256"
assert isinstance(dump.get("plaintext_bytes"),int) and dump["plaintext_bytes"] > 0
assert re.fullmatch(r"[0-9a-f]{64}",dump.get("plaintext_sha256",""))
assert isinstance(protected,dict)
assert protected.get("checksum_algorithm") == "sha256"
assert protected.get("encryption_policy") == "gnupg-symmetric-aes256-s2k3-sha512"
assert re.fullmatch(r"[0-9a-f]{64}",protected.get("sha256",""))
assert isinstance(protected.get("bytes"),int) and protected["bytes"] > 0
assert isinstance(protected.get("filename"),str) and protected["filename"]
for value in (
    data["backup_id"],
    data["pg_client_image"],
    str(dump["plaintext_bytes"]),
    dump["plaintext_sha256"],
    str(protected["bytes"]),
    protected["sha256"],
    protected["filename"],
):
    print(value)
PY
)

test "${#meta[@]}" = "7"
backup_id="${meta[0]}"
metadata_image="${meta[1]}"
plaintext_bytes="${meta[2]}"
plaintext_sha256="${meta[3]}"
protected_bytes="${meta[4]}"
protected_sha256="${meta[5]}"
protected_filename="${meta[6]}"

test "$metadata_image" = "$P10_PG_CLIENT_IMAGE"
test "$protected_filename" = "$(basename "$P10_BACKUP_ARTIFACT")"
test "$protected_bytes" = "$(stat -c '%s' "$P10_BACKUP_ARTIFACT")"
actual_protected_sha256="$(sha256sum "$P10_BACKUP_ARTIFACT" | awk '{print $1}')"
test "$actual_protected_sha256" = "$protected_sha256"

runtime_root="${RUNNER_TEMP:-/tmp}"
work_dir="$(mktemp -d "$runtime_root/talos-p10-restore.XXXXXX")"
chmod 700 "$work_dir"
gpg_home="$work_dir/gnupg"
mkdir -p "$gpg_home"
chmod 700 "$gpg_home"
plaintext="$work_dir/${backup_id}.dump"

cleanup() {
  set +e
  rm -f "$plaintext"
  gpgconf --homedir "$gpg_home" --kill gpg-agent >/dev/null 2>&1 || true
  rm -rf "$work_dir"
}
trap cleanup EXIT

gpg --homedir "$gpg_home" --batch --yes --no-options \
  --pinentry-mode loopback \
  --passphrase-file "$P10_BACKUP_KEY_FILE" \
  --output "$plaintext" \
  --decrypt "$P10_BACKUP_ARTIFACT"

test -s "$plaintext"
test "$plaintext_bytes" = "$(stat -c '%s' "$plaintext")"
actual_plaintext_sha256="$(sha256sum "$plaintext" | awk '{print $1}')"
test "$actual_plaintext_sha256" = "$plaintext_sha256"

docker run --rm \
  --network "$P10_RESTORE_NETWORK" \
  -v "$P10_RESTORE_PGPASS_FILE:/run/secrets/p10-restore-pgpass:ro" \
  -v "$plaintext:/backup/restore.dump:ro" \
  -e PGPASSFILE=/run/secrets/p10-restore-pgpass \
  "$P10_PG_CLIENT_IMAGE" \
  pg_restore \
    --exit-on-error \
    --no-owner \
    --no-acl \
    -h "$P10_RESTORE_HOST" \
    -p "$P10_RESTORE_PORT" \
    -U "$P10_RESTORE_USER" \
    -d "$P10_RESTORE_DATABASE" \
    /backup/restore.dump

rm -f "$plaintext"
test ! -e "$plaintext"

printf 'P10_RESTORE_COMPLETED backup_id=%s target_database=%s ciphertext_sha256=%s plaintext_sha256=%s plaintext_removed=true\n' \
  "$backup_id" "$P10_RESTORE_DATABASE" "$protected_sha256" "$plaintext_sha256"
