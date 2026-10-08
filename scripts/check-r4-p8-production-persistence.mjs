#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(SCRIPT_DIR, '..');

const PATHS = {
  config: 'backend/src/config.rs',
  main: 'backend/src/main.rs',
  inventory: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p8-production-persistence-inventory.md',
};

const SCAN_ROOTS = ['backend/src', 'backend/official', 'backend/system'];
const SQLITE_MARKERS = [
  /\brusqlite\b/,
  /\bSqliteConnectionManager\b/,
  /\bSqliteRepositoryProvider\b/,
  /\bSqliteRepositorySession\b/,
  /\bcreate_pool\s*\(/,
];

// Transitional leaf-level guards may inspect DB_BACKEND only to reject a
// PostgreSQL profile before using their still-SQLite implementation. They do
// not choose a backend. P8-C/P8-D must eventually remove these guards by
// providing PostgreSQL ownership; no new reader may be added to this allowlist
// without explicit P8 evidence.
const DEFENSIVE_DB_BACKEND_READERS = new Set([
  'backend/system/tenant-governance/src/lib.rs',
  'backend/system/tenant-preview/src/lib.rs',
  'backend/system/tenant-simulation/src/lib.rs',
]);

const CLASSIFICATION_RULES = [
  {
    id: 'startup-composition',
    category: 'Startup/Worker',
    status: 'cutover-required',
    match: (file) => file === 'backend/src/main.rs' || file === 'backend/src/state.rs',
  },
  {
    id: 'sqlite-local-infrastructure',
    category: 'Deployment/Recovery hook',
    status: 'local-only',
    match: (file) => file === 'backend/src/db/pool.rs' || file.startsWith('backend/src/repositories/sqlite/'),
  },
  {
    id: 'dual-backend-migrations',
    category: 'Deployment/Recovery hook',
    status: 'dual-backend',
    match: (file) => file.startsWith('backend/src/db/'),
  },
  {
    id: 'scoped-repository-contract',
    category: 'ScopedRepository',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/src/repositories/'),
  },
  {
    id: 'auth-security',
    category: 'Auth/Security',
    status: 'cutover-required',
    match: (file) =>
      file === 'backend/src/middleware/tenant.rs' ||
      /^backend\/src\/services\/(auth|session|totp|api_key)/.test(file) ||
      file.startsWith('backend/system/admin/src/two_fa'),
  },
  {
    id: 'audit-persistence',
    category: 'Audit/Observability persistence',
    status: 'cutover-required',
    match: (file) => file.includes('/audit') || file.includes('observability'),
  },
  {
    id: 'error-compatibility-adapter',
    category: 'Compatibility/Legacy',
    status: 'cutover-required',
    match: (file) => file === 'backend/src/error.rs',
  },
  {
    id: 'registry-runtime',
    category: 'Compatibility/Legacy',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/src/registry/'),
  },
  {
    id: 'integration-webhook',
    category: 'Integration/Webhook',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/src/integration/'),
  },
  {
    id: 'interconnect-plugin-worker',
    category: 'Interconnect/Event/Work/State',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/src/application/'),
  },
  {
    id: 'compatibility-services-routes',
    category: 'Compatibility/Legacy',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/src/services/') || file.startsWith('backend/src/routes/'),
  },
  {
    id: 'official-modules',
    category: 'Compatibility/Legacy',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/official/'),
  },
  {
    id: 'system-modules',
    category: 'Compatibility/Legacy',
    status: 'cutover-required',
    match: (file) => file.startsWith('backend/system/'),
  },
];

function read(relativePath) {
  return fs.readFileSync(path.join(ROOT, relativePath), 'utf8');
}

function walk(relativeRoot) {
  const absoluteRoot = path.join(ROOT, relativeRoot);
  if (!fs.existsSync(absoluteRoot)) return [];
  const output = [];
  const stack = [absoluteRoot];
  while (stack.length > 0) {
    const current = stack.pop();
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const absolute = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(absolute);
      } else if (entry.isFile() && entry.name.endsWith('.rs')) {
        output.push(path.relative(ROOT, absolute).split(path.sep).join('/'));
      }
    }
  }
  return output.sort();
}

function classify(file) {
  return CLASSIFICATION_RULES.find((rule) => rule.match(file)) ?? null;
}

function sqliteCandidates() {
  const files = SCAN_ROOTS.flatMap(walk);
  return files
    .map((file) => ({ file, content: read(file) }))
    .filter(({ content }) => SQLITE_MARKERS.some((marker) => marker.test(content)))
    .map(({ file }) => {
      const rule = classify(file);
      return {
        file,
        ruleId: rule?.id ?? null,
        category: rule?.category ?? null,
        status: rule?.status ?? null,
      };
    });
}

function rawBackendReaders() {
  const files = SCAN_ROOTS.flatMap(walk);
  return files.filter((file) => {
    if (file === PATHS.config) return false;
    return /(?:std::)?env::var\(\s*"DB_BACKEND"\s*\)/.test(read(file));
  });
}

export function collectProductionPersistenceSnapshot() {
  const readers = rawBackendReaders();
  return {
    config: read(PATHS.config),
    main: read(PATHS.main),
    inventory: read(PATHS.inventory),
    candidates: sqliteCandidates(),
    rawBackendReaders: readers,
    defensiveBackendReaders: Object.fromEntries(
      readers
        .filter((file) => DEFENSIVE_DB_BACKEND_READERS.has(file))
        .map((file) => [file, read(file)]),
    ),
  };
}

