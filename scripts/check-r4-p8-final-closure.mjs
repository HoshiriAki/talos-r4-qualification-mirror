#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  performance: 'backend/src/repositories/postgres/p8f_performance_qualification_tests.rs',
  pgMod: 'backend/src/repositories/postgres/mod.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
  handoff: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p8f-performance-recovery-handoff.md',
  story: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p8-postgresql18-production-cutover-story-packet.md',
  deployment: 'policy/qualification/legacy-evidence/docs/deployment.md',
  drPlan: 'policy/qualification/legacy-evidence/docs/dr-plan.md',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP8FinalClosureSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateP8FinalClosureSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'mod p8f_performance_qualification_tests;',
  ]) {
    if (!snapshot.pgMod.includes(token)) {
      errors.push('P8-F PostgreSQL performance qualification module is not registered')
    }
  }

  for (const token of [
    'const PERF_ORDER_COUNT: i64 = 500;',
    'const READ_SAMPLES: usize = 30;',
    'const WRITE_SAMPLES: usize = 15;',
    'const ORDER_LIST_P95_LIMIT: Duration = Duration::from_millis(500);',
    'const IMPORTED_DRAFT_P95_LIMIT: Duration = Duration::from_millis(1_000);',
    'live_pg18_p8f_performance_baseline_bounds_order_read_and_transactional_write',
    "current_setting('server_version_num')",
    'pg_is_in_recovery()',
    'P8F_PERF_BASELINE order_list',
    'P8F_PERF_BASELINE imported_draft',
  ]) {
    if (!snapshot.performance.includes(token)) {
      errors.push(`P8-F performance baseline invariant missing: ${token}`)
    }
  }

  for (const token of [
    'PostgreSQL 18 bounded P8-F performance baseline',
    'live_pg18_p8f_performance_baseline_bounds_order_read_and_transactional_write',
    'p8f-pg18-performance-${{ github.run_id }}-${{ github.run_attempt }}',
    'P8F_PERF_BASELINE',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push(`Exact-Head P8-F performance evidence wiring missing: ${token}`)
    }
  }

  for (const token of [
    'not production SLOs',
    'P8-F only freezes this contract',
    'scripts/backup.sh',
    'scripts/db-backup.js',
    'scripts/dr-restore.sh',
    'must **not** be used as PostgreSQL 18 production backup/restore evidence',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ]) {
    if (!snapshot.handoff.includes(token)) {
      errors.push(`P8-F recovery handoff invariant missing: ${token}`)
    }
  }

  if (!snapshot.story.includes('r4-p8f-performance-recovery-handoff.md')) {
    errors.push('P8 story packet must bind the normative P8-F handoff')
  }
  if (!snapshot.story.includes('P8F_PERF_BASELINE')) {
    errors.push('P8 story packet must require same-head P8-F performance evidence')
  }

  for (const token of [
    'SQLite-only/local legacy tooling',
    '不得用于 PostgreSQL 18 production backup/restore',
    'r4-p8f-performance-recovery-handoff.md',
  ]) {
    if (!snapshot.deployment.includes(token)) {
      errors.push(`Deployment PostgreSQL recovery boundary missing: ${token}`)
    }
  }

  for (const token of [
    'SQLite-only/local legacy tooling',
    'No current SQLite restore script is valid for this path.',
    'pg_dump --format=custom --no-owner --no-acl',
    'restore to fresh isolated PostgreSQL 18',
    'MIGRATION_BACKUP_RESTORE_ROLLBACK_PASS',
  ]) {
    if (!snapshot.drPlan.includes(token)) {
      errors.push(`DR/P10 handoff invariant missing: ${token}`)
    }
  }

  return errors
}

function main() {
  const errors = validateP8FinalClosureSnapshot(collectP8FinalClosureSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 final closure gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 final closure gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
