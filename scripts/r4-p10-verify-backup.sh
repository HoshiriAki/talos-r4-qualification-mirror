#!/usr/bin/env bash
set -euo pipefail
umask 077

: "${P10_BACKUP_ARTIFACT:?P10_BACKUP_ARTIFACT is required}"
: "${P10_BACKUP_METADATA:?P10_BACKUP_METADATA is required}"
: "${P10_BACKUP_KEY_FILE:?P10_BACKUP_KEY_FILE is required}"
: "${P10_PG_CLIENT_IMAGE:?P10_PG_CLIENT_IMAGE is required}"

for command in docker gpg gpgconf python3 sha256sum stat; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command unavailable: $command" >&2
    exit 1
  }
done

test -f "$P10_BACKUP_ARTIFACT"
test -s "$P10_BACKUP_ARTIFACT"
test -f "$P10_BACKUP_METADATA"
test -s "$P10_BACKUP_METADATA"
test -f "$P10_BACKUP_KEY_FILE"
test -s "$P10_BACKUP_KEY_FILE"

key_mode="$(stat -c '%a' "$P10_BACKUP_KEY_FILE")"
(( (8#$key_mode & 077) == 0 )) || {
  echo "P10_BACKUP_KEY_FILE must be owner-private" >&2
  exit 1
}

readarray -t meta < <(python3 - "$P10_BACKUP_METADATA" <<'PY'
import json, re, sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

assert data.get("schema") == "talos.p10.backup-metadata/v1"
for key in ("backup_id", "created_at", "source_sha", "source_tree", "run_id", "operator_id", "pg_client_image"):
    value = data.get(key)
    assert isinstance(value, str) and value
assert re.fullmatch(r"[0-9a-f]{40}", data["source_sha"])
assert re.fullmatch(r"[0-9a-f]{40}", data["source_tree"])
source = data.get("source")
assert isinstance(source, dict)
for key in ("database", "user", "server_version_num"):
    value = source.get(key)
    assert isinstance(value, str) and value
assert isinstance(source.get("pg_is_in_recovery"), bool)
migration = data.get("migration")
assert isinstance(migration, dict)
assert isinstance(migration.get("count"), int) and migration["count"] >= 0
assert isinstance(migration.get("latest_id"), str) and migration["latest_id"]
assert re.fullmatch(r"[0-9a-f]{64}", migration.get("repository_manifest_sha256", ""))
assert isinstance(migration.get("repository_head"), str) and migration["repository_head"].endswith(".sql")
dump = data.get("dump")
assert isinstance(dump, dict)
assert dump.get("format") == "pg_dump-custom"
assert dump.get("checksum_algorithm") == "sha256"
assert isinstance(dump.get("plaintext_bytes"), int) and dump["plaintext_bytes"] > 0
assert re.fullmatch(r"[0-9a-f]{64}", dump.get("plaintext_sha256", ""))
protected = data.get("protected_artifact")
assert isinstance(protected, dict)
assert protected.get("checksum_algorithm") == "sha256"
assert protected.get("encryption_policy") == "gnupg-symmetric-aes256-s2k3-sha512"
assert isinstance(protected.get("bytes"), int) and protected["bytes"] > 0
assert re.fullmatch(r"[0-9a-f]{64}", protected.get("sha256", ""))
assert isinstance(protected.get("filename"), str) and protected["filename"]
for value in (
    data["backup_id"],
    data["pg_client_image"],
    dump["plaintext_sha256"],
    str(dump["plaintext_bytes"]),
    protected["filename"],
    protected["sha256"],
    str(protected["bytes"]),
):
    print(value)
PY
)

test "${#meta[@]}" = "7"
backup_id="${meta[0]}"
metadata_image="${meta[1]}"
plaintext_sha256="${meta[2]}"
plaintext_bytes="${meta[3]}"
protected_filename="${meta[4]}"
protected_sha256="${meta[5]}"
protected_bytes="${meta[6]}"

test "$metadata_image" = "$P10_PG_CLIENT_IMAGE"
test "$protected_filename" = "$(basename "$P10_BACKUP_ARTIFACT")"
test "$protected_bytes" = "$(stat -c '%s' "$P10_BACKUP_ARTIFACT")"
actual_protected_sha256="$(sha256sum "$P10_BACKUP_ARTIFACT" | awk '{print $1}')"
test "$actual_protected_sha256" = "$protected_sha256"

runtime_root="${RUNNER_TEMP:-/tmp}"
work_dir="$(mktemp -d "$runtime_root/talos-p10-verify.XXXXXX")"
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

docker run --rm --network none \
  -v "$plaintext:/backup/verified.dump:ro" \
  "$P10_PG_CLIENT_IMAGE" \
  pg_restore --list /backup/verified.dump >/dev/null

rm -f "$plaintext"
test ! -e "$plaintext"

printf 'P10_BACKUP_VERIFIED backup_id=%s ciphertext_sha256=%s plaintext_sha256=%s\n' \
  "$backup_id" "$protected_sha256" "$plaintext_sha256"