export function validateProductionPersistenceSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'pub enum DatabaseProfile',
    'SqliteLocal',
    'Postgres18',
    'let default_backend = if is_production { "postgres" } else { "sqlite" }',
    'production requires the PostgreSQL 18 database profile',
  ]) {
    if (!snapshot.config.includes(token)) {
      errors.push(`database profile contract missing: ${token}`);
    }
  }

  const unauthorizedReaders = snapshot.rawBackendReaders.filter(
    (file) => !DEFENSIVE_DB_BACKEND_READERS.has(file),
  );
  if (unauthorizedReaders.length > 0) {
    errors.push(
      `DB_BACKEND authority escaped config.rs: ${unauthorizedReaders.join(', ')}`,
    );
  }

  for (const [file, source] of Object.entries(snapshot.defensiveBackendReaders ?? {})) {
    if (!source.includes('eq_ignore_ascii_case("postgres")')) {
      errors.push(`defensive DB_BACKEND reader no longer detects PostgreSQL: ${file}`);
    }
    if (!source.includes('SYS_BACKEND_UNSUPPORTED')) {
      errors.push(`defensive DB_BACKEND reader no longer fails closed: ${file}`);
    }
  }

  for (const token of [
    'database_profile_from_env(config.is_production)',
    'DatabaseProfile::Postgres18',
    'if config.is_production && pg_pool.is_none()',
    'R4-P5 production webhook Event Lane requires PostgreSQL 18 durable driver',
    'let sqlite_pool = if database_profile == DatabaseProfile::SqliteLocal',
    'create_pool(&config.db_path)',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`trusted composition root missing P8 admission token: ${token}`);
    }
  }

  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('transitional PostgreSQL admission barrier must be removed after P8-D closure');
  }

  const productionPgGuard = snapshot.main.indexOf(
    'if config.is_production && pg_pool.is_none()',
  );
  const sentry = snapshot.main.indexOf('sentry::init');
  const sqliteOpen = snapshot.main.indexOf('create_pool(&config.db_path)');
  const pgMigrations = snapshot.main.indexOf('db::run_all_pg_migrations(&pool).await');
  if (productionPgGuard < 0 || sentry < 0 || productionPgGuard > sentry) {
    errors.push('production PostgreSQL authority must fail closed before telemetry/application side effects');
  }
  if (productionPgGuard < 0 || sqliteOpen < 0 || productionPgGuard > sqliteOpen) {
    errors.push('production PostgreSQL authority must be established before any local SQLite capability can open');
  }
  if (pgMigrations < 0 || productionPgGuard < 0 || pgMigrations > productionPgGuard) {
    errors.push('PostgreSQL migrations must complete before production runtime admission');
  }

  const createCalls = snapshot.main.match(/create_pool\(&config\.db_path\)\?/g) ?? [];
  if (createCalls.length !== 1) {
    errors.push('composition root must retain exactly one SQLite pool creation site');
  }

  if (snapshot.main.includes('pg_pool.is_none().then(||')) {
    errors.push('composition root must not infer SQLite fallback from PostgreSQL absence');
  }

  const unclassified = snapshot.candidates.filter((candidate) => !candidate.ruleId);
  if (unclassified.length > 0) {
    errors.push(
      `unclassified SQLite persistence consumers: ${unclassified.map((entry) => entry.file).join(', ')}`,
    );
  }

  const ruleIds = new Set(snapshot.candidates.map((candidate) => candidate.ruleId).filter(Boolean));
  for (const ruleId of ruleIds) {
    if (!snapshot.inventory.includes(`\`${ruleId}\``)) {
      errors.push(`inventory does not document active classification rule ${ruleId}`);
    }
  }

  for (const requiredCategory of [
    'ScopedRepository',
    'Auth/Security',
    'Integration/Webhook',
    'Interconnect/Event/Work/State',
    'Audit/Observability persistence',
    'Compatibility/Legacy',
    'Startup/Worker',
    'Deployment/Recovery hook',
  ]) {
    if (!snapshot.inventory.includes(requiredCategory)) {
      errors.push(`inventory missing required P8 category ${requiredCategory}`);
    }
  }

  return errors;
}

function main() {
  const snapshot = collectProductionPersistenceSnapshot();
  const errors = validateProductionPersistenceSnapshot(snapshot);
  if (errors.length > 0) {
    console.error('R4-P8 production persistence gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }

  const grouped = new Map();
  for (const candidate of snapshot.candidates) {
    const key = `${candidate.category} / ${candidate.status}`;
    const entries = grouped.get(key) ?? [];
    entries.push(candidate.file);
    grouped.set(key, entries);
  }

  console.log(`R4-P8 persistence inventory classified ${snapshot.candidates.length} SQLite-bearing Rust files.`);
  for (const [key, files] of [...grouped.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    console.log(`- ${key}: ${files.length}`);
    for (const file of files) console.log(`  - ${file}`);
  }
  if (snapshot.rawBackendReaders.length > 0) {
    console.log('Transitional defensive DB_BACKEND readers:');
    for (const file of snapshot.rawBackendReaders) console.log(`  - ${file}`);
  }
  console.log('R4-P8 production persistence gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
