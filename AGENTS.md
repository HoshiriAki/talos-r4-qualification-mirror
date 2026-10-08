# TALOS Implementation Agent Guide

This repository is the active TALOS/Maxwell implementation worktree. The default integration branch is `prototype`.

## Authority order

1. Explicit owner instruction for the current task.
2. This `AGENTS.md` and any narrower scoped `AGENTS.md`.
3. TALOS Knowledge / governance material made available from the private `talos-knowledge` authority.
4. Current code, migrations, tests, schemas, runtime behavior and CI evidence.
5. `policy/qualification/legacy-evidence/` only where a current checker explicitly requires historical qualification evidence.

Human-facing architecture, historical, migration and research documentation belongs in TALOS Knowledge rather than this implementation repository.

## Repository workflow

- Work from the current `prototype` baseline unless the owner specifies another base.
- Keep each Story Packet / cleanup scope isolated in its own branch and PR.
- Do not rewrite or force-push shared history.
- Preserve unrelated changes.
- Treat a PASS as valid only for the exact tested commit.
- Never infer implementation completion from prose, filenames or static search alone.

Project-local Claude/Codex/OpenCode/Harness skill packs are retired and must not be reintroduced. Repository checks and workflows are direct scripts under `scripts/`, package commands, Rust tests and GitHub Actions.

## Current structure

```text
backend/                 Rust/Axum implementation, migrations, modules and tests
frontend/                Vue 3 / TypeScript application and UI Lab
scripts/                 repository, CI and operational checks
policy/qualification/    machine qualification policy and legacy evidence inputs
.talos/                  TALOS-local authority/bootstrap material
.github/workflows/       Exact-Head, milestone and maintenance workflows
```

## Direct validation

```bash
pnpm quality:ci-policy:test
pnpm quality:ci-policy
pnpm quality:ci-package:test
pnpm quality:ci-package
pnpm quality:layout
pnpm quality:talos-ops
pnpm quality:frontend
pnpm ui:registry:check

cd backend
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked --no-fail-fast
cargo check --workspace --no-default-features --features postgres --locked
```

Use the current `package.json`, Cargo workspace and workflow files as command authority when commands evolve.

## CI model

Automatic Exact-Head qualification validates the exact candidate SHA, global/static boundaries, frontend/Rust/PostgreSQL quality and the current package dispatcher. Historical milestone rehearsal is explicit-dispatch and SHA-pinned; it is not replayed chronologically on every PR.

When GitHub-hosted qualification is executed through an ephemeral public R-scoped mirror, the private implementation repository remains authority. Mirror transport MUST bind the private source commit and source root tree, pass `quality:public-surface`, and use a parentless transport commit. The mirror SHA is transport identity only; it MUST NOT replace the private source SHA in provenance or release claims.

Public mirror package qualification is explicit `workflow_dispatch`. Broad `.talos-evidence` upload is disabled for mirror dispatch until a package defines a public-safe artifact allowlist.

## Runtime and test state

Generated diagnostics, disposable agent-test credentials and local runtime state belong under ignored `.talos-runtime/`. Do not commit credentials, local databases, screenshots, runtime reports or generated state.

## Review discipline

Before declaring completion:

- inspect changed paths and `git diff --check`;
- verify compilation/tests appropriate to the scope;
- distinguish local evidence from remote CI evidence;
- keep production/runtime facts separate from target knowledge;
- stop rather than fabricate evidence when an external credential or environment is unavailable.
