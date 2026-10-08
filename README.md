# TALOS

TALOS is the active implementation repository for the current TALOS/Maxwell business platform prototype.

The repository contains a Rust/Axum backend, Vue 3/TypeScript frontend, database migrations, runtime tooling and exact-head qualification infrastructure. The default integration branch is `prototype`.

## Authority boundary

Executable truth lives in code, migrations, tests, schemas and CI/runtime evidence.

Long-form architecture, historical records, migration narratives, research and design documentation are maintained in the private TALOS Knowledge repository. A small compatibility set under `policy/qualification/legacy-evidence/` exists only because current machine gates still verify historical qualification markers; it is not the human documentation authority.

## Environment

- Node.js 22
- pnpm version pinned by `package.json#packageManager`
- Rust stable with edition 2024 support
- PostgreSQL 18 for production qualification paths
- SQLite remains available where the implementation explicitly uses it for local/test compatibility

## Install and run

```powershell
pnpm install --frozen-lockfile
start.cmd doctor
start.cmd
```

Common local runtime endpoints are selected by the supervisor. Generated diagnostics and disposable test state are written under ignored `.talos-runtime/`.

For an isolated agent/Playwright test session:

```powershell
pnpm agent:test:start
```

The launcher creates temporary credentials and a disposable database under `.talos-runtime/agent-test/` and removes them when the session stops unless explicitly retained.

## Quality

```powershell
pnpm quality:ci-policy:test
pnpm quality:ci-policy
pnpm quality:ci-package:test
pnpm quality:ci-package
pnpm quality:layout
pnpm quality:public-surface:test
pnpm quality:public-surface
pnpm quality:talos-ops
pnpm quality:frontend
pnpm quality:dependencies
```

Backend:

```bash
cd backend
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked --no-fail-fast
cargo check --workspace --no-default-features --features postgres --locked
```

## CI

`TALOS Exact-Head Qualification` is the automatic qualification authority for a candidate exact SHA. It runs global/static checks once, frontend/Rust/PostgreSQL quality, and the current Story Packet qualification dispatcher.

Historical milestone qualification is a separate explicit-dispatch workflow pinned to an expected SHA. Legacy chronological replay is not appended to every new Story Packet.

For R-scoped ephemeral public CI, `bash scripts/export-public-qualification-mirror.sh` creates a parentless transport commit from an approved qualification tree. Mirror-mode `workflow_dispatch` binds the private `source_commit_sha` and `source_tree_sha`; the public mirror commit is never treated as implementation authority. `quality:public-surface` must pass before export. Current-package qualification can be selected with `package_branch`; public mirror dispatch does not upload the broad `.talos-evidence` directory by default.

## Repository map

```text
backend/                 backend, migrations and module implementation
frontend/                SPA and UI Lab
scripts/                 validation and operational tooling
policy/qualification/    machine qualification policy/evidence
.talos/                  local TALOS authority/bootstrap material
.github/workflows/       CI qualification workflows
```

Project-local `.agents`, `.claude`, `.codex`, `.harness`, `.opencode` and similar plugin/skill trees are intentionally not part of repository authority.
