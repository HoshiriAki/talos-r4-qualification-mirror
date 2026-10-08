# Frontend Agent Notes

Scope: `frontend/`.

The frontend is Vue 3 + TypeScript + Vite with PrimeVue/UnoCSS and the registry-driven TALOS UI Lab. Project-local Harness/Claude skill packs are retired.

## Commands

```bash
pnpm --dir frontend typecheck
pnpm --dir frontend build
pnpm --dir frontend test
pnpm --dir frontend ui:registry:check
```

## Rules

- Use pnpm; do not create npm lockfiles.
- Keep component identity and UI Lab behavior registry-driven.
- Preserve tenant/runtime boundaries; frontend code must not invent backend authority.
- Keep test/runtime artifacts out of version control.
- Use the root `AGENTS.md`, current frontend source/tests and TALOS Knowledge for architecture context.
