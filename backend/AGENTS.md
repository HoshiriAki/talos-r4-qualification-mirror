# Backend Agent Notes

Scope: `backend/`.

The backend is the Rust/Axum implementation. Repository, execution-context, tenant and migration boundaries are enforced by code/tests and the root quality gates; no project-local Harness authority exists.

## Commands

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked --no-fail-fast
cargo check --workspace --no-default-features --features postgres --locked
```

## Rules

- Keep tenant authority derived from trusted execution context.
- Keep production persistence behind approved repository/provider boundaries.
- Do not introduce route-layer SQL or raw tenant fallbacks.
- Update both SQLite/PostgreSQL migration authorities when a migration contract requires parity.
- Preserve exact error/security semantics covered by existing tests.
- Use the root `AGENTS.md`, current code/tests and TALOS Knowledge for architecture context.
