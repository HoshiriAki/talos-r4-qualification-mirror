#!/usr/bin/env bash
set -euo pipefail

SOURCE_REF="HEAD"
BASE_REF=""
MIRROR_REMOTE=""
MIRROR_BRANCH="main"
DISPATCH_REPOSITORY=""
PACKAGE_BRANCH=""
DRY_RUN="false"

usage() {
  cat <<'EOF'
Usage:
  scripts/export-public-qualification-mirror.sh \
    [--source-ref <ref>] \
    [--base-ref <ref>] \
    [--remote <git-remote-url>] \
    [--branch <mirror-branch>] \
    [--dispatch-repository <owner/repo>] \
    [--package-branch <private-source-branch>] \
    [--dry-run]

Creates a parentless transport commit that points at the exact approved source tree.
No source commit history is attached to the mirror commit.

--base-ref is optional. When supplied, git diff --check is executed against
<base-ref>...<source-ref> before export.

When --dispatch-repository is supplied, the script uses the local gh CLI after
push to dispatch exact-head-qualification.yml with authoritative source commit/tree
inputs. --package-branch is forwarded to current-package qualification resolution.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --source-ref)
      SOURCE_REF="${2:?missing value for --source-ref}"
      shift 2
      ;;
    --base-ref)
      BASE_REF="${2:?missing value for --base-ref}"
      shift 2
      ;;
    --remote)
      MIRROR_REMOTE="${2:?missing value for --remote}"
      shift 2
      ;;
    --branch)
      MIRROR_BRANCH="${2:?missing value for --branch}"
      shift 2
      ;;
    --dispatch-repository)
      DISPATCH_REPOSITORY="${2:?missing value for --dispatch-repository}"
      shift 2
      ;;
    --package-branch)
      PACKAGE_BRANCH="${2:?missing value for --package-branch}"
      shift 2
      ;;
    --dry-run)
      DRY_RUN="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

SOURCE_SHA="$(git rev-parse --verify "${SOURCE_REF}^{commit}")"
SOURCE_TREE="$(git rev-parse --verify "${SOURCE_SHA}^{tree}")"

if command -v node >/dev/null 2>&1; then
  NODE_BIN="node"
elif command -v node.exe >/dev/null 2>&1; then
  NODE_BIN="node.exe"
elif command -v where.exe >/dev/null 2>&1; then
  WINDOWS_NODE="$(where.exe node 2>/dev/null | tr -d '\r' | head -n 1 || true)"
  if [[ -n "$WINDOWS_NODE" ]]; then
    if command -v wslpath >/dev/null 2>&1; then
      NODE_BIN="$(wslpath "$WINDOWS_NODE")"
    else
      NODE_BIN="$WINDOWS_NODE"
    fi
  else
    echo "Node.js is required: no node executable was found in Bash or Windows PATH" >&2
    exit 2
  fi
else
  echo "Node.js is required: no node executable was found in this shell" >&2
  exit 2
fi

"$NODE_BIN" scripts/check-public-surface.test.mjs
"$NODE_BIN" scripts/check-public-surface.mjs --ref "$SOURCE_SHA"

if [[ -n "$BASE_REF" ]]; then
  BASE_SHA="$(git rev-parse --verify "${BASE_REF}^{commit}")"
  git diff --check "${BASE_SHA}...${SOURCE_SHA}"
else
  BASE_SHA=""
fi

message="$(cat <<EOF
TALOS R4 qualification snapshot

source-repository: HoshiriAki/TESSERACT-WAREHOUSiNG
source-commit: ${SOURCE_SHA}
source-tree: ${SOURCE_TREE}
source-base: ${BASE_SHA:-none}
public-surface-policy: scripts/check-public-surface.mjs
history-model: parentless-exact-tree
EOF
)"

export GIT_AUTHOR_NAME="TALOS Qualification Mirror"
export GIT_AUTHOR_EMAIL="qualification-mirror@talos.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

MIRROR_COMMIT="$(printf '%s\n' "$message" | git commit-tree "$SOURCE_TREE")"
MIRROR_TREE="$(git rev-parse --verify "${MIRROR_COMMIT}^{tree}")"

if [[ "$MIRROR_TREE" != "$SOURCE_TREE" ]]; then
  echo "mirror tree mismatch: source=$SOURCE_TREE mirror=$MIRROR_TREE" >&2
  exit 1
fi

cat <<EOF
TALOS_PUBLIC_MIRROR_EXPORT_READY
source_sha=${SOURCE_SHA}
source_tree=${SOURCE_TREE}
mirror_commit=${MIRROR_COMMIT}
mirror_tree=${MIRROR_TREE}
mirror_branch=${MIRROR_BRANCH}
EOF

if [[ "$DRY_RUN" == "true" ]]; then
  exit 0
fi

if [[ -z "$MIRROR_REMOTE" ]]; then
  echo "--remote is required unless --dry-run is used" >&2
  exit 2
fi

# Authentication boundary:
# local object construction may use the shell's native git, but on Windows/WSL
# prefer Git for Windows for the authenticated push so it can reuse the user's
# GitHub CLI / Git Credential Manager configuration.
PUSH_GIT_BIN="git"
if command -v git.exe >/dev/null 2>&1; then
  PUSH_GIT_BIN="git.exe"
elif command -v where.exe >/dev/null 2>&1; then
  WINDOWS_GIT="$(where.exe git 2>/dev/null | tr -d '\r' | head -n 1 || true)"
  if [[ -n "$WINDOWS_GIT" ]]; then
    if command -v wslpath >/dev/null 2>&1; then
      PUSH_GIT_BIN="$(wslpath "$WINDOWS_GIT")"
    else
      PUSH_GIT_BIN="$WINDOWS_GIT"
    fi
  fi
fi

echo "TALOS_PUBLIC_MIRROR_PUSH_GIT=${PUSH_GIT_BIN}"

# Dedicated ephemeral mirror: replacing the single qualification branch is intentional.
# The parentless commit makes source history unreachable from the pushed ref.
"$PUSH_GIT_BIN" push --force "$MIRROR_REMOTE" "${MIRROR_COMMIT}:refs/heads/${MIRROR_BRANCH}"

echo "TALOS_PUBLIC_MIRROR_EXPORT_PUSHED remote=${MIRROR_REMOTE} branch=${MIRROR_BRANCH} commit=${MIRROR_COMMIT}"

if [[ -n "$DISPATCH_REPOSITORY" ]]; then
  if command -v gh >/dev/null 2>&1; then
    GH_BIN="gh"
  elif command -v gh.exe >/dev/null 2>&1; then
    GH_BIN="gh.exe"
  elif command -v where.exe >/dev/null 2>&1; then
    WINDOWS_GH="$(where.exe gh 2>/dev/null | tr -d '\r' | head -n 1 || true)"
    if [[ -n "$WINDOWS_GH" ]]; then
      if command -v wslpath >/dev/null 2>&1; then
        GH_BIN="$(wslpath "$WINDOWS_GH")"
      else
        GH_BIN="$WINDOWS_GH"
      fi
    else
      echo "GitHub CLI is required for --dispatch-repository: no gh executable was found in Bash or Windows PATH" >&2
      exit 2
    fi
  else
    echo "GitHub CLI is required for --dispatch-repository: no gh executable was found in this shell" >&2
    exit 2
  fi

  dispatch_args=(
    workflow run exact-head-qualification.yml
    --repo "$DISPATCH_REPOSITORY"
    --ref "$MIRROR_BRANCH"
    -f "source_commit_sha=$SOURCE_SHA"
    -f "source_tree_sha=$SOURCE_TREE"
    -f "source_repository=HoshiriAki/TESSERACT-WAREHOUSiNG"
  )
  if [[ -n "$PACKAGE_BRANCH" ]]; then
    dispatch_args+=( -f "package_branch=$PACKAGE_BRANCH" )
  fi

  "$GH_BIN" "${dispatch_args[@]}"
  echo "TALOS_PUBLIC_MIRROR_QUALIFICATION_DISPATCHED repository=${DISPATCH_REPOSITORY} source_sha=${SOURCE_SHA} source_tree=${SOURCE_TREE}"
fi
